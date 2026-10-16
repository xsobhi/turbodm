//! yt-dlp: TurboDM's copy (the latest GitHub release's build for this system, checked against
//! that release's SHA2-256SUMS, updated every few days or when a page fails in a way that looks
//! like an outdated yt-dlp), else Linux's own.

use super::setup::{size_of, Helper};
use super::{checksum_for, dir, download, get, on_path, EXE};
use crate::config::own_user_agent;
use crate::engine::task::unix_now;
use std::path::{Path, PathBuf};

const RELEASES: &str = "https://github.com/yt-dlp/yt-dlp/releases";

/// yt-dlp's build for this system (one program, Python included).
#[cfg(all(windows, target_arch = "aarch64"))]
const ASSET: &str = "yt-dlp_arm64.exe";
#[cfg(all(windows, not(target_arch = "aarch64")))]
const ASSET: &str = "yt-dlp.exe";
#[cfg(all(not(windows), target_arch = "aarch64"))]
const ASSET: &str = "yt-dlp_linux_aarch64";
#[cfg(all(not(windows), not(target_arch = "aarch64")))]
const ASSET: &str = "yt-dlp_linux";

/// How often our yt-dlp is checked for a newer version (sites change, old versions break).
const UPDATE_EVERY: u64 = 3 * 24 * 3600;
/// …and at most this often when it failed on a page (it may be broken, or the page may be).
const RETRY_UPDATE_AFTER: u64 = 3600;

/// One fetch or update at a time, however many ask.
static FETCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn own() -> PathBuf {
    dir().join(format!("yt-dlp{EXE}"))
}

fn checked_file() -> PathBuf {
    dir().join("yt-dlp.checked")
}

fn version_file() -> PathBuf {
    dir().join("yt-dlp.version")
}

/// The yt-dlp to run, if there is one: ours (fetched before), else Linux's own.
pub fn yt_dlp_now() -> Option<PathBuf> {
    let own = own();
    if own.is_file() {
        return Some(own);
    }
    if cfg!(windows) { None } else { on_path("yt-dlp") }
}

/// yt-dlp, fetched now if there's none.
pub async fn yt_dlp(client: &reqwest::Client) -> Result<PathBuf, String> {
    if let Some(found) = yt_dlp_now() {
        if found == own() && due(UPDATE_EVERY) {
            let client = client.clone();
            tokio::spawn(async move { let _ = update(&client, false).await; }); // in the background
        }
        return Ok(found);
    }
    let _one = FETCHING.lock().await;
    if let Some(found) = yt_dlp_now() {
        return Ok(found); // fetched while we waited
    }
    fetch(client).await?;
    Ok(own())
}

/// Longer than `secs` since our yt-dlp was last checked for an update.
fn due(secs: u64) -> bool {
    let last = std::fs::read_to_string(checked_file()).ok().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or(0);
    unix_now().saturating_sub(last) >= secs
}

/// Get the latest yt-dlp if ours is older (or we use the system's, which just failed: `failed`).
/// True when a newer one was installed.
pub async fn update(client: &reqwest::Client, failed: bool) -> Result<bool, String> {
    let ours = own().is_file();
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
    fetch(client).await.map(|_| true)
}

/// The latest release's name ("2026.08.19") and its SHA2-256SUMS.
async fn latest_sums(client: &reqwest::Client) -> Result<(String, String), String> {
    // GitHub sends …/releases/latest on to …/releases/tag/<tag>: that first answer is enough
    let lost = || "yt-dlp's latest release can't be found".to_string();
    let first = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().map_err(|e| e.to_string())?;
    let resp = first.head(format!("{RELEASES}/latest")).header("User-Agent", own_user_agent()).send().await
        .map_err(|e| e.to_string())?;
    let location = resp.headers().get(reqwest::header::LOCATION).and_then(|l| l.to_str().ok()).ok_or_else(lost)?;
    let tag = location.split("/releases/tag/").nth(1).map(|t| t.trim_end_matches('/')).filter(|t| !t.is_empty())
        .ok_or_else(lost)?.to_string();
    let sums = get(client, &format!("{RELEASES}/download/{tag}/SHA2-256SUMS")).await?
        .text().await.map_err(|e| e.to_string())?;
    Ok((tag, sums))
}

/// Where yt-dlp comes from, for the setup window's download.
pub(super) async fn helper(client: &reqwest::Client) -> Result<Helper, String> {
    let (tag, sums) = latest_sums(client).await?;
    let sha256 = checksum_for(&sums, ASSET).ok_or("yt-dlp's release has no checksum for this system")?;
    let url = format!("{RELEASES}/download/{tag}/{ASSET}");
    Ok(Helper { name: "yt-dlp", size: size_of(client, &url).await, url, file: ASSET.into(), sha256, version: tag })
}

/// A checked download of release `tag` into place.
pub(super) fn install(fresh: &Path, tag: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(fresh, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    replace(fresh, &own()).map_err(|e| format!("Couldn't install yt-dlp: {e}"))?;
    let _ = std::fs::write(version_file(), tag);
    let _ = std::fs::write(checked_file(), unix_now().to_string());
    Ok(())
}

async fn fetch(client: &reqwest::Client) -> Result<(), String> {
    let (tag, sums) = latest_sums(client).await?;
    let expected = checksum_for(&sums, ASSET).ok_or("yt-dlp's release has no checksum for this system")?;
    let url = format!("{RELEASES}/download/{tag}/{ASSET}");
    let fresh = dir().join(format!("yt-dlp{EXE}.download"));
    download(client, &url, &fresh, &expected, None).await.map_err(|e| format!("Couldn't get yt-dlp: {e}"))?;
    install(&fresh, &tag)
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
