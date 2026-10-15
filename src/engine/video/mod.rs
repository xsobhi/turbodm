//! Videos on web pages (YouTube and the thousand-odd other sites yt-dlp knows), like IDM's
//! "Download this video": yt-dlp (https://github.com/yt-dlp/yt-dlp, Unlicense) finds what a page
//! offers (probe.rs) and downloads the quality chosen (run.rs); FFmpeg joins the separate video
//! and sound the higher qualities come in. Both are fetched on demand (tools/).

mod choices;
mod download;
mod formats;
mod info;
mod probe;
mod process;
mod progress;
mod run;
#[cfg(test)]
mod tests;
pub mod tools;

pub use info::{quality_of, Choice, VideoInfo};
pub use probe::probe;
pub use run::run;

use super::segments::Segment;
use super::task::{Status, TaskInfo};
use crate::categories::is_downloadable;
use crate::util::{filename_from_url, sanitize_filename};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

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

    /// Make a download being added this video: one yt-dlp, which carries on from its part files.
    pub(crate) fn attach(self, info: &mut TaskInfo) {
        (info.connections, info.resumable) = (1, true);
        info.size = self.parts.iter().map(|(_, size)| *size).sum::<Option<u64>>().filter(|_| !self.parts.is_empty());
        info.video = Some(self);
    }
}

/// What a running video download shows besides its bytes (not saved): what it's doing, and its
/// parts (its video, its sound), shown as its connections.
#[derive(Debug, Default)]
pub struct Live {
    activity: Mutex<Option<&'static str>>,
    parts: Mutex<Vec<Segment>>,
}

impl Live {
    pub fn activity(&self) -> Option<&'static str> {
        *self.activity.lock().unwrap()
    }

    pub fn parts(&self) -> Vec<Segment> {
        self.parts.lock().unwrap().clone()
    }

    fn set_activity(&self, what: Option<&'static str>) {
        *self.activity.lock().unwrap() = what;
    }

    fn set_parts(&self, parts: Vec<Segment>) {
        *self.parts.lock().unwrap() = parts;
    }

    /// yt-dlp isn't running: nothing is under way.
    fn stopped(&self) {
        self.set_activity(None);
        self.parts.lock().unwrap().iter_mut().for_each(|p| p.active = false);
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

fn on_site(url: &str, sites: &[&str]) -> bool {
    let rest = url.split("://").nth(1).unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
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
