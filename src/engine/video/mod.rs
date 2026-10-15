//! Videos on web pages (YouTube and the thousand-odd other sites yt-dlp knows), like IDM's
//! "Download this video": yt-dlp (https://github.com/yt-dlp/yt-dlp, Unlicense) finds what a page
//! offers and downloads the quality chosen; FFmpeg joins the separate video and sound that the
//! higher qualities come in. Both are fetched on demand (tools.rs).

mod formats;
mod process;
mod progress;
mod run;
pub mod tools;

pub use formats::{quality_of, Choice, VideoInfo};
pub use run::run;

use super::http::Headers;
use super::task::{Status, TaskInfo};
use crate::categories::is_downloadable;
use crate::util::{filename_from_url, sanitize_filename};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

/// A video download, saved with it: the quality chosen and how far it got.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoJob {
    pub format: String,  // yt-dlp's -f; empty: look at the page first, then take the usual quality
    pub quality: String, // what it's called: "720p MP4"
    pub merge: bool,     // FFmpeg joins video and sound into an MP4
    pub parts: Vec<(String, Option<u64>)>, // (format id, estimated size) downloaded in turn
    pub done: u64,       // bytes downloaded, all parts
    pub title: String,
    pub with_cookies: bool, // the page needs the browser's cookies
}

impl VideoJob {
    pub fn new(info: &VideoInfo, choice: &Choice) -> Self {
        VideoJob {
            format: choice.format.clone(),
            quality: choice.label.clone(),
            merge: choice.merge,
            parts: choice.parts.clone(),
            done: 0,
            title: info.title.clone(),
            with_cookies: info.with_cookies,
        }
    }
}

/// The file name for a video: its title (kept short: yt-dlp adds to it while downloading,
/// and Windows paths are limited), then the choice's extension.
pub fn filename(title: &str, ext: &str) -> String {
    // titles are full of what file names can't have: "Part 2: the end" → "Part 2 - the end"
    let mut plain = String::with_capacity(title.len());
    for c in title.chars() {
        match c {
            '"' => plain.push('\''),
            ':' => plain.push_str(" -"),
            '|' | '/' | '\\' => plain.push('-'),
            '?' | '*' | '<' | '>' => plain.push(' '),
            c => plain.push(c),
        }
    }
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = sanitize_filename(&plain).chars().take(120).collect();
    format!("{}.{ext}", name.trim().trim_end_matches('.'))
}

