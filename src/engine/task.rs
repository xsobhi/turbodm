//! A download: persistent info, live segment map, speed meter and pause control.

use super::http::Headers;
use super::limiter::RateLimiter;
use super::segments::SegmentMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Queued,
    Connecting,
    Downloading,
    Paused,
    Completed,
    Error,
}

impl Status {
    pub fn is_active(self) -> bool {
        matches!(self, Status::Connecting | Status::Downloading)
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Queued => "Queued",
            Status::Connecting => "Connecting…",
            Status::Downloading => "Downloading",
            Status::Paused => "Paused",
            Status::Completed => "Complete",
            Status::Error => "Error",
        }
    }
}

/// Everything about a download that is saved to disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskInfo {
    pub url: String,
    pub download_url: String,
    pub directory: PathBuf,
    pub filename: Option<String>,
    pub referrer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub connections: usize,
    pub size: Option<u64>,
    pub resumable: bool,
    pub etag: Option<String>,
    pub status: Status,
    pub error: Option<String>,
    pub added: u64,
    pub completed: Option<u64>,
    pub category_base: Option<PathBuf>, // choose the category folder once the name is known
    #[serde(default)]
    pub speed_limit_kib: u64, // this download only, 0 = unlimited
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl TaskInfo {
    pub fn new(url: String, directory: PathBuf, connections: usize) -> Self {
        TaskInfo {
            download_url: url.clone(),
            url,
            directory,
            filename: None,
            referrer: None,
            cookies: None,
            user_agent: None,
            connections,
            size: None,
            resumable: false,
            etag: None,
            status: Status::Queued,
            error: None,
            added: unix_now(),
            completed: None,
            category_base: None,
            speed_limit_kib: 0,
        }
    }

    pub fn headers(&self) -> Headers {
        Headers {
            user_agent: self.user_agent.clone(),
            referrer: self.referrer.clone(),
            cookies: self.cookies.clone(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.directory.join(self.filename.as_deref().unwrap_or("download"))
    }
}

pub struct Task {
    pub id: String,
    pub info: Mutex<TaskInfo>,
    pub segments: Mutex<Option<Arc<SegmentMap>>>,
    pub cancel: Mutex<CancellationToken>,
    pub runner: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub limiter: Arc<RateLimiter>,
    /// Started while its "Download file info" dialog is still open (like IDM): not listed,
    /// not saved, no events, until the dialog confirms it.
    pub pending: AtomicBool,
    /// Wakes the running download to add connections up to `info.connections`.
    pub grow: tokio::sync::Notify,
    samples: Mutex<VecDeque<(Instant, u64)>>,
    speed: Mutex<f64>,
}

impl Task {
    pub fn new(id: String, info: TaskInfo, segments: Option<SegmentMap>) -> Arc<Self> {
        Arc::new(Task {
            id,
            limiter: Arc::new(RateLimiter::new(info.speed_limit_kib * 1024)),
            info: Mutex::new(info),
            segments: Mutex::new(segments.map(Arc::new)),
            cancel: Mutex::new(CancellationToken::new()),
            runner: Mutex::new(None),
            pending: AtomicBool::new(false),
            grow: tokio::sync::Notify::new(),
            samples: Mutex::new(VecDeque::new()),
            speed: Mutex::new(0.0),
        })
    }

    pub fn status(&self) -> Status {
        self.info.lock().unwrap().status
    }

    pub fn set_status(&self, status: Status) {
        self.info.lock().unwrap().status = status;
    }

    pub fn downloaded(&self) -> u64 {
        let info = self.info.lock().unwrap();
        if info.status == Status::Completed
            && let Some(size) = info.size {
                return size;
            }
        drop(info);
        self.segments.lock().unwrap().as_ref().map_or(0, |s| s.total_done())
    }

    pub fn speed(&self) -> f64 {
        *self.speed.lock().unwrap()
    }

    /// Update the speed from a 2-second sliding window (called 10 times a second).
    pub fn tick(&self, now: Instant) {
        let total = self.downloaded();
        let mut samples = self.samples.lock().unwrap();
        samples.push_back((now, total));
        while samples.len() > 2 && now.duration_since(samples[0].0).as_secs_f64() > 2.0 {
            samples.pop_front();
        }
        let (t0, b0) = samples[0];
        let elapsed = now.duration_since(t0).as_secs_f64();
        let downloading = self.status() == Status::Downloading;
        let speed = if downloading && elapsed > 0.0 { (total - b0.min(total)) as f64 / elapsed } else { 0.0 };
        *self.speed.lock().unwrap() = speed;
        if let Some(map) = self.segments.lock().unwrap().as_ref() {
            map.set_speed(speed / map.active_count().max(1) as f64);
        }
        if !downloading {
            samples.clear(); // start a fresh window when downloading resumes
        }
    }

    pub fn eta(&self) -> Option<f64> {
        let size = self.info.lock().unwrap().size?;
        let speed = self.speed();
        (speed > 0.0).then(|| size.saturating_sub(self.downloaded()) as f64 / speed)
    }

    /// Record the first error and stop every connection.
    pub fn fail(&self, message: &str) {
        let mut info = self.info.lock().unwrap();
        if info.error.is_none() {
            info.error = Some(message.to_string());
        }
        drop(info);
        self.cancel.lock().unwrap().cancel();
    }

    pub fn pause(&self) {
        self.cancel.lock().unwrap().cancel();
    }
}
