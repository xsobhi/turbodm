//! The programs videos need, fetched on demand into the data folder's `tools`: yt-dlp (from its
//! latest GitHub release, checked against that release's SHA2-256SUMS) and, on Windows, FFmpeg
//! (BtbN's build, checked against its checksums.sha256). Linux uses the system's when it has
//! them (`yt-dlp`, `ffmpeg` on PATH).

use crate::config::{data_dir, own_user_agent};
use crate::engine::task::unix_now;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const YT_DLP_RELEASES: &str = "https://github.com/yt-dlp/yt-dlp/releases";

/// yt-dlp's build for this system (one program, Python included).
#[cfg(all(windows, target_arch = "aarch64"))]
const YT_DLP_ASSET: &str = "yt-dlp_arm64.exe";
#[cfg(all(windows, not(target_arch = "aarch64")))]
const YT_DLP_ASSET: &str = "yt-dlp.exe";
#[cfg(all(not(windows), target_arch = "aarch64"))]
const YT_DLP_ASSET: &str = "yt-dlp_linux_aarch64";
#[cfg(all(not(windows), not(target_arch = "aarch64")))]
const YT_DLP_ASSET: &str = "yt-dlp_linux";

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

/// FFmpeg for Windows: BtbN's nightly build with its libraries apart (~85 MB, half the size of
/// the all-in-one build), from a release whose name stays "latest".
const FFMPEG_RELEASE: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
const FFMPEG_ASSET: &str = "ffmpeg-master-latest-win64-gpl-shared.zip";

/// How often our yt-dlp is checked for a newer version (sites change, old versions break).
const UPDATE_EVERY: u64 = 3 * 24 * 3600;
/// …and at most this often when it failed on a page (it may be broken, or the page may be).
const RETRY_UPDATE_AFTER: u64 = 3600;

pub fn dir() -> PathBuf {
    data_dir().join("tools")
}

fn own_yt_dlp() -> PathBuf {
    dir().join(format!("yt-dlp{EXE}"))
}

/// A program on PATH.
fn on_path(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{EXE}");
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(&file)).find(|p| p.is_file())
}

/// The yt-dlp to run, if there is one: ours (fetched before), else Linux's own.
pub fn yt_dlp_now() -> Option<PathBuf> {
    let own = own_yt_dlp();
    if own.is_file() {
        return Some(own);
    }
    if cfg!(windows) { None } else { on_path("yt-dlp") }
}

/// yt-dlp, fetched now if there's none (once at a time, however many ask).
pub async fn yt_dlp(client: &reqwest::Client) -> Result<PathBuf, String> {
    if let Some(found) = yt_dlp_now() {
        if found == own_yt_dlp() && due(UPDATE_EVERY) {
            let client = client.clone();
            tokio::spawn(async move { let _ = update(&client, false).await; }); // in the background
        }
        return Ok(found);
    }
    let _one = FETCHING.lock().await;
    if let Some(found) = yt_dlp_now() {
        return Ok(found); // fetched while we waited
    }
    fetch_yt_dlp(client).await?;
    Ok(own_yt_dlp())
}

static FETCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn checked_file() -> PathBuf {
    dir().join("yt-dlp.checked")
}

fn version_file() -> PathBuf {
    dir().join("yt-dlp.version")
}

/// Longer than `secs` since our yt-dlp was last checked for an update.
fn due(secs: u64) -> bool {
    let last = std::fs::read_to_string(checked_file()).ok().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or(0);
    unix_now().saturating_sub(last) >= secs
}

/// Get the latest yt-dlp if ours is older (or we use the system's, which just failed: `failed`).
/// True when a newer one was installed.
pub async fn update(client: &reqwest::Client, failed: bool) -> Result<bool, String> {
    let ours = own_yt_dlp().is_file();
    if ours && !due(if failed { RETRY_UPDATE_AFTER } else { UPDATE_EVERY }) {
        return Ok(false);
    }
    if !ours && !failed {
        return Ok(false); // the system's: updated with the system
    }
    let _one = FETCHING.lock().await;
    let (tag, _) = latest_sums(client).await?;
    let _ = std::fs::write(checked_file(), unix_now().to_string());
    let installed = std::fs::read_to_string(version_file()).unwrap_or_default();
    if ours && installed.trim() == tag {
        return Ok(false);
    }
    fetch_yt_dlp(client).await.map(|_| true)
}

