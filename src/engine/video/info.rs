//! A video found on a page and the ways it can be downloaded, and which of them to offer first.

use crate::util::human_size;

/// A video found on a page, and the ways it can be downloaded.
#[derive(Clone, Debug, Default)]
pub struct VideoInfo {
    pub url: String, // the video's own page: what yt-dlp is given when downloading
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>, // seconds
    pub site: String,          // "YouTube", "Vimeo"…; empty for a page's own <video>
    pub choices: Vec<Choice>,  // best first, sound only last
    pub with_cookies: bool,    // the browser's cookies were needed to see it
}

/// One way to download it: "720p MP4", "Audio only (M4A)"…
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub label: String,
    pub format: String,   // yt-dlp's -f: the formats picked, then fallbacks
    pub ext: String,      // the file's extension
    pub size: Option<u64>, // estimated
    pub merge: bool,      // video and sound come apart: FFmpeg joins them
    pub height: Option<u32>, // None: sound only (or a size the site doesn't say)
    pub parts: Vec<(String, Option<u64>)>, // (format id, size) yt-dlp downloads, in order
}

impl Choice {
    pub fn audio_only(&self) -> bool {
        self.height.is_none() && self.label.starts_with("Audio")
    }

    /// For a drop-down: "720p MP4  ·  26.4 MB".
    pub fn summary(&self) -> String {
        match self.size {
            Some(size) => format!("{}  ·  {}", self.label, human_size(Some(size))),
            None => self.label.clone(),
        }
    }
}

impl VideoInfo {
    /// The choice to offer first: the best up to 1080p; without FFmpeg, the best one that
    /// doesn't need it (when there is one), so nothing big is fetched unasked.
    pub fn default_choice(&self, ffmpeg: bool) -> usize {
        let videos = || self.choices.iter().enumerate().filter(|(_, c)| !c.audio_only());
        let fits = |c: &Choice| c.height.is_none_or(|h| h <= 1080);
        let pick = if ffmpeg {
            videos().find(|(_, c)| fits(c))
        } else {
            videos().find(|(_, c)| !c.merge).or_else(|| videos().find(|(_, c)| fits(c)))
        };
        pick.or_else(|| videos().next_back()).map_or(0, |(i, _)| i)
    }

    /// The choice nearest to the quality picked last time ("720p", "Audio").
    pub fn choice_like(&self, quality: &str) -> Option<usize> {
        if quality.is_empty() {
            return None;
        }
        if quality == "Audio" {
            return self.choices.iter().position(|c| c.audio_only());
        }
        let wanted: u32 = quality.trim_end_matches('p').parse().ok()?;
        // that height, else the best below it
        self.choices.iter().position(|c| c.height.is_some_and(|h| h <= wanted))
    }
}

/// What to remember of a choice for next time: "720p", "Audio".
pub fn quality_of(choice: &Choice) -> String {
    match choice.height {
        Some(h) => format!("{h}p"),
        None if choice.audio_only() => "Audio".into(),
        None => String::new(),
    }
}
