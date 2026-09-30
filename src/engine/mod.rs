//! Download engine: segmented multi-connection HTTP downloads on tokio.

pub mod http;
pub mod limiter;
pub mod manager;
mod runner;
pub mod segments;
pub mod state;
pub mod task;
mod worker;

use crate::config::Settings;
use limiter::RateLimiter;
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
}

impl Shared {
    pub fn new(settings: Settings) -> Self {
        Shared {
            client: RwLock::new(http::build_client(Duration::from_secs(settings.timeout_secs))),
            limiter: Arc::new(RateLimiter::new(settings.speed_limit_kib * 1024)),
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
        *self.settings.write().unwrap() = settings;
    }
}
