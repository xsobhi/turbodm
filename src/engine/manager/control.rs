//! Controlling downloads: resume/pause, remove, refresh address, settings, queries.

use super::Manager;
use crate::config::Settings;
use crate::engine::state::{self, Snapshot};
use crate::engine::task::{Status, Task, TaskInfo};
use crate::util::{part_path, sanitize_filename, unique_path};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// What the add dialog settled on for a download it started early (`Manager::prefetch`).
#[derive(Clone, Debug)]
pub struct Confirm {
    pub filename: Option<String>,
    pub directory: PathBuf,
    pub connections: usize,
    pub start: bool, // false: "Download later" (pause it, keep what's downloaded)
}

impl Manager {
    /// Start now (like IDM's Resume), ignoring the queue limit.
    pub fn resume(&self, id: &str) {
        if let Some(task) = self.find(id) {
            self.start_task(&task);
        }
    }

    pub fn pause(&self, id: &str) {
        let Some(task) = self.find(id) else { return };
        match task.status() {
            s if s.is_active() => task.pause(),
            Status::Queued => task.set_status(Status::Paused),
            _ => {}
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn pause_all(&self) {
        for task in self.all() {
            self.pause(&task.id);
        }
    }

    pub fn resume_all(&self) {
        for task in self.all() {
            if matches!(task.status(), Status::Paused | Status::Error) {
                task.set_status(Status::Queued);
            }
        }
        self.schedule();
    }

    /// Stop the download, wait for it, optionally delete its files, forget it.
    pub fn remove(self: &Arc<Self>, id: &str, delete_files: bool) {
        let Some(task) = self.find(id) else { return };
        let me = self.clone();
        self.rt.spawn(async move {
            me.stop_and_wait(&task).await;
            if delete_files {
                state::remove_files(&task);
            }
            me.tasks.lock().unwrap().retain(|t| t.id != task.id);
            me.statuses.lock().unwrap().remove(&task.id);
            me.dirty.store(true, Ordering::Relaxed);
        });
    }

    /// Swap in a fresh link for an expired one; progress is kept if it's the same file.
    pub fn refresh_address(self: &Arc<Self>, id: &str, url: String) {
        let Some(task) = self.find(id) else { return };
        let me = self.clone();
        self.rt.spawn(async move {
            let was_active = task.status().is_active();
            me.stop_and_wait(&task).await;
            {
                let mut info = task.info.lock().unwrap();
                (info.url, info.download_url, info.error) = (url.clone(), url, None);
            }
            if was_active || task.status() == Status::Error {
                me.start_task(&task);
            }
            me.dirty.store(true, Ordering::Relaxed);
        });
    }

    /// Speed limit for one download (KiB/s, 0 = unlimited); applies immediately.
    pub fn set_speed_limit(&self, id: &str, kib: u64) {
        let Some(task) = self.find(id) else { return };
        task.info.lock().unwrap().speed_limit_kib = kib;
        task.limiter.set_rate(kib * 1024);
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// The add dialog's Start / Download later for a download it started early: move what's
    /// downloaded to the chosen name and folder, then list it like any other download.
    pub fn confirm(self: &Arc<Self>, id: &str, choice: Confirm) {
        let Some(task) = self.find(id) else { return };
        // from the early single connection to all of them, without reconnecting (set right
        // away: the progress window opening next shows that many)
        task.info.lock().unwrap().connections = choice.connections;
        task.grow.notify_one();
        let me = self.clone();
        self.rt.spawn(async move {
            let name = choice.filename.as_deref().filter(|n| !n.trim().is_empty()).map(sanitize_filename);
            if move_files(&task, name.clone(), &choice.directory).is_err() {
                // another disk: stop, then move the file the slow way
                me.stop_and_wait(&task).await;
                let (task2, dir) = (task.clone(), choice.directory.clone());
                let moved = tokio::task::spawn_blocking(move || copy_files(&task2, name, &dir)).await;
                if let Ok(Err(err)) = moved {
                    task.info.lock().unwrap().error = Some(format!("Cannot move the file: {err}"));
                }
            }
            if choice.start {
                me.start_task(&task); // no-op if still running or already complete
            } else {
                me.stop_and_wait(&task).await;
            }
            task.pending.store(false, Ordering::Relaxed); // listed, saved, events from here on
            me.dirty.store(true, Ordering::Relaxed);
        });
    }

    pub(super) async fn stop_and_wait(&self, task: &Task) {
        task.pause();
        let handle = task.runner.lock().unwrap().take();
        if let Some(handle) = handle {
            let _ = tokio::time::timeout(Duration::from_secs(10), handle).await;
        }
    }

    pub fn settings(&self) -> Settings {
        self.shared.settings()
    }

    pub fn update_settings(&self, settings: Settings) {
        let _ = settings.save();
        self.shared.set_settings(settings);
        self.schedule();
    }

    pub(super) fn all(&self) -> Vec<Arc<Task>> {
        self.tasks.lock().unwrap().clone()
    }

    /// Downloads in the list: all but those still waiting in their add dialog.
    pub(super) fn listed(&self) -> Vec<Arc<Task>> {
        self.all().into_iter().filter(|t| !t.pending.load(Ordering::Relaxed)).collect()
    }

    pub fn snapshot(&self) -> Vec<Snapshot> {
        self.snapshot_where(|_, _| true)
    }

    /// Snapshots of only the downloads `keep(id, status)` picks; the rest cost next to nothing.
    pub fn snapshot_where(&self, keep: impl Fn(&str, Status) -> bool) -> Vec<Snapshot> {
        self.listed().iter().filter(|t| keep(&t.id, t.status())).map(|t| state::snapshot(t)).collect()
    }

    /// (all, active, queued) download counts.
    pub fn counts(&self) -> (usize, usize, usize) {
        let statuses: Vec<Status> = self.listed().iter().map(|t| t.status()).collect();
        let count = |f: fn(Status) -> bool| statuses.iter().filter(|&&s| f(s)).count();
        (statuses.len(), count(Status::is_active), count(|s| s == Status::Queued))
    }

    pub fn get(&self, id: &str) -> Option<Snapshot> {
        self.find(id).map(|t| state::snapshot(&t))
    }

    pub fn total_speed(&self) -> f64 {
        self.listed().iter().map(|t| t.speed()).sum()
    }

    pub fn busy(&self) -> bool {
        self.all().iter().any(|t| t.status().is_active() || t.status() == Status::Queued)
    }

}

/// The file (or its unfinished part) now, and as it would be named with this name and folder.
fn target(info: &TaskInfo, name: Option<String>, dir: &Path) -> (PathBuf, PathBuf, Option<String>) {
    let completed = info.status == Status::Completed;
    let file = |p: PathBuf| if completed { p } else { part_path(&p) };
    let name = name.or_else(|| info.filename.clone());
    let old = info.path();
    let mut new = dir.join(name.as_deref().unwrap_or("download"));
    if new != old && (new.exists() || part_path(&new).exists()) {
        new = unique_path(&new);
    }
    let name = name.map(|_| new.file_name().unwrap().to_string_lossy().into_owned());
    (file(old), file(new), name)
}

fn set_place(info: &mut TaskInfo, name: Option<String>, dir: &Path) {
    info.directory = dir.to_path_buf();
    info.category_base = None; // the dialog chose the folder
    if name.is_some() {
        info.filename = name;
    }
}

/// Rename in place, locked so the download can't open the file meanwhile. Fine while
/// downloading (open files follow a rename); fails across disks.
fn move_files(task: &Task, name: Option<String>, dir: &Path) -> std::io::Result<()> {
    let mut info = task.info.lock().unwrap();
    let (old, new, name) = target(&info, name, dir);
    if old != new {
        std::fs::create_dir_all(dir)?;
        match std::fs::rename(&old, &new) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    set_place(&mut info, name, dir);
    Ok(())
}

/// Copy then delete, for a download that is stopped (not locked: the copy can take a while).
fn copy_files(task: &Task, name: Option<String>, dir: &Path) -> std::io::Result<()> {
    let (old, new, name) = target(&task.info.lock().unwrap(), name, dir);
    if old != new && old.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::copy(&old, &new)?;
        std::fs::remove_file(&old)?;
    }
    set_place(&mut task.info.lock().unwrap(), name, dir);
    Ok(())
}
