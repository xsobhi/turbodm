//! FFmpeg, which joins a video's video and sound and makes MP3s: the system's (on PATH), or on
//! Windows BtbN's build, set up once the user agreed (it's big) and then updated now and then
//! without asking.

use super::setup::{size_of, Helper};
use super::{checksum_for, dir, download, get, on_path, Fetch, EXE};
use crate::engine::task::unix_now;
use std::path::{Path, PathBuf};

/// BtbN's nightly build with its libraries apart (~85 MB, half the size of the all-in-one
/// build), from a release whose name stays "latest".
const RELEASE: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
const ASSET: &str = "ffmpeg-master-latest-win64-gpl-shared.zip";

/// How often ours is checked for a newer build (it only copies streams and makes MP3s: no hurry).
const UPDATE_EVERY: u64 = 30 * 24 * 3600;

/// TurboDM can fetch FFmpeg itself (Windows); elsewhere the system has it, or not.
pub const CAN_FETCH_FFMPEG: bool = cfg!(windows);

/// Why the joined qualities aren't offered, where FFmpeg can't be fetched.
pub const NO_FFMPEG: &str = "Higher qualities need FFmpeg to join their video and sound: install ffmpeg \
                             (e.g. sudo apt install ffmpeg) to get them";

fn own() -> PathBuf {
    dir().join("ffmpeg")
}

/// Where FFmpeg is, for yt-dlp's --ffmpeg-location: ours, else the system's (None: none).
pub fn ffmpeg() -> Option<PathBuf> {
    if own().join(format!("ffmpeg{EXE}")).is_file() {
        return Some(own());
    }
    on_path("ffmpeg").and_then(|p| p.parent().map(Path::to_path_buf))
}

/// The ffmpeg program itself.
pub fn ffmpeg_exe() -> Option<PathBuf> {
    ffmpeg().map(|dir| dir.join(format!("ffmpeg{EXE}")))
}

/// The build in BtbN's release now: its checksum.
async fn latest(client: &reqwest::Client) -> Result<String, String> {
    let sums = get(client, &format!("{RELEASE}/checksums.sha256")).await?.text().await.map_err(|e| e.to_string())?;
    checksum_for(&sums, ASSET).ok_or_else(|| "FFmpeg's release has no checksum for it".into())
}

/// Where FFmpeg comes from, for the setup window's download.
pub(super) async fn helper(client: &reqwest::Client) -> Result<Helper, String> {
    let url = format!("{RELEASE}/{ASSET}");
    Ok(Helper { name: "FFmpeg", sha256: latest(client).await?, size: size_of(client, &url).await, url,
                file: ASSET.into(), version: String::new() })
}

/// A checked download of the zip: its programs into place, and which build it is.
pub(super) fn install(zip: &Path, sha256: &str) -> Result<(), String> {
    unpack(zip).map_err(|e| format!("Couldn't unpack FFmpeg: {e}"))?;
    let _ = std::fs::write(dir().join("ffmpeg.sha256"), sha256);
    let _ = std::fs::write(dir().join("ffmpeg.checked"), unix_now().to_string());
    Ok(())
}

/// Fetch FFmpeg into tools/ffmpeg without a window (`turbodm video --ffmpeg`, the quiet
/// updates): the zip, checked, then its programs.
pub async fn fetch_ffmpeg(client: &reqwest::Client, progress: &Fetch) -> Result<(), String> {
    if !CAN_FETCH_FFMPEG {
        return Err(NO_FFMPEG.into());
    }
    static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = ONE.lock().await;
    let expected = latest(client).await?;
    let zip = dir().join("ffmpeg.zip.download");
    download(client, &format!("{RELEASE}/{ASSET}"), &zip, &expected, Some(progress)).await
        .map_err(|e| format!("Couldn't get FFmpeg: {e}"))?;
    tokio::task::spawn_blocking(move || {
        let result = install(&zip, &expected);
        let _ = std::fs::remove_file(&zip);
        result
    }).await.map_err(|e| e.to_string())?
}

/// A newer build of ours, now and then (monthly), quietly. True when one was installed.
pub async fn update_ffmpeg(client: &reqwest::Client) -> Result<bool, String> {
    let checked = std::fs::read_to_string(dir().join("ffmpeg.checked")).ok().and_then(|t| t.trim().parse::<u64>().ok());
    if !own().join(format!("ffmpeg{EXE}")).is_file() || unix_now().saturating_sub(checked.unwrap_or(0)) < UPDATE_EVERY {
        return Ok(false);
    }
    let _ = std::fs::write(dir().join("ffmpeg.checked"), unix_now().to_string());
    let installed = std::fs::read_to_string(dir().join("ffmpeg.sha256")).unwrap_or_default();
    if latest(client).await? == installed.trim() {
        return Ok(false);
    }
    fetch_ffmpeg(client, &Fetch::default()).await.map(|_| true)
}

/// The zip's bin folder (ffmpeg, ffprobe and their DLLs, not ffplay) into tools/ffmpeg, with
/// Windows' own tar (bsdtar reads zips; part of Windows since 10 1803).
fn unpack(zip: &Path) -> std::io::Result<()> {
    let fresh = dir().join("ffmpeg.new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh)?;
    let tar = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| "C:\\Windows".into())
        .join("System32").join("tar.exe");
    let mut command = std::process::Command::new(tar);
    command.arg("-xf").arg(zip).arg("-C").arg(&fresh)
        .args(["--strip-components", "2", "--exclude", "*/ffplay.exe", "*/bin/*"]);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, super::super::process::NO_WINDOW);
    let status = command.status()?;
    if !status.success() || !fresh.join(format!("ffmpeg{EXE}")).is_file() {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(std::io::Error::other("ffmpeg.exe isn't in the download"));
    }
    let _ = std::fs::remove_dir_all(own());
    std::fs::rename(&fresh, own())
}
