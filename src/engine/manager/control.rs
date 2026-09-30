//! Controlling downloads: resume/pause, remove, refresh address, settings, queries.

use super::Manager;
use crate::config::Settings;
use crate::engine::state::{self, Snapshot};
use crate::engine::task::{Status, Task};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

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

    pub fn snapshot(&self) -> Vec<Snapshot> {
        self.all().iter().map(|t| state::snapshot(t)).collect()
    }

    pub fn get(&self, id: &str) -> Option<Snapshot> {
        self.find(id).map(|t| state::snapshot(&t))
    }

    pub fn total_speed(&self) -> f64 {
        self.all().iter().map(|t| t.speed()).sum()
    }

    pub fn busy(&self) -> bool {
        self.all().iter().any(|t| t.status().is_active() || t.status() == Status::Queued)
    }

}