/// The latest release's name ("2026.08.19") and its SHA2-256SUMS.
async fn latest_sums(client: &reqwest::Client) -> Result<(String, String), String> {
    // GitHub sends …/releases/latest on to …/releases/tag/<tag>: that first answer is enough
    let lost = || "yt-dlp's latest release can't be found".to_string();
    let first = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().map_err(|e| e.to_string())?;
    let resp = first.head(format!("{YT_DLP_RELEASES}/latest")).header("User-Agent", own_user_agent()).send().await
        .map_err(|e| e.to_string())?;
    let location = resp.headers().get(reqwest::header::LOCATION).and_then(|l| l.to_str().ok()).ok_or_else(lost)?;
    let tag = location.split("/releases/tag/").nth(1).map(|t| t.trim_end_matches('/')).filter(|t| !t.is_empty())
        .ok_or_else(lost)?.to_string();
    let sums = get(client, &format!("{YT_DLP_RELEASES}/download/{tag}/SHA2-256SUMS")).await?
        .text().await.map_err(|e| e.to_string())?;
    Ok((tag, sums))
}

/// The SHA-256 a checksums file ("<hex>  <name>" lines) gives for `name`.
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

async fn fetch_yt_dlp(client: &reqwest::Client) -> Result<(), String> {
    let (tag, sums) = latest_sums(client).await?;
    let expected = checksum_for(&sums, YT_DLP_ASSET).ok_or("yt-dlp's release has no checksum for this system")?;
    let url = format!("{YT_DLP_RELEASES}/download/{tag}/{YT_DLP_ASSET}");
    let fresh = dir().join(format!("yt-dlp{EXE}.download"));
    download(client, &url, &fresh, &expected, None).await.map_err(|e| format!("Couldn't get yt-dlp: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    replace(&fresh, &own_yt_dlp()).map_err(|e| format!("Couldn't install yt-dlp: {e}"))?;
    let _ = std::fs::write(version_file(), &tag);
    let _ = std::fs::write(checked_file(), unix_now().to_string());
    Ok(())
}

/// Put `fresh` where `target` is. A running program can't be overwritten on Windows, but it
/// can be renamed out of the way.
fn replace(fresh: &Path, target: &Path) -> std::io::Result<()> {
    let old = target.with_extension("old");
    let _ = std::fs::remove_file(&old); // from last time (no longer running)
    if target.exists() {
        std::fs::rename(target, &old)?;
    }
    std::fs::rename(fresh, target)
}

async fn get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, String> {
    client.get(url).header("User-Agent", own_user_agent()).send().await
        .and_then(|r| r.error_for_status()).map_err(|e| e.to_string())
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

/// Where FFmpeg is, for yt-dlp's --ffmpeg-location: ours, else the system's (None: none).
pub fn ffmpeg() -> Option<PathBuf> {
    let own = dir().join("ffmpeg");
    if own.join(format!("ffmpeg{EXE}")).is_file() {
        return Some(own);
    }
    on_path("ffmpeg").and_then(|p| p.parent().map(Path::to_path_buf))
}

/// TurboDM can fetch FFmpeg itself (Windows); elsewhere the system has it, or not.
pub const CAN_FETCH_FFMPEG: bool = cfg!(windows);

/// Why the joined qualities aren't offered, where FFmpeg can't be fetched.
pub const NO_FFMPEG: &str = "Higher qualities need FFmpeg to join their video and sound: install ffmpeg \
                             (e.g. sudo apt install ffmpeg) to get them";

/// The size of the FFmpeg download, to ask the user first.
pub async fn ffmpeg_size(client: &reqwest::Client) -> Option<u64> {
    let resp = client.head(format!("{FFMPEG_RELEASE}/{FFMPEG_ASSET}")).header("User-Agent", own_user_agent())
        .send().await.ok()?.error_for_status().ok()?;
    resp.content_length().filter(|n| *n > 0)
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
    let sums = get(client, &format!("{FFMPEG_RELEASE}/checksums.sha256")).await?.text().await.map_err(|e| e.to_string())?;
    let expected = checksum_for(&sums, FFMPEG_ASSET).ok_or("FFmpeg's release has no checksum for it")?;
    let zip = dir().join("ffmpeg.zip.download");
    download(client, &format!("{FFMPEG_RELEASE}/{FFMPEG_ASSET}"), &zip, &expected, Some(progress)).await
        .map_err(|e| format!("Couldn't get FFmpeg: {e}"))?;
    let unpacked = tokio::task::spawn_blocking(move || {
        let result = unpack_ffmpeg(&zip);
        let _ = std::fs::remove_file(&zip);
        result
    }).await.map_err(|e| e.to_string())?;
    unpacked.map_err(|e| format!("Couldn't unpack FFmpeg: {e}"))
}

/// The zip's bin folder (ffmpeg, ffprobe and their DLLs, not ffplay) into tools/ffmpeg, with
/// Windows' own tar (bsdtar reads zips; part of Windows since 10 1803).
fn unpack_ffmpeg(zip: &Path) -> std::io::Result<()> {
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
    std::os::windows::process::CommandExt::creation_flags(&mut command, super::process::NO_WINDOW);
    let status = command.status()?;
    if !status.success() || !fresh.join(format!("ffmpeg{EXE}")).is_file() {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(std::io::Error::other("ffmpeg.exe isn't in the download"));
    }
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&fresh, &target)
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
