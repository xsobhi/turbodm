//! A video found on a page, the ways it can be downloaded (a height in a container, or the sound
//! alone as MP3, M4A or Opus), and which to offer first.

use super::formats::Format;
use super::{plan, Output, Source};
use crate::util::human_size;

/// A video found on a page.
#[derive(Clone, Debug, Default)]
pub struct VideoInfo {
    pub url: String, // the video's own page: what yt-dlp is given
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>, // seconds
    pub site: String,          // "YouTube", "Vimeo"…; empty for a page's own <video>
    pub with_cookies: bool,    // the browser's cookies were needed to see it
    pub(super) formats: Vec<Format>,
}

/// The file a video goes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Mp4,  // plays everywhere (H.264 and AAC where the site has them)
    WebM, // VP9/AV1 and Opus, often smaller
    Mkv,  // the best video and sound, whatever they are
}

/// The sound alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Mp3,  // converted by FFmpeg (VBR, ~190 kbit/s)
    M4a,  // AAC as the site sends it
    Opus, // Opus as the site sends it (in WebM)
}

impl Container {
    pub const ALL: [Container; 3] = [Container::Mp4, Container::WebM, Container::Mkv];

    pub fn name(self) -> &'static str {
        match self {
            Container::Mp4 => "MP4",
            Container::WebM => "WebM",
            Container::Mkv => "MKV",
        }
    }

    pub(super) fn output(self) -> Output {
        match self {
            Container::Mp4 => Output::Mp4,
            Container::WebM => Output::WebM,
            Container::Mkv => Output::Mkv,
        }
    }
}

impl Sound {
    pub const ALL: [Sound; 3] = [Sound::Mp3, Sound::M4a, Sound::Opus];

    pub fn name(self) -> &'static str {
        match self {
            Sound::Mp3 => "MP3",
            Sound::M4a => "M4A",
            Sound::Opus => "Opus",
        }
    }
}

/// What the user wants of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Video(Option<u32>, Container), // a height (None: the site doesn't say)
    Sound(Sound),
}

/// One way to download it, worked out: which streams, and what's made of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub want: Want,
    pub label: String, // "1080p MP4", "720p60 WebM", "MP3"
    pub sources: Vec<Source>,
    pub output: Output,
    pub ext: String,       // the file's
    pub size: Option<u64>, // estimated
    pub format: String,    // yt-dlp's -f for the same: picked formats, then fallbacks
}

impl Plan {
    pub fn needs_ffmpeg(&self) -> bool {
        self.output.needs_ffmpeg()
    }

    /// For a list: "720p MP4  ·  26.4 MB".
    pub fn summary(&self) -> String {
        match self.size {
            Some(size) => format!("{}  ·  {}", self.label, human_size(Some(size))),
            None => self.label.clone(),
        }
    }
}

impl VideoInfo {
    /// The containers worth offering: MP4 always; WebM and MKV when the site sends video and
    /// sound apart (they're joined into either) or has WebM files.
    pub fn containers(&self) -> Vec<Container> {
        Container::ALL.into_iter().filter(|c| *c == Container::Mp4 || plan::heights(&self.formats).into_iter()
            .any(|h| self.plan(Want::Video(h, *c)).is_some())).collect()
    }

    /// What to offer with videos in `container`: each height, best first, then the sound alone.
    pub fn plans(&self, container: Container) -> Vec<Plan> {
        let videos = plan::heights(&self.formats).into_iter().filter_map(|h| self.plan(Want::Video(h, container)));
        let sounds = Sound::ALL.into_iter().filter_map(|s| self.plan(Want::Sound(s)));
        let mut all: Vec<Plan> = videos.chain(sounds).collect();
        if all.is_empty() {
            all.push(plan::anything()); // nothing described well enough: let yt-dlp pick
        }
        all
    }

    pub fn plan(&self, want: Want) -> Option<Plan> {
        match want {
            Want::Video(height, container) => plan::video(&self.formats, self.duration, height, container),
            Want::Sound(sound) => plan::sound(&self.formats, self.duration, sound),
        }
    }

    /// What to offer first: what was picked last time (`quality` "720p" or "MP3", `container`
    /// "WebM"), else the best up to 1080p as MP4; without FFmpeg, the best that needs none.
    pub fn first(&self, quality: &str, container: &str, ffmpeg: bool) -> Plan {
        let container = Container::ALL.into_iter().find(|c| c.name() == container).unwrap_or(Container::Mp4);
        let plans = self.plans(container);
        let sound = Sound::ALL.into_iter().find(|s| s.name() == quality || (quality == "Audio" && *s == Sound::M4a));
        let height: Option<u32> = quality.strip_suffix('p').and_then(|h| h.parse().ok());
        let videos = || plans.iter().filter(|p| matches!(p.want, Want::Video(..)));
        let fits = |p: &&Plan, most: u32| matches!(p.want, Want::Video(h, _) if h.is_none_or(|h| h <= most));
        let remembered = match (sound, height) {
            (Some(sound), _) => plans.iter().find(|p| p.want == Want::Sound(sound)),
            (_, Some(most)) => videos().find(|p| fits(p, most)),
            _ => None,
        };
        let usual = || if ffmpeg { videos().find(|p| fits(p, 1080)) } else {
            videos().find(|p| !p.needs_ffmpeg()).or_else(|| videos().find(|p| fits(p, 1080)))
        };
        remembered.or_else(usual).or_else(|| plans.first()).cloned().unwrap_or_else(plan::anything)
    }
}

/// What to remember of a choice for next time: ("720p" or "MP3", "MP4"/"WebM"/"MKV" or none).
pub fn remembered(want: Want) -> (String, Option<&'static str>) {
    match want {
        Want::Video(Some(h), c) => (format!("{h}p"), Some(c.name())),
        Want::Video(None, c) => (String::new(), Some(c.name())),
        Want::Sound(s) => (s.name().into(), None),
    }
}
