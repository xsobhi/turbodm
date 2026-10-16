//! The helpers videos need, in the data folder's `tools`: yt-dlp (ytdlp.rs) and, on Windows,
//! FFmpeg (ffmpeg.rs), each checked against its release's checksums. On Windows the user is asked
//! first, and TurboDM downloads them like any file (setup.rs); then they keep themselves up to
//! date. Linux uses the system's (`yt-dlp`, `ffmpeg` on PATH), else fetches yt-dlp quietly.

mod ffmpeg;
mod setup;
mod ytdlp;

pub use ffmpeg::{fetch_ffmpeg, ffmpeg, ffmpeg_exe, update_ffmpeg, CAN_FETCH_FFMPEG, NO_FFMPEG};
pub use setup::{helpers, incoming, install, missing, Helper};
pub use ytdlp::{update, yt_dlp, yt_dlp_now};

use crate::config::{data_dir, own_user_agent};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

pub fn dir() -> PathBuf {
    data_dir().join("tools")
}

/// A program on PATH.
fn on_path(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{EXE}");
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(&file)).find(|p| p.is_file())
}

async fn get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, String> {
    client.get(url).header("User-Agent", own_user_agent()).send().await
        .and_then(|r| r.error_for_status()).map_err(|e| e.to_string())
}

/// The SHA-256 a checksums file ("<hex>  <name>" lines) gives for `name`.
fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

/// How far a fetch got (bytes), for the window that waits for it.
#[derive(Debug, Default)]
pub struct Fetch {
    pub done: AtomicU64,
    pub total: AtomicU64, // 0: not known yet
}

impl Fetch {
    pub fn get(&self) -> (u64, Option<u64>) {
        let total = self.total.load(Ordering::Relaxed);
        (self.done.load(Ordering::Relaxed), (total > 0).then_some(total))
    }
}

/// Save `url` to `path`, checking its SHA-256 on the way; nothing is left behind if it's wrong.
async fn download(client: &reqwest::Client, url: &str, path: &Path, sha256: &str, progress: Option<&Fetch>)
                  -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let resp = get(client, url).await?;
    if let (Some(p), Some(total)) = (progress, resp.content_length()) {
        p.total.store(total, Ordering::Relaxed);
    }
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut body = resp.bytes_stream();
    let result: Result<(), String> = async {
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            hash.update(&chunk);
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            if let Some(p) = progress {
                p.done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
        }
        file.flush().map_err(|e| e.to_string())
    }.await;
    drop(file);
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    match result {
        Ok(()) if got == sha256 => Ok(()),
        other => {
            let _ = std::fs::remove_file(path);
            other.and(Err("it didn't arrive intact (wrong checksum), try again".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_checksums() {
        let sums = "1fa6733c37ea6fb51c99ad8fe785e7b7e5f3246c9b980230329d4fb72ed8d4d6  yt-dlp\n\
                    66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a  yt-dlp.exe\n\
                    58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a  yt-dlp_linux\n";
        assert_eq!(checksum_for(sums, "yt-dlp.exe").as_deref(),
                   Some("66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a"));
        assert_eq!(checksum_for(sums, "yt-dlp_linux").as_deref(),
                   Some("58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a"));
        assert_eq!(checksum_for(sums, "yt-dlp_macos"), None);
        // sha256sum's binary-mode star
        assert!(checksum_for("abababababababababababababababababababababababababababababababab *x.zip", "x.zip").is_some());
    }
}
