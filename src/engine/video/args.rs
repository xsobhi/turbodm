//! What yt-dlp is always given: none of the user's own yt-dlp settings (they'd change its
//! output), its cache with TurboDM's tools, and the browser's request, with the browser's cookies
//! in a cookies file for the page's whole site (Instagram's and Facebook's pages call their other
//! hosts with them), deleted once yt-dlp is done.

use super::sites::cookie_domain;
use super::tools;
use crate::engine::http::Headers;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::process::Command;

/// A cookies file for one run of yt-dlp; deleted when dropped.
pub(super) struct CookieFile(PathBuf);

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The browser's cookies ("a=1; b=2") as a Netscape cookies file in `dir` for `url`'s site.
fn cookie_file(dir: &Path, url: &str, cookies: &str) -> Option<CookieFile> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let domain = cookie_domain(url);
    let mut text = String::from("# Netscape HTTP Cookie File\n");
    for (name, value) in cookies.split(';').filter_map(|pair| pair.trim().split_once('=')) {
        // (no expiry the file could fail on: far in the future; yt-dlp only reads it this once)
        text.push_str(&format!("{domain}\tTRUE\t/\tTRUE\t2147483647\t{}\t{}\n", name.trim(), value.trim()));
    }
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(format!("cookies-{}-{}.txt", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&path, text).ok()?;
    Some(CookieFile(path))
}

/// Give `command` what yt-dlp is always given, for the page at `url`; keep what's returned
/// until yt-dlp is done (the cookies file).
pub(super) fn common(command: &mut Command, url: &str, headers: &Headers, cookies: bool, timeout: u64) -> Option<CookieFile> {
    command.args(["--ignore-config", "--color", "never", "--no-playlist", "--socket-timeout"]).arg(timeout.to_string())
        .arg("--cache-dir").arg(tools::dir().join("cache"))
        // YouTube's 360p in one file (format 18: no FFmpeg needed) comes from its embedded
        // player, which needs a JavaScript runtime; without one it adds nothing. yt-dlp only uses
        // Deno unless told; Node is more common (and ignored when it isn't installed)
        .args(["--extractor-args", "youtube:player_client=default,web_embedded", "--js-runtimes", "node"]);
    if let Some(ua) = &headers.user_agent {
        command.arg("--user-agent").arg(ua);
    }
    if let Some(referrer) = &headers.referrer {
        command.arg("--referer").arg(referrer);
    }
    if let Some(ffmpeg) = tools::ffmpeg() {
        command.arg("--ffmpeg-location").arg(ffmpeg);
    }
    let file = headers.cookies.as_deref().filter(|_| cookies).and_then(|c| cookie_file(&tools::dir().join("cache"), url, c))?;
    command.arg("--cookies").arg(&file.0);
    Some(file)
}

#[cfg(test)]
mod tests {
    #[test]
    fn cookies_go_to_the_whole_site() {
        let dir = std::env::temp_dir().join(format!("turbodm-cookies-{}", std::process::id()));
        let file = super::cookie_file(&dir, "https://www.instagram.com/reel/abc/", "sessionid=1; csrftoken=x=y").unwrap();
        let text = std::fs::read_to_string(&file.0).unwrap();
        assert!(text.contains(".instagram.com\tTRUE\t/\tTRUE\t2147483647\tsessionid\t1\n"));
        assert!(text.contains("\tcsrftoken\tx=y\n"));
        let path = file.0.clone();
        drop(file);
        assert!(!path.exists());
        let _ = std::fs::remove_dir(&dir);
    }
}
