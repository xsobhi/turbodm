//! The browser's menu of a video's qualities (IDM's "Download this video" list): each a choice
//! with a stable id ("1080p/mp4", "audio/mp3") that the app maps back to what to download.

use super::info::{Container, Plan, Sound, VideoInfo, Want};
use super::{probe, tools};
use crate::engine::http::Headers;
use crate::engine::Shared;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What the browser is told when the helpers aren't set up yet (the app asks for them).
pub const NEEDS_HELPERS: &str = "Videos need yt-dlp and FFmpeg: get them in TurboDM";

/// When the browser last asked for a menu, and how long its choice may take to come.
static ASKED: Mutex<Option<Instant>> = Mutex::new(None);
const CHOOSING: Duration = Duration::from_secs(180);

/// A page is being looked at, or the browser showed a video's menu lately (the choice comes
/// next, and finds the look here): a TurboDM the browser started for it stays meanwhile.
pub fn wanted() -> bool {
    probe::looking() || ASKED.lock().unwrap().is_some_and(|at| at.elapsed() < CHOOSING)
}

/// A choice's id: "1080p/mp4", "video/webm" (a video the site gives no height for), "audio/mp3".
pub fn choice_id(want: Want) -> String {
    match want {
        Want::Video(Some(height), container) => format!("{height}p/{}", container.name().to_ascii_lowercase()),
        Want::Video(None, container) => format!("video/{}", container.name().to_ascii_lowercase()),
        Want::Sound(sound) => format!("audio/{}", sound.name().to_ascii_lowercase()),
    }
}

/// What a choice's id stands for.
pub fn want_of(id: &str) -> Option<Want> {
    let (quality, format) = id.split_once('/')?;
    if quality == "audio" {
        return Sound::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(format)).map(Want::Sound);
    }
    let container = Container::ALL.into_iter().find(|c| c.name().eq_ignore_ascii_case(format))?;
    let height = match quality {
        "video" => None,
        p => Some(p.strip_suffix('p')?.parse().ok()?),
    };
    Some(Want::Video(height, container))
}

fn height(plan: &Plan) -> Option<u32> {
    match plan.want {
        Want::Video(height, _) => height,
        Want::Sound(_) => None,
    }
}

impl VideoInfo {
    /// The menu: each height in `container` (the one picked last), and those found only in
    /// another one (2160p WebM), best first; then the sound alone as MP3, M4A and Opus.
    pub fn menu(&self, container: &str) -> Vec<Plan> {
        let first = Container::ALL.into_iter().find(|c| c.name() == container).unwrap_or(Container::Mp4);
        let others = Container::ALL.into_iter().filter(|c| *c != first);
        let mut videos: Vec<Plan> = vec![];
        for container in std::iter::once(first).chain(others) {
            for plan in self.plans(container).into_iter().filter(|p| matches!(p.want, Want::Video(..))) {
                if !videos.iter().any(|v| height(v) == height(&plan)) {
                    videos.push(plan);
                }
            }
        }
        videos.sort_by_key(|p| std::cmp::Reverse(height(p).map_or(0, |h| h + 1)));
        videos.extend(Sound::ALL.into_iter().filter_map(|s| self.plan(Want::Sound(s))));
        videos
    }
}

/// A page's title and its menu, for the browser: at once if the page was looked at already
/// (the look the browser started while the video played), else once it's looked at.
pub async fn qualities(shared: &Shared, url: &str, headers: &Headers) -> Result<(String, Vec<Plan>), String> {
    if !tools::missing().is_empty() {
        return Err(NEEDS_HELPERS.into());
    }
    let settings = shared.settings();
    let found = probe(&shared.client(), url, headers, settings.timeout_secs).await;
    *ASKED.lock().unwrap() = Some(Instant::now()); // (from the answer: the menu shows now)
    let found = found?;
    let mut menu = found.menu(&settings.video_format);
    if tools::ffmpeg().is_none() {
        menu.retain(|p| !p.needs_ffmpeg()); // (Linux without FFmpeg: what works without it)
    }
    if menu.is_empty() {
        return Err(tools::NO_FFMPEG.into());
    }
    Ok((found.title.clone(), menu))
}

