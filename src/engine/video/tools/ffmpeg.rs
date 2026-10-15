//! FFmpeg, which joins the separate video and sound of the higher qualities: the system's (on
//! PATH), or on Windows BtbN's build fetched into tools/ffmpeg once the user agreed (it's big).

use super::{checksum_for, dir, download, get, on_path, Fetch, EXE};
use crate::config::own_user_agent;
use std::path::{Path, PathBuf};

/// BtbN's nightly build with its libraries apart (~85 MB, half the size of the all-in-one
/// build), from a release whose name stays "latest".
const RELEASE: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
const ASSET: &str = "ffmpeg-master-latest-win64-gpl-shared.zip";

/// TurboDM can fetch FFmpeg itself (Windows); elsewhere the system has it, or not.
pub const CAN_FETCH_FFMPEG: bool = cfg!(windows);

/// Why the joined qualities aren't offered, where FFmpeg can't be fetched.
pub const NO_FFMPEG: &str = "Higher qualities need FFmpeg to join their video and sound: install ffmpeg \
                             (e.g. sudo apt install ffmpeg) to get them";

/// Where FFmpeg is, for yt-dlp's --ffmpeg-location: ours, else the system's (None: none).
pub fn ffmpeg() -> Option<PathBuf> {
    let own = dir().join("ffmpeg");
    if own.join(format!("ffmpeg{EXE}")).is_file() {
        return Some(own);
    }
    on_path("ffmpeg").and_then(|p| p.parent().map(Path::to_path_buf))
}

/// The size of the FFmpeg download, to ask the user first.
pub async fn ffmpeg_size(client: &reqwest::Client) -> Option<u64> {
    let resp = client.head(format!("{RELEASE}/{ASSET}")).header("User-Agent", own_user_agent())
        .send().await.ok()?.error_for_status().ok()?;
    // (the header: a HEAD answer's body, which content_length() measures, is empty)
    let length = resp.headers().get(reqwest::header::CONTENT_LENGTH)?.to_str().ok()?;
    length.parse().ok().filter(|n: &u64| *n > 0)
}

/// Fetch FFmpeg into tools/ffmpeg (Windows): the zip, checked, then its programs and libraries.
pub async fn fetch_ffmpeg(client: &reqwest::Client, progress: &Fetch) -> Result<(), String> {
    if !CAN_FETCH_FFMPEG {
        return Err(NO_FFMPEG.into());
    }
    static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = ONE.lock().await;
    if ffmpeg().is_some() {
        return Ok(()); // fetched meanwhile (another window asked for it too)
    }
    let sums = get(client, &format!("{RELEASE}/checksums.sha256")).await?.text().await.map_err(|e| e.to_string())?;
    let expected = checksum_for(&sums, ASSET).ok_or("FFmpeg's release has no checksum for it")?;
    let zip = dir().join("ffmpeg.zip.download");
    download(client, &format!("{RELEASE}/{ASSET}"), &zip, &expected, Some(progress)).await
        .map_err(|e| format!("Couldn't get FFmpeg: {e}"))?;
    let unpacked = tokio::task::spawn_blocking(move || {
        let result = unpack(&zip);
        let _ = std::fs::remove_file(&zip);
        result
    }).await.map_err(|e| e.to_string())?;
    unpacked.map_err(|e| format!("Couldn't unpack FFmpeg: {e}"))
}

/// The zip's bin folder (ffmpeg, ffprobe and their DLLs, not ffplay) into tools/ffmpeg, with
/// Windows' own tar (bsdtar reads zips; part of Windows since 10 1803).
fn unpack(zip: &Path) -> std::io::Result<()> {
    let target = dir().join("ffmpeg");
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
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&fresh, &target)
}
