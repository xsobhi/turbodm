//! User settings (JSON in ~/.config/turbodm) and standard file locations.

use crate::categories::{category_for, target_dir};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "io.github.xsobhi.TurboDM";
pub const NATIVE_HOST: &str = "com.xsobhi.turbodm";
pub const FIREFOX_EXTENSION_ID: &str = "turbodm@xsobhi.github.io";
pub const CHROME_EXTENSION_ID: &str = "edlkglikdjabdhlailjmdopnlocdegbe"; // loaded by hand ("Load unpacked")
/// The extension's IDs in the stores, once it's listed there (each store picks its own). With
/// one, Chromium browsers add it by themselves, asking first, as they do IDM's (register.rs).
pub const CHROME_STORE_ID: Option<&str> = Some("enaaoegoakgmcdbhepcoflfmpeodipob"); // Chrome Web Store: Chrome, Brave, Vivaldi, Opera
pub const EDGE_STORE_ID: Option<&str> = None; // Microsoft Edge Add-ons (else Edge takes Chrome's)
pub const MAX_CONNECTIONS: usize = 32;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every Chromium extension that may talk to TurboDM: the one loaded by hand, and the stores'.
pub fn chrome_extension_ids() -> Vec<&'static str> {
    [Some(CHROME_EXTENSION_ID), CHROME_STORE_ID, EDGE_STORE_ID].into_iter().flatten().collect()
}

pub fn own_user_agent() -> String {
    format!("TurboDM/{VERSION}")
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// `TURBODM_HOME=folder`: a TurboDM of its own (tests, trying a build beside the installed one),
/// with its settings and download list in that folder and its own running instance, leaving
/// the browsers connected to the installed one.
pub fn sandbox() -> Option<PathBuf> {
    std::env::var_os("TURBODM_HOME").filter(|home| !home.is_empty()).map(PathBuf::from)
}

pub fn config_dir() -> PathBuf {
    match sandbox() {
        Some(home) => home.join("config"),
        None => dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("turbodm"),
    }
}

pub fn data_dir() -> PathBuf {
    match sandbox() {
        Some(home) => home.join("data"),
        None => dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("turbodm"),
    }
}

pub fn downloads_file() -> PathBuf {
    data_dir().join("downloads.json")
}

/// Where the running app listens: a user-only Unix socket, or a named pipe on Windows. Each
/// TURBODM_HOME has a pipe of its own, so two sandboxes never take each other for theirs.
#[cfg(windows)]
pub fn socket_path() -> PathBuf {
    let sandboxed = match sandbox() {
        // FNV-1a of the folder: the same name in every process that has it
        Some(home) => format!("-sandbox-{:08x}", home.to_string_lossy().to_lowercase().bytes()
            .fold(0x811c_9dc5_u32, |hash, byte| (hash ^ byte as u32).wrapping_mul(0x0100_0193))),
        None => String::new(),
    };
    PathBuf::from(format!(r"\\.\pipe\turbodm-{}{sandboxed}", user_name()))
}

#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    if let Some(home) = sandbox() {
        return home.join("ipc.sock");
    }
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
    pub folders: BTreeMap<String, PathBuf>, // category → folder ("Always save … here", like IDM)
    pub browsers_offered: bool,    // the "add the extension to your browser" guide was shown
    pub show_categories: bool,     // the main window's categories (View → Categories)
    pub show_toolbar: bool,        // and its toolbar (View → Toolbar)
    pub toolbar_text: bool,        // the toolbar's names under its icons (View → Toolbar text; off: smaller)
    pub video_quality: String,     // the video quality picked last ("720p", "Audio"), offered first next time
    #[serde(default)]
    pub seed: bool,                // keep sharing finished torrents (off: stop once complete)
    #[serde(default)]
    pub video_format: String,      // the video format picked last ("MP4", "WebM", "MKV")
    #[serde(default)]
    pub video_helpers_declined: bool, // "Not now" to getting yt-dlp and FFmpeg: not asked at start again
    pub delete_files_too: bool, // Delete's "Also delete the files", as last answered
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: dirs::download_dir().unwrap_or_else(|| home().join("Downloads")),
            use_categories: false, // (straight into the download folder; Options → Save to sorts them)
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
            folders: BTreeMap::new(),
            browsers_offered: false,
            show_categories: true,
            show_toolbar: true,
            toolbar_text: true,
            video_quality: String::new(),
            seed: false,
            video_format: String::new(),
            video_helpers_declined: false,
            delete_files_too: false,
        }
    }
}

impl Settings {
    /// Where a file of this name goes by default: the folder chosen for its kind of file,
    /// or its category's sub-folder of the download folder.
    pub fn folder_for(&self, filename: &str) -> PathBuf {
        match self.folders.get(category_for(filename)) {
            Some(dir) => dir.clone(),
            None => target_dir(&self.download_dir, filename, self.use_categories),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_folder_wins_for_that_kind_of_file() {
        let mut s = Settings { download_dir: "/dl".into(), use_categories: true, ..Settings::default() };
        s.folders.insert("Documents".into(), "/mnt/d/mydocs".into());
        assert_eq!(s.folder_for("report.pdf"), PathBuf::from("/mnt/d/mydocs"));
        assert_eq!(s.folder_for("movie.mkv"), PathBuf::from("/dl/Video"));
        s.use_categories = false;
        assert_eq!(s.folder_for("notes.docx"), PathBuf::from("/mnt/d/mydocs"));
        assert_eq!(s.folder_for("movie.mkv"), PathBuf::from("/dl"));
    }
}
