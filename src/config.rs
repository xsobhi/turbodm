//! User settings (JSON in ~/.config/turbodm) and standard file locations.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "io.github.xsobhi.TurboDM";
pub const NATIVE_HOST: &str = "com.xsobhi.turbodm";
pub const FIREFOX_EXTENSION_ID: &str = "turbodm@xsobhi.github.io";
pub const CHROME_EXTENSION_ID: &str = "edlkglikdjabdhlailjmdopnlocdegbe";
pub const MAX_CONNECTIONS: usize = 32;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn own_user_agent() -> String {
    format!("TurboDM/{VERSION}")
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("turbodm")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("turbodm")
}

pub fn downloads_file() -> PathBuf {
    data_dir().join("downloads.json")
}

/// Where the running app listens: a user-only Unix socket, or a named pipe on Windows.
#[cfg(windows)]
pub fn socket_path() -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\turbodm-{}", user_name()))
}

#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    match dirs::runtime_dir() {
        Some(dir) => dir.join("turbodm").join("ipc.sock"),
        None => PathBuf::from(format!("/tmp/turbodm-{}/ipc.sock", user_name())),
    }
}

fn user_name() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "user".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub use_categories: bool,
    pub connections: usize,        // per download, 1-32
    pub max_parallel: usize,       // downloads running at once
    pub speed_limit_kib: u64,      // global, 0 = unlimited
    pub retries: u32,
    pub timeout_secs: u64,         // no data for this long -> reconnect
    pub min_split_kib: u64,        // smallest piece worth another connection
    pub show_add_dialog: bool,     // confirm downloads caught from the browser
    pub predownload: bool,         // start downloading while that dialog is open, like IDM
    pub show_progress_window: bool, // IDM's per-download window, opened on start/resume
    pub show_complete_dialog: bool,
    pub notify_complete: bool,
    pub clipboard_monitor: bool,
    pub auto_resume: bool,         // resume unfinished downloads at start
    pub check_updates: bool,       // ask GitHub for a newer release, at start and daily
    pub skipped_update: String,    // "Skip this version" in the update dialog
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: dirs::download_dir().unwrap_or_else(|| home().join("Downloads")),
            use_categories: true,
            connections: 8, // like IDM: many servers block more per IP (up to 32)
            max_parallel: 3,
            speed_limit_kib: 0,
            retries: 10,
            timeout_secs: 30,
            min_split_kib: 1024,
            show_add_dialog: true,
            predownload: true,
            show_progress_window: true,
            show_complete_dialog: true,
            notify_complete: true,
            clipboard_monitor: false,
            auto_resume: false,
            check_updates: true,
            skipped_update: String::new(),
        }
    }
}

impl Settings {
    pub fn clamp(mut self) -> Self {
        self.connections = self.connections.clamp(1, MAX_CONNECTIONS);
        self.max_parallel = self.max_parallel.clamp(1, 16);
        self.retries = self.retries.min(100);
        self.timeout_secs = self.timeout_secs.clamp(5, 300);
        self.min_split_kib = self.min_split_kib.clamp(64, 65536);
        self
    }

    pub fn file() -> PathBuf {
        config_dir().join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::file())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_default()
            .clamp()
    }

    pub fn save(&self) -> std::io::Result<()> {
        write_json_atomic(&Self::file(), &self.clone().clamp())
    }
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(tmp, path)
}
