//! Download engine: segmented multi-connection HTTP downloads on tokio, and torrents.

pub mod http;
pub mod limiter;
pub mod manager;
mod runner;
mod disk;
mod pool;
pub mod segments;
pub mod state;
pub mod task;
pub mod video;
pub mod torrent;
mod worker;

use crate::config::Settings;
use limiter::RateLimiter;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub use manager::{AddRequest, Manager};
pub use state::Snapshot;
pub use task::Status;

/// Engine-wide state shared by every download.
pub struct Shared {
    client: RwLock<reqwest::Client>,
    settings: RwLock<Settings>,
    pub limiter: Arc<RateLimiter>,
    /// The torrent session, kept in `torrent_dir`.
    pub torrents: torrent::Torrents,
}

impl Shared {
    pub fn new(settings: Settings, torrent_dir: PathBuf) -> Self {
        Shared {
            client: RwLock::new(http::build_client(Duration::from_secs(settings.timeout_secs))),
            limiter: Arc::new(RateLimiter::new(settings.speed_limit_kib * 1024)),
            torrents: torrent::Torrents::new(torrent_dir, settings.speed_limit_kib * 1024),
            settings: RwLock::new(settings),
        }
    }

    pub fn client(&self) -> reqwest::Client {
        self.client.read().unwrap().clone()
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn set_settings(&self, settings: Settings) {
        let old_timeout = self.settings().timeout_secs;
        if settings.timeout_secs != old_timeout {
            *self.client.write().unwrap() = http::build_client(Duration::from_secs(settings.timeout_secs));
        }
        self.limiter.set_rate(settings.speed_limit_kib * 1024);
        self.torrents.set_speed_limit(settings.speed_limit_kib * 1024);
        *self.settings.write().unwrap() = settings;
    }
}
