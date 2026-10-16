//! Setting the helpers up (Windows): which are missing, where each comes from (address, checksum,
//! size) so TurboDM can download them like any file, in its progress windows, and putting a
//! downloaded one in place once its checksum is right.

use super::{dir, ffmpeg, ytdlp};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// A helper to download.
#[derive(Clone, Debug)]
pub struct Helper {
    pub name: &'static str, // "yt-dlp", "FFmpeg"
    pub url: String,
    pub file: String, // its download's name
    pub sha256: String,
    pub size: Option<u64>,
    pub(super) version: String, // (yt-dlp's release, remembered once it's in)
}

/// The helpers videos need that aren't here: none on Linux, where the system's are used (and
/// yt-dlp fetched quietly if it has none).
pub fn missing() -> Vec<&'static str> {
    if !super::CAN_FETCH_FFMPEG {
        return vec![];
    }
    let yt_dlp = super::yt_dlp_now().is_none().then_some("yt-dlp");
    let ffmpeg = super::ffmpeg().is_none().then_some("FFmpeg");
    yt_dlp.into_iter().chain(ffmpeg).collect()
}

/// Where each missing helper comes from.
pub async fn helpers(client: &reqwest::Client) -> Result<Vec<Helper>, String> {
    let mut found = vec![];
    for name in missing() {
        found.push(if name == "yt-dlp" { ytdlp::helper(client).await? } else { ffmpeg::helper(client).await? });
    }
    Ok(found)
}

/// Where their downloads go.
pub fn incoming() -> PathBuf {
    dir().join("incoming")
}

/// Its size, from the server (a HEAD answer: its Content-Length header, not its empty body).
pub(super) async fn size_of(client: &reqwest::Client, url: &str) -> Option<u64> {
    let resp = client.head(url).header("User-Agent", crate::config::own_user_agent()).send().await.ok()?
        .error_for_status().ok()?;
    let length = resp.headers().get(reqwest::header::CONTENT_LENGTH)?.to_str().ok()?;
    length.parse().ok().filter(|n: &u64| *n > 0)
}

/// The SHA-256 of a file, in hex.
pub(super) fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let (mut hash, mut buf) = (Sha256::new(), vec![0u8; 1 << 20]);
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Put a downloaded helper in place, if it's what its release says (slow: call it off the UI
/// thread). The download is deleted either way.
pub fn install(helper: &Helper, downloaded: &Path) -> Result<(), String> {
    let checked = sha256_of(downloaded).map_err(|e| format!("Couldn't read {}: {e}", helper.name));
    let result = match checked {
        Ok(got) if got == helper.sha256 => match helper.name {
            "yt-dlp" => ytdlp::install(downloaded, &helper.version),
            _ => ffmpeg::install(downloaded, &helper.sha256),
        },
        Ok(_) => Err(format!("{} didn't arrive intact (wrong checksum): try again", helper.name)),
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_file(downloaded);
    result
}
