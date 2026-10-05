//! The download manager: queue, parallel-download limit, persistence, events.

mod confirm;
mod control;

pub use confirm::Confirm;

use super::state::{self, Snapshot};
use super::task::{Status, Task, TaskInfo};
use super::{runner, Shared};
use crate::categories::target_dir;
use crate::config::Settings;
use crate::util::{filename_from_url, sanitize_filename, unique_path};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default)]
pub struct AddRequest {
    pub url: String,
    pub filename: Option<String>,
    pub directory: Option<PathBuf>,
    pub referrer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub connections: Option<usize>,
    pub start: bool,
}

pub struct Manager {
    pub shared: Arc<Shared>,
    tasks: Mutex<Vec<Arc<Task>>>,
    statuses: Mutex<HashMap<String, Status>>,
    rt: Handle,
    events: async_channel::Sender<Snapshot>,
    dirty: AtomicBool,
    stopped: AtomicBool,
    next_id: AtomicU64,
    store: PathBuf,
}

impl Manager {
    /// Load saved downloads and start the monitor. Events carry status changes.
    pub fn new(settings: Settings, store: PathBuf, rt: Handle) -> (Arc<Self>, async_channel::Receiver<Snapshot>) {
        let tasks = state::load(&store, settings.auto_resume, settings.min_split_kib * 1024);
        let statuses = tasks.iter().map(|t| (t.id.clone(), t.status())).collect();
        let (tx, rx) = async_channel::unbounded();
        let manager = Arc::new(Manager {
            shared: Arc::new(Shared::new(settings)),
            tasks: Mutex::new(tasks),
            statuses: Mutex::new(statuses),
            rt: rt.clone(),
            events: tx,
            dirty: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            next_id: AtomicU64::new(0),
            store,
        });
        rt.spawn(manager.clone().monitor());
        manager.schedule();
        (manager, rx)
    }

    pub fn add(self: &Arc<Self>, req: AddRequest) -> String {
        self.insert(req, false)
    }

    /// Start downloading behind the add dialog while the user looks at it, like IDM: on one
    /// connection, so it's already connected and under way when they press Start, without
    /// using much bandwidth. Hidden until `confirm`; `remove(id, true)` throws it away.
    pub fn prefetch(self: &Arc<Self>, req: AddRequest) -> String {
        let id = self.insert(AddRequest { start: false, connections: Some(1), ..req }, true);
        if let Some(task) = self.find(&id) {
            self.start_task(&task); // not queued: the user is waiting for it
        }
        id
    }

    fn insert(self: &Arc<Self>, req: AddRequest, pending: bool) -> String {
        let s = self.shared.settings();
        let name = req.filename.as_deref().filter(|n| !n.is_empty()).map(sanitize_filename);
        let auto_dir = req.directory.is_none();
        let dir = req.directory.clone().unwrap_or_else(|| {
            let guess = name.clone().unwrap_or_else(|| filename_from_url(&req.url));
            target_dir(&s.download_dir, &guess, s.use_categories)
        });
        let mut info = TaskInfo::new(req.url, dir.clone(), req.connections.unwrap_or(s.connections));
        info.filename = name.map(|n| unique_path(&dir.join(n)).file_name().unwrap().to_string_lossy().into_owned());
        if auto_dir && info.filename.is_none() && s.use_categories {
            info.category_base = Some(s.download_dir.clone());
        }
        (info.referrer, info.cookies, info.user_agent) = (req.referrer, req.cookies, req.user_agent);
        info.status = if req.start { Status::Queued } else { Status::Paused };
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let id = format!("{:x}{:x}", nanos.as_nanos(), self.next_id.fetch_add(1, Ordering::Relaxed));
        self.statuses.lock().unwrap().insert(id.clone(), info.status);
        let task = Task::new(id.clone(), info, None);
        task.pending.store(pending, Ordering::Relaxed);
        self.tasks.lock().unwrap().push(task);
        self.dirty.store(true, Ordering::Relaxed);
        self.schedule();
        id
    }

    pub(super) fn find(&self, id: &str) -> Option<Arc<Task>> {
        self.tasks.lock().unwrap().iter().find(|t| t.id == id).cloned()
    }

    pub(super) fn start_task(&self, task: &Arc<Task>) {
        if task.status().is_active() || task.status() == Status::Completed {
            return;
        }
        let token = CancellationToken::new();
        *task.cancel.lock().unwrap() = token.clone();
        {
            let mut info = task.info.lock().unwrap();
            info.status = Status::Connecting;
            info.error = None;
        }
        let handle = self.rt.spawn(runner::run(task.clone(), self.shared.clone(), token));
        *task.runner.lock().unwrap() = Some(handle);
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn save(&self) {
        let _ = state::save(&self.store, &self.listed());
        self.dirty.store(false, Ordering::Relaxed);
    }

    /// Pause everything and wait (max ~10 s) before exiting. Call off the runtime.
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.pause_all();
        for task in self.all() {
            let handle = task.runner.lock().unwrap().take();
            if let Some(handle) = handle {
                let wait = async move { tokio::time::timeout(Duration::from_secs(10), handle).await };
                let _ = self.rt.block_on(wait);
            }
            if task.pending.load(Ordering::Relaxed) {
                state::remove_files(&task); // its dialog never confirmed it
            }
        }
        self.save();
    }

    pub(super) fn schedule(&self) {
        let tasks = self.all();
        let running = tasks.iter().filter(|t| t.status().is_active()).count();
        let free = self.settings().max_parallel.saturating_sub(running);
        for task in tasks.iter().filter(|t| t.status() == Status::Queued).take(free) {
            self.start_task(task);
        }
    }

    /// Speeds, status-change events (completion dialog, queue) and autosave, 10 times a second.
    async fn monitor(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        let mut last_save = Instant::now();
        while !self.stopped.load(Ordering::Relaxed) {
            interval.tick().await;
            let now = Instant::now();
            let mut changed = vec![];
            for task in self.all() {
                task.tick(now);
                if task.pending.load(Ordering::Relaxed) {
                    continue; // changes are reported once it's confirmed
                }
                let status = task.status();
                if self.statuses.lock().unwrap().insert(task.id.clone(), status) != Some(status) {
                    changed.push(state::snapshot(&task));
                }
            }
            if !changed.is_empty() {
                self.dirty.store(true, Ordering::Relaxed);
                self.schedule();
            }
            for snap in changed {
                let _ = self.events.try_send(snap);
            }
            let busy = self.all().iter().any(|t| t.status().is_active());
            if (self.dirty.load(Ordering::Relaxed) || busy) && last_save.elapsed().as_secs_f64() > 2.0 {
                self.save();
                last_save = Instant::now();
            }
        }
    }
}
