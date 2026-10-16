//! Videos on web pages (YouTube and the thousand-odd other sites yt-dlp knows), like IDM's
//! "Download this video". yt-dlp (https://github.com/yt-dlp/yt-dlp, Unlicense) finds what a page
//! offers and where its streams are (probe.rs); TurboDM downloads them itself, with all its
//! connections (direct.rs), and FFmpeg makes the file asked for (make.rs): video and sound joined,
//! or the sound as MP3. Streams in fragments (HLS…) are left to yt-dlp (download.rs).

mod args;
mod direct;
mod formats;
mod fresh;
mod info;
mod make;
mod menu;
mod names;
mod place;
mod plan;
mod probe;
mod process;
mod progress;
mod run;
mod said;
mod sites;
#[cfg(test)]
mod tests;
pub mod tools;
mod ytdlp;

pub use info::{remembered, Container, Plan, Sound, VideoInfo, Want};
pub use menu::{choice_id, qualities, want_of, wanted, NEEDS_HELPERS};
pub use names::{filename, remove_files};
pub use place::{move_running, move_stopped};
pub use probe::{forget, look_ahead, probe};
pub use run::run;
pub use sites::{is_video_site, is_web_page, is_youtube};

use super::segments::Segment;
use super::task::TaskInfo;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// What's made of a video's parts once they're in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Output {
    #[default]
    Single, // one part, as it comes
    Mp4,    // video and sound joined (copied as they are, not re-encoded)
    WebM,
    Mkv,
    Mp3, // the sound, converted
}

impl Output {
    pub fn needs_ffmpeg(self) -> bool {
        self != Output::Single
    }

    /// The made file's extension (None: the part's own).
    pub fn ext(self) -> Option<&'static str> {
        match self {
            Output::Single => None,
            Output::Mp4 => Some("mp4"),
            Output::WebM => Some("webm"),
            Output::Mkv => Some("mkv"),
            Output::Mp3 => Some("mp3"),
        }
    }
}

/// One stream of a video (its video, its sound, or both in one), as yt-dlp found it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Source {
    pub format_id: String,
    pub url: String,                    // direct; expires after hours (asked for again then)
    pub headers: Vec<(String, String)>, // what the site wants with it (yt-dlp's, its cookies)
    pub protocol: String,               // "https": TurboDM downloads it; else (HLS…) yt-dlp does
    pub ext: String,
    pub size: Option<u64>,
    pub chunk: u64, // the most to ask for at once (YouTube: 10 MB, or it slows down); 0: any
    pub done: bool,
}

impl Source {
    /// A plain file TurboDM's connections can fetch (not a playlist of fragments).
    pub fn plain(&self) -> bool {
        matches!(self.protocol.as_str(), "https" | "http") && self.url.starts_with("http")
    }
}

/// A video download, saved with it: the quality chosen, its parts and how far it got.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoJob {
    pub format: String,  // yt-dlp's -f; empty: look at the page first, then take the usual quality
    pub quality: String, // what it's called: "720p MP4", "MP3"
    pub merge: bool,     // video and sound joined (what 1.8 saved instead of `output`)
    pub parts: Vec<(String, Option<u64>)>, // (format id, estimated size), for yt-dlp's own download
    pub done: u64,       // bytes of the parts finished (yt-dlp's own download: all it got)
    pub title: String,
    pub with_cookies: bool, // the page needs the browser's cookies
    pub output: Output,
    pub sources: Vec<Source>, // its parts, downloaded by TurboDM in turn
    pub by_ytdlp: bool,       // yt-dlp downloads it itself (fragments, or TurboDM couldn't)
    pub choice: String, // with `format` empty: the quality picked in the browser ("1080p/mp4")
}

impl VideoJob {
    pub fn new(info: &VideoInfo, plan: &Plan) -> Self {
        VideoJob {
            format: plan.format.clone(),
            quality: plan.label.clone(),
            merge: matches!(plan.output, Output::Mp4 | Output::WebM | Output::Mkv),
            parts: plan.sources.iter().map(|s| (s.format_id.clone(), s.size)).collect(),
            done: 0,
            title: info.title.clone(),
            with_cookies: info.with_cookies,
            output: plan.output,
            sources: plan.sources.clone(),
            by_ytdlp: false,
            choice: menu::choice_id(plan.want),
        }
    }

    /// What's made of it (a job saved by 1.8 says it with `merge`: an MP4).
    pub fn output(&self) -> Output {
        if self.output == Output::Single && self.merge { Output::Mp4 } else { self.output }
    }

    /// Make a download being added this video: resumable, its size its parts'.
    pub(crate) fn attach(self, info: &mut TaskInfo) {
        info.resumable = true;
        info.size = self.parts.iter().map(|(_, size)| *size).sum::<Option<u64>>().filter(|_| !self.parts.is_empty());
        info.video = Some(self);
    }
}

/// The parts as the progress window shows connections: those done as one finished stretch each,
/// then the connections of the one under way, all one after the other.
pub fn shown_segments(job: &VideoJob, current: Vec<Segment>) -> Vec<Segment> {
    let mut start = 0;
    let mut shown = vec![];
    for source in &job.sources {
        let size = source.size.unwrap_or(0);
        if source.done {
            shown.push(Segment { start, end: Some(start + size), done: size, ahead: 0, active: false, heard: None });
        } else {
            shown.extend(current.into_iter().map(|s| Segment { start: s.start + start, end: s.end.map(|e| e + start), ..s }));
            break;
        }
        start += size;
    }
    shown
}

/// What a running video download shows besides its bytes (not saved): what it's doing, and,
/// while yt-dlp downloads it, its parts (its video, its sound), shown as its connections.
#[derive(Debug, Default)]
pub struct Live {
    activity: Mutex<Option<&'static str>>,
    parts: Mutex<Vec<Segment>>,
    making: AtomicBool, // its file is being made of its parts: they stay where they are
}

impl Live {
    pub fn activity(&self) -> Option<&'static str> {
        *self.activity.lock().unwrap()
    }

    pub fn parts(&self) -> Vec<Segment> {
        self.parts.lock().unwrap().clone()
    }

    /// Its file is being made (or was just made and isn't said to be done yet).
    pub fn making(&self) -> bool {
        self.making.load(Ordering::SeqCst)
    }

    fn set_making(&self) {
        self.making.store(true, Ordering::SeqCst);
    }

    fn set_activity(&self, what: Option<&'static str>) {
        *self.activity.lock().unwrap() = what;
    }

    fn set_parts(&self, parts: Vec<Segment>) {
        *self.parts.lock().unwrap() = parts;
    }

    /// Nothing is running: nothing is under way.
    fn stopped(&self) {
        self.set_activity(None);
        self.making.store(false, Ordering::SeqCst);
        self.parts.lock().unwrap().iter_mut().for_each(|p| p.active = false);
    }
}