impl VideoInfo {
    /// The plan a menu's choice stands for (None: the page doesn't have it).
    pub fn chosen(&self, id: &str) -> Option<Plan> {
        let want = want_of(id)?;
        self.plan(want).or_else(|| self.menu(Container::Mp4.name()).into_iter().find(|p| p.want == want))
    }
}

#[cfg(test)]
mod tests {
    use super::super::formats::parse;
    use super::*;

    fn rows(menu: &[Plan]) -> Vec<(String, &str)> {
        menu.iter().map(|p| (choice_id(p.want), p.label.as_str())).collect()
    }

    #[test]
    fn youtube_menu_and_its_ids() {
        let info = parse(include_str!("testdata/youtube.json").as_bytes(), "").unwrap();
        let menu = info.menu("MP4");
        assert_eq!(rows(&menu), [("1080p/mp4".into(), "1080p MP4"), ("720p/mp4".into(), "720p MP4"),
                                 ("480p/mp4".into(), "480p MP4"), ("360p/mp4".into(), "360p MP4"),
                                 ("audio/mp3".into(), "MP3"), ("audio/m4a".into(), "M4A"), ("audio/opus".into(), "Opus")]);
        assert_eq!(rows(&info.menu("WebM"))[0], ("1080p/webm".into(), "1080p WebM")); // the one picked last
        // each id stands for its own line
        for plan in info.menu("MKV") {
            let id = choice_id(plan.want);
            assert_eq!(want_of(&id), Some(plan.want), "{id}");
            assert_eq!(info.chosen(&id).as_ref(), Some(&plan), "{id}");
        }
        assert_eq!(info.chosen("2160p/mp4"), None); // not on this page
    }

    #[test]
    fn heights_found_only_in_another_container() {
        // WebM picked last, but 1080p comes only as H.264 (which WebM can't hold)
        let json = r#"{"title": "t", "duration": 10, "formats": [
            {"format_id": "a", "ext": "webm", "vcodec": "none", "acodec": "opus", "tbr": 128},
            {"format_id": "hd", "ext": "mp4", "vcodec": "avc1.64", "acodec": "none", "height": 1080, "tbr": 4000},
            {"format_id": "sd", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 720, "tbr": 2000}
        ]}"#;
        let info = parse(json.as_bytes(), "").unwrap();
        assert_eq!(rows(&info.menu("WebM")), [("1080p/mp4".into(), "1080p MP4"), ("720p/webm".into(), "720p WebM"),
                                              ("audio/mp3".into(), "MP3"), ("audio/opus".into(), "Opus")]);
        assert_eq!(rows(&info.menu("MP4"))[..2], [("1080p/mp4".into(), "1080p MP4"), ("720p/mp4".into(), "720p MP4")]);
    }

    #[test]
    fn ids_as_written() {
        assert_eq!(want_of("1080p/mp4"), Some(Want::Video(Some(1080), Container::Mp4)));
        assert_eq!(want_of("2160p/WEBM"), Some(Want::Video(Some(2160), Container::WebM)));
        assert_eq!(want_of("video/mkv"), Some(Want::Video(None, Container::Mkv)));
        assert_eq!(want_of("audio/opus"), Some(Want::Sound(Sound::Opus)));
        for bad in ["", "1080p", "1080/mp4", "1080p/avi", "audio/flac", "p/mp4", "best"] {
            assert_eq!(want_of(bad), None, "{bad}");
        }
        assert_eq!(choice_id(Want::Video(None, Container::WebM)), "video/webm");
        // a page yt-dlp says little about: its one line is "let yt-dlp pick"
        let info = parse(r#"{"title": "t", "url": "https://x.org/v", "ext": "unknown_video"}"#.as_bytes(), "").unwrap();
        let menu = info.menu("WebM");
        assert_eq!(rows(&menu), [("video/mp4".into(), "Best quality")]);
        assert_eq!(info.chosen("video/mp4").map(|p| p.format), Some("b/bv*+ba".into()));
    }
}
