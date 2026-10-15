//! Controlling downloads: resume/pause, remove, refresh address, settings, queries.

use super::{AddRequest, Manager};
use crate::config::Settings;
use crate::engine::state::{self, Snapshot};
use crate::engine::task::{Status, Task};
use crate::engine::torrent;
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
            if task.is_torrent() {
                torrent::remove(&me.shared, &task, delete_files).await;
            } else if delete_files {
                state::remove_files(&task);
            }
            me.tasks.lock().unwrap().retain(|t| t.id != task.id);
            me.statuses.lock().unwrap().remove(&task.id);
            me.dirty.store(true, Ordering::Relaxed);
        });
    }

    /// Swap in a fresh link for an expired one; progress is kept if it's the same file.
    pub fn refresh_address(self: &Arc<Self>, id: &str, url: String) {
        self.refresh_from(id, AddRequest { url, ..AddRequest::default() });
    }

    /// Swap in what the browser sent for the same file again (IDM's "Refresh download address":
    /// the page's download link clicked once more): its link, and its cookies, referrer and user
    /// agent when it has them; then go on from where it was.
    pub fn refresh_from(self: &Arc<Self>, id: &str, fresh: AddRequest) {
        let Some(task) = self.find(id) else { return };
        let me = self.clone();
        self.rt.spawn(async move {
            let was_active = task.status().is_active();
            me.stop_and_wait(&task).await;
            {
                let mut guard = task.info.lock().unwrap();
                let info = &mut *guard; // (its fields borrowed apart)
                (info.url, info.download_url, info.error) = (fresh.url.clone(), fresh.url, None);
                for (now, new) in [(&mut info.cookies, fresh.cookies), (&mut info.referrer, fresh.referrer),
                                   (&mut info.user_agent, fresh.user_agent)] {
                    if new.is_some() {
                        *now = new;
                    }
                }
            }
            if was_active || matches!(task.status(), Status::Error | Status::Paused) {
                me.start_task(&task);
            }
            me.dirty.store(true, Ordering::Relaxed);
        });
    }

    /// Start over: the file is deleted and downloaded again from the beginning, from the same
    /// link with the same cookies, to the same place (as a new download in the list).
    pub fn redownload(self: &Arc<Self>, id: &str) {
        let Some(task) = self.find(id) else { return };
        let me = self.clone();
        self.rt.spawn(async move {
            me.stop_and_wait(&task).await;
            if task.is_torrent() {
                torrent::remove(&me.shared, &task, true).await; // (else the new copy is "already in the list")
            } else {
                state::remove_files(&task);
            }
            let info = task.info.lock().unwrap().clone();
            me.tasks.lock().unwrap().retain(|t| t.id != task.id);
            me.statuses.lock().unwrap().remove(&task.id);
            me.add(AddRequest { url: info.url, filename: info.filename, directory: Some(info.directory),
                                referrer: info.referrer, cookies: info.cookies, user_agent: info.user_agent,
                                connections: Some(info.connections), start: true, video: info.video });
            me.dirty.store(true, Ordering::Relaxed);
        });
    }

    /// The link, cookies, referrer and user agent a download was added with (to refresh it from
    /// the page it came from).
    pub fn request_of(&self, id: &str) -> Option<AddRequest> {
        let info = self.find(id)?.info.lock().unwrap().clone();
        Some(AddRequest { url: info.url, filename: info.filename, directory: Some(info.directory), referrer: info.referrer,
                          cookies: info.cookies, user_agent: info.user_agent, connections: Some(info.connections),
                          start: false, video: info.video })
    }

    /// Speed limit for one download (KiB/s, 0 = unlimited); applies immediately. (Not to
    /// torrents: they go by the limit for all downloads.)
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
        let seed = (settings.seed != self.settings().seed).then_some(settings.seed);
        self.shared.set_settings(settings);
        if let Some(on) = seed {
            self.share_finished(on);
        }
        self.schedule();
    }

    /// Note which torrent each saved download has (two downloads never share one).
    pub(super) fn claim_torrents(&self) {
        for task in self.all() {
            let info = task.info.lock().unwrap();
            if let Some(hash) = info.torrent.as_ref().and_then(|t| t.info_hash.as_deref()) {
                self.shared.torrents.claim(hash, &task.id);
            }
        }
    }

    /// Keep sharing finished torrents (the "seed" setting), or stop.
    pub(super) fn share_finished(&self, on: bool) {
        let done: Vec<Arc<Task>> = self.all().into_iter()
            .filter(|t| t.is_torrent() && t.status() == Status::Completed).collect();
        if !done.is_empty() {
            let shared = self.shared.clone();
            self.rt.spawn(async move { torrent::share(&shared, done, on).await });
        }
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