fn host(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

fn on_site(url: &str, sites: &[&str]) -> bool {
    let host = host(url);
    sites.iter().any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

pub fn is_youtube(url: &str) -> bool {
    on_site(url, &["youtube.com", "youtu.be", "youtube-nocookie.com"])
}

/// A page of a site known for its videos (looked at for a video straight away, not downloaded
/// as a file first). Any other page is looked at once its server says it's a web page.
pub fn is_video_site(url: &str) -> bool {
    const SITES: [&str; 18] = ["youtube.com", "youtu.be", "youtube-nocookie.com", "vimeo.com", "dailymotion.com",
        "dai.ly", "twitch.tv", "tiktok.com", "instagram.com", "facebook.com", "fb.watch", "x.com", "twitter.com",
        "reddit.com", "soundcloud.com", "bilibili.com", "rumble.com", "streamable.com"];
    url.starts_with("http") && on_site(url, &SITES) && !is_downloadable(&filename_from_url(url))
}

/// A web page, by its content type, rather than a file.
pub fn is_web_page(content_type: &str) -> bool {
    matches!(content_type, "text/html" | "application/xhtml+xml")
}

/// What yt-dlp is always given: no settings of the user's own yt-dlp (they'd change its
/// output), its cache with TurboDM's tools, and the browser's request.
fn common(command: &mut Command, headers: &Headers, cookies: bool, timeout: u64) {
    command.args(["--ignore-config", "--color", "never", "--no-playlist", "--socket-timeout"]).arg(timeout.to_string())
        .arg("--cache-dir").arg(tools::dir().join("cache"))
        // YouTube's 360p in one file (format 18: no FFmpeg needed) comes from its embedded
        // player, which needs a JavaScript runtime; without one it adds nothing, and every video
        // quality needs FFmpeg. yt-dlp only uses Deno unless told; Node is more common (and
        // ignored when it isn't installed)
        .args(["--extractor-args", "youtube:player_client=default,web_embedded", "--js-runtimes", "node"]);
    if let Some(ua) = &headers.user_agent {
        command.arg("--user-agent").arg(ua);
    }
    if let Some(referrer) = &headers.referrer {
        command.arg("--referer").arg(referrer);
    }
    if let (true, Some(cookies)) = (cookies, &headers.cookies) {
        // (yt-dlp keeps them to the page's site)
        command.arg("--add-headers").arg(format!("Cookie:{cookies}"));
    }
    if let Some(ffmpeg) = tools::ffmpeg() {
        command.arg("--ffmpeg-location").arg(ffmpeg);
    }
}

/// YouTube works best without the browser's cookies (with them yt-dlp must use pickier
/// players); they're kept for when it asks to sign in. Other sites get them from the start.
fn cookies_first(url: &str) -> bool {
    !is_youtube(url)
}

/// The site wants to know who's asking: the browser's cookies may get in.
fn wants_sign_in(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["sign in", "log in", "login", "cookies", "your age", "private", "members"].iter().any(|w| e.contains(w))
}

/// yt-dlp itself may be out of date (sites change; yt-dlp follows within days).
fn looks_outdated(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["unable to extract", "failed to extract", "latest version", "signature", "nsig", "challenge", "precondition",
     "http error 400", "no video formats"].iter().any(|w| e.contains(w))
}

/// yt-dlp's last "ERROR: …" line, in a few plain words.
fn error_of(stderr: &str) -> Option<String> {
    let line = stderr.lines().rev().find(|l| l.starts_with("ERROR:"))?;
    let mut text = line.trim_start_matches("ERROR:").trim();
    // "[youtube] Z4C82eyhwgU: Sign in…" → "Sign in…"
    if text.starts_with('[')
        && let Some((_, rest)) = text.split_once("] ") {
            text = rest.split_once(": ").filter(|(id, _)| !id.contains(' ')).map_or(rest, |(_, r)| r);
        }
    // the advice for yt-dlp's own users (its options, links) goes
    for cut in [". Use --", "; please report", ". Please report", " See  http", ". See http", " (caused by"] {
        if let Some(i) = text.find(cut) {
            text = &text[..i];
        }
    }
    let text = text.trim().trim_end_matches('.');
    Some(if text.starts_with("Unsupported URL") { "No video found on this page".into() } else { text.to_string() })
}

/// Look at a page: its video's title and the qualities it comes in. Fetches yt-dlp first if
/// needed, retries with the browser's cookies when the site asks to sign in, and with the
/// latest yt-dlp when it looks out of date.
pub async fn probe(client: &reqwest::Client, url: &str, headers: &Headers, timeout: u64) -> Result<VideoInfo, String> {
    let mut exe = tools::yt_dlp(client).await?;
    let mut cookies = cookies_first(url) && headers.cookies.is_some();
    let mut updated = false;
    loop {
        match probe_once(&exe, url, headers, cookies, timeout).await {
            Ok(mut info) => {
                info.with_cookies = cookies;
                return Ok(info);
            }
            Err(e) if !cookies && headers.cookies.is_some() && wants_sign_in(&e) => cookies = true,
            Err(e) if !updated && looks_outdated(&e) => {
                updated = true;
                if tools::update(client, true).await != Ok(true) {
                    return Err(e);
                }
                exe = tools::yt_dlp_now().ok_or(e)?;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn probe_once(exe: &Path, url: &str, headers: &Headers, cookies: bool, timeout: u64) -> Result<VideoInfo, String> {
    let mut command = process::command(exe);
    common(&mut command, headers, cookies, timeout);
    command.args(["-J", "--playlist-items", "1", "--"]).arg(url);
    let mut running = process::Running::spawn(command).map_err(|e| format!("yt-dlp doesn't start: {e}"))?;
    let (mut stdout, mut stderr) = (running.child.stdout.take(), running.child.stderr.take());
    let read = async {
        use tokio::io::AsyncReadExt;
        let (mut out, mut err) = (vec![], vec![]);
        let (a, b) = tokio::join!(async { if let Some(s) = &mut stdout { s.read_to_end(&mut out).await } else { Ok(0) } },
                                  async { if let Some(s) = &mut stderr { s.read_to_end(&mut err).await } else { Ok(0) } });
        a.and(b).map(|_| (out, err))
    };
    // a page that never answers: give up after a while
    let read = tokio::time::timeout(Duration::from_secs(timeout.max(30) * 4), read).await;
    let Ok(Ok((out, err))) = read else {
        running.stop().await;
        return Err("The page took too long to answer".into());
    };
    let status = running.child.wait().await.map_err(|e| e.to_string())?;
    let err = String::from_utf8_lossy(&err);
    if !status.success() || out.is_empty() {
        return Err(error_of(&err).unwrap_or_else(|| format!("yt-dlp couldn't read the page ({status})")));
    }
    formats::parse(&out, url)
}

/// What yt-dlp leaves beside a video while downloading it ("a.mp4.part", "a.f137.mp4",
/// "a.mp4.part-Frag3", "a.mp4.ytdl", "a.temp.mp4"), for a file named `stem` + extension.
fn is_leftover(name: &str, stem: &str) -> bool {
    let Some(rest) = name.strip_prefix(stem).and_then(|r| r.strip_prefix('.')) else { return false };
    let format_part = rest.split_once('.').is_some_and(|(first, ext)| {
        first.len() > 1 && first.starts_with('f') && !ext.contains('.')
            && first[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    });
    rest.contains(".part") || rest.ends_with(".ytdl") || rest.starts_with("temp.") || format_part
}

/// Delete a video download's files: what's unfinished, and the video itself once finished.
pub fn remove_files(info: &TaskInfo) {
    let Some(name) = info.filename.as_deref() else { return };
    if info.status == Status::Completed {
        let _ = std::fs::remove_file(info.path());
    }
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let Ok(entries) = std::fs::read_dir(&info.directory) else { return };
    for entry in entries.flatten() {
        if is_leftover(&entry.file_name().to_string_lossy(), stem) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_video_sites_and_pages() {
        assert!(is_video_site("https://www.youtube.com/watch?v=Z4C82eyhwgU"));
        assert!(is_video_site("https://youtu.be/Z4C82eyhwgU?t=3"));
        assert!(is_video_site("https://m.youtube.com/shorts/abc"));
        assert!(is_video_site("https://vimeo.com/76979871"));
        assert!(!is_video_site("https://notyoutube.com/watch"));
        assert!(!is_video_site("https://example.org/movie.mp4"));
        assert!(!is_video_site("https://vimeo.com/files/movie.zip")); // a file is a file
        assert!(is_youtube("https://user@www.youtube.com:443/watch?v=x") && !is_youtube("https://vimeo.com/1"));
        assert!(is_web_page("text/html") && !is_web_page("video/mp4"));
    }

    #[test]
    fn reads_yt_dlp_errors() {
        let bot = "WARNING: [youtube] No title found\nERROR: [youtube] Z4C82eyhwgU: Sign in to confirm you’re not \
                   a bot. Use --cookies-from-browser or --cookies for the authentication. See  https://github.com/x";
        let message = error_of(bot).unwrap();
        assert_eq!(message, "Sign in to confirm you’re not a bot");
        assert!(wants_sign_in(&message) && !looks_outdated(&message));
        assert_eq!(error_of("ERROR: Unsupported URL: https://example.org/").as_deref(), Some("No video found on this page"));
        let broken = "ERROR: [youtube] abc: Unable to extract yt initial data; please report this issue on  \
                      https://github.com/yt-dlp/yt-dlp/issues . Confirm you are on the latest version using yt-dlp -U";
        let message = error_of(broken).unwrap();
        assert_eq!(message, "Unable to extract yt initial data");
        assert!(looks_outdated(&message));
        assert_eq!(error_of("ERROR: unable to download video data: HTTP Error 403: Forbidden").as_deref(),
                   Some("unable to download video data: HTTP Error 403: Forbidden"));
        assert_eq!(error_of("nothing here"), None);
    }

    #[test]
    fn names_and_leftovers() {
        assert_eq!(filename("\"Caminandes 2: Gran Dillama\" | Blender?", "mp4"), "'Caminandes 2 - Gran Dillama' - Blender.mp4");
        assert_eq!(filename(&"x".repeat(300), "m4a").len(), 124);
        for left in ["a b.mp4.part", "a b.f137.mp4", "a b.f251-drc.webm.part", "a b.mp4.ytdl", "a b.mp4.part-Frag12",
                     "a b.temp.mp4"] {
            assert!(is_leftover(left, "a b"), "{left}");
        }
        for keep in ["a b.mp4", "a b.srt", "a bc.mp4.part", "a b.final.cut.mp4", "a b.f.mp4"] {
            assert!(!is_leftover(keep, "a b"), "{keep}");
        }
    }

    #[test]
    fn video_jobs_load_from_older_lists() {
        // a download saved before videos existed has no "video"; one saved with it keeps it
        let job: VideoJob = serde_json::from_str(r#"{"format": "18/b", "quality": "360p MP4"}"#).unwrap();
        assert_eq!((job.format.as_str(), job.merge, job.done), ("18/b", false, 0));
    }
}
