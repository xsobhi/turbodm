//! What a page offers, from `yt-dlp -J`: its title and a short list of choices built from the
//! many formats sites list (YouTube lists ~50: every height in three codecs, sound apart).

use crate::util::human_size;
use serde::Deserialize;
use serde_json::Value;

/// A video found on a page, and the ways it can be downloaded.
#[derive(Clone, Debug, Default)]
pub struct VideoInfo {
    pub url: String, // the video's own page: what yt-dlp is given when downloading
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>, // seconds
    pub site: String,          // "Youtube", "Vimeo"… (yt-dlp's name for the site)
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

/// One of the site's formats, as yt-dlp describes it (numbers may come as floats or be missing).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct Format {
    format_id: String,
    ext: String,
    vcodec: Option<String>, // "none": no video; missing: unknown
    acodec: Option<String>,
    width: Option<f64>,
    height: Option<f64>,
    fps: Option<f64>,
    filesize: Option<f64>,
    filesize_approx: Option<f64>,
    tbr: Option<f64>, // kbit/s
    protocol: Option<String>,
    format_note: Option<String>,
    dynamic_range: Option<String>,
    has_drm: Value, // true, false or "maybe"
    language_preference: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Page {
    #[serde(rename = "_type")]
    kind: Option<String>,
    title: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
    extractor_key: Option<String>,
    webpage_url: Option<String>,
    formats: Vec<Format>,
    entries: Vec<Value>, // a playlist's videos
    #[serde(flatten)]
    single: Format, // a site with one format describes it at the top
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Both, // video with its sound: one file, nothing to join
    Video,
    Audio,
}

const AUDIO_EXTS: [&str; 8] = ["m4a", "mp3", "aac", "opus", "ogg", "oga", "wav", "flac"];

/// Read `yt-dlp -J` output. `page_url` is used when yt-dlp doesn't say where the video is.
pub fn parse(json: &[u8], page_url: &str) -> Result<VideoInfo, String> {
    let mut page: Page = serde_json::from_slice(json).map_err(|e| format!("yt-dlp's answer can't be read: {e}"))?;
    if page.kind.as_deref() == Some("playlist") {
        // a playlist's page: its first video
        let first = page.entries.drain(..).find(|e| e.get("formats").is_some() || e.get("url").is_some())
            .ok_or("This playlist has no videos")?;
        page = serde_json::from_value(first).map_err(|e| format!("yt-dlp's answer can't be read: {e}"))?;
    }
    if page.formats.is_empty() && !page.single.format_id.is_empty() {
        page.formats.push(page.single.clone());
    }
    let duration = page.duration.filter(|d| *d > 0.0);
    Ok(VideoInfo {
        url: page.webpage_url.clone().filter(|u| u.starts_with("http")).unwrap_or_else(|| page_url.to_string()),
        title: page.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "video".into()),
        thumbnail: page.thumbnail.clone().filter(|t| t.starts_with("http")),
        duration,
        site: site_name(page.extractor_key.as_deref().unwrap_or_default()),
        choices: choices(&page.formats, duration),
        with_cookies: false,
    })
}

/// yt-dlp's name for the site, as people write it ("Youtube" is YouTube); none for a page it
/// found a video on without knowing the site.
fn site_name(key: &str) -> String {
    match key {
        "Generic" | "HTML5MediaEmbed" => "", // (a page's own <video>)
        k if k.starts_with("Youtube") => "YouTube",
        "DailyMotion" => "Dailymotion",
        k if k.starts_with("Twitch") => "Twitch",
        "Twitter" => "X",
        other => other,
    }.to_string()
}

/// Some(true): has it; Some(false): "none"; None: yt-dlp doesn't know.
fn codec(value: &Option<String>) -> Option<bool> {
    value.as_deref().map(|c| c != "none")
}

fn kind(f: &Format, sound_apart: bool) -> Option<Kind> {
    let drm = f.has_drm == Value::Bool(true);
    let storyboard = f.ext == "mhtml" || f.format_note.as_deref().is_some_and(|n| n.contains("storyboard"));
    // YouTube lists each sound twice: the second, quieter-loud-parts copy ("DRC") isn't wanted
    let drc = f.format_id.ends_with("-drc");
    if drm || storyboard || drc {
        return None;
    }
    match (codec(&f.vcodec), codec(&f.acodec)) {
        (Some(false), Some(false)) => None, // pictures
        (Some(false), _) => Some(Kind::Audio),
        (_, Some(false)) => Some(Kind::Video),
        (None, None) if AUDIO_EXTS.contains(&f.ext.as_str()) => Some(Kind::Audio),
        // a known video with unknown sound, on a site that sends its sound apart: video only
        // (YouTube's HLS formats)
        (Some(true), None) if sound_apart => Some(Kind::Video),
        _ => Some(Kind::Both),
    }
}

/// The "720" of 720p: YouTube's own name for it, else the shorter side (a phone's upright video
/// is 1080p, not 1920p).
fn quality(f: &Format) -> Option<u32> {
    let named: Option<u32> = f.format_note.as_deref().and_then(|n| {
        let digits: String = n.chars().take_while(char::is_ascii_digit).collect();
        n[digits.len()..].starts_with('p').then(|| digits.parse().ok()).flatten()
    });
    let side = match (f.width, f.height) {
        (Some(w), Some(h)) => Some(w.min(h)),
        (_, h) => h,
    };
    named.or(side.filter(|s| *s > 0.0).map(|s| s.round() as u32))
}

/// Bytes, from the size yt-dlp gives, or its bit rate and the length.
fn size(f: &Format, duration: Option<f64>) -> Option<u64> {
    let bytes = f.filesize.or(f.filesize_approx).filter(|s| *s > 0.0)
        .or_else(|| Some(f.tbr? * 125.0 * duration?)); // kbit/s → bytes/s
    bytes.map(|b| b as u64)
}

/// Plays everywhere first: H.264 (in MP4 everywhere), VP9 (Windows has it), then AV1 (it may
/// need an add-on), then anything else.
fn codec_rank(f: &Format) -> u8 {
    let c = f.vcodec.as_deref().unwrap_or("");
    if c.starts_with("avc1") || c.starts_with("h264") { 3 }
    else if c.starts_with("vp9") || c.starts_with("vp09") { 2 }
    else if c.starts_with("av01") { 1 }
    else { 0 }
}

/// Plain HTTP(S) downloads resume and run steadily; HLS/DASH fragments less so.
fn plain(f: &Format) -> bool {
    matches!(f.protocol.as_deref(), Some("https" | "http") | None)
}

fn sdr(f: &Format) -> bool {
    f.dynamic_range.as_deref().is_none_or(|r| r == "SDR")
}

/// Better first, for videos of one height.
fn video_order(f: &Format) -> (bool, bool, u8, i64, i64) {
    (sdr(f), plain(f), codec_rank(f), f.fps.unwrap_or(0.0) as i64, f.tbr.unwrap_or(0.0) as i64)
}

/// The sound to join to MP4 video: a plain download, in the site's own language, AAC (M4A)
/// before others, then the better bit rate.
fn audio_order(f: &Format) -> (bool, i64, bool, i64) {
    let aac = f.ext == "m4a" || f.acodec.as_deref().is_some_and(|c| c.starts_with("mp4a"));
    (plain(f), f.language_preference.unwrap_or(-1.0) as i64, aac, f.tbr.unwrap_or(0.0) as i64)
}

fn best<'a>(formats: impl Iterator<Item = &'a Format>, order: fn(&Format) -> (bool, bool, u8, i64, i64)) -> Option<&'a Format> {
    formats.max_by_key(|f| order(f))
}

/// "MP4", "WebM": a file type as people write it.
fn container(ext: &str) -> String {
    match ext {
        "webm" => "WebM".into(),
        "m4a" => "M4A".into(),
        "opus" => "Opus".into(),
        other => other.to_uppercase(),
    }
}

fn height_label(p: u32, fps: Option<f64>) -> String {
    let fps = fps.filter(|f| *f > 30.5).map(|f| format!("{}", f.round() as u32)).unwrap_or_default();
    let k = match p {
        4320.. => " 8K",
        2160.. => " 4K",
        _ => "",
    };
    format!("{p}p{fps}{k}")
}

/// Video qualities shown, at most; the smallest ones (144p, 240p) only when there's little else.
const MAX_VIDEOS: usize = 6;
const SMALL: u32 = 360;

fn choices(formats: &[Format], duration: Option<f64>) -> Vec<Choice> {
    let sound_apart = formats.iter().any(|f| codec(&f.vcodec) == Some(false) && codec(&f.acodec) == Some(true));
    let kinds: Vec<(&Format, Kind)> = formats.iter().filter_map(|f| kind(f, sound_apart).map(|k| (f, k))).collect();
    let of = |kind: Kind| kinds.iter().filter(move |(_, k)| *k == kind).map(|(f, _)| *f);
    let audio = of(Kind::Audio).max_by_key(|f| audio_order(f));

    let mut heights: Vec<Option<u32>> = kinds.iter().filter(|(_, k)| *k != Kind::Audio).map(|(f, _)| quality(f)).collect();
    heights.sort_unstable_by(|a, b| b.cmp(a)); // best first, unknown last
    heights.dedup();
    let big = heights.iter().filter(|h| h.is_none_or(|h| h >= SMALL)).count();
    let keep = if big >= 2 { big } else { heights.len() };

    let mut list = vec![];
    for height in heights.into_iter().take(keep.min(MAX_VIDEOS)) {
        let at = |kind: Kind| of(kind).filter(move |f| quality(f) == height);
        // one file with its sound beats joining two
        let choice = match (best(at(Kind::Both), video_order), best(at(Kind::Video), video_order), audio) {
            (Some(both), ..) => single(both, height, duration),
            (None, Some(video), Some(audio)) => joined(video, audio, height, duration),
            (None, Some(video), None) => single(video, height, duration), // a site without sound
            (None, None, _) => continue,
        };
        list.push(choice);
    }
    if let Some(audio) = audio {
        list.push(Choice {
            label: format!("Audio only ({})", container(&audio.ext)),
            format: format!("{}/ba", audio.format_id),
            ext: audio.ext.clone(),
            size: size(audio, duration),
            merge: false,
            height: None,
            parts: vec![(audio.format_id.clone(), size(audio, duration))],
        });
    }
    if list.is_empty() {
        // nothing yt-dlp described well enough: let it pick
        list.push(Choice { label: "Best quality".into(), format: "b/bv*+ba".into(), ext: "mp4".into(), size: None,
                           merge: false, height: None, parts: vec![] });
    }
    list
}

/// A format with both video and sound (or a video without sound).
fn single(f: &Format, height: Option<u32>, duration: Option<f64>) -> Choice {
    let label = match height {
        Some(p) => format!("{} {}", height_label(p, f.fps), container(&f.ext)),
        None => format!("{} {}", f.format_note.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| "Video".into()),
                        container(&f.ext)),
    };
    let fallback = match f.height {
        Some(h) => format!("b[height<=?{}]", h as u32),
        None => "b".into(),
    };
    Choice {
        label,
        format: format!("{}/{fallback}", f.format_id),
        ext: f.ext.clone(),
        size: size(f, duration),
        merge: false,
        height,
        parts: vec![(f.format_id.clone(), size(f, duration))],
    }
}

/// Video and sound apart, joined into an MP4 by FFmpeg.
fn joined(video: &Format, audio: &Format, height: Option<u32>, duration: Option<f64>) -> Choice {
    let (v, a) = (size(video, duration), size(audio, duration));
    let limit = video.height.map(|h| format!("[height<=?{}]", h as u32)).unwrap_or_default();
    Choice {
        label: format!("{} MP4", height.map(|p| height_label(p, video.fps)).unwrap_or_else(|| "Video".into())),
        format: format!("{}+{}/bv*{limit}+ba/b{limit}", video.format_id, audio.format_id),
        ext: "mp4".into(),
        size: v.map(|v| v + a.unwrap_or(0)),
        merge: true,
        height,
        parts: vec![(video.format_id.clone(), v), (audio.format_id.clone(), a)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YOUTUBE: &str = include_str!("testdata/youtube.json");

    #[test]
    fn youtube_formats_make_a_short_list() {
        let info = parse(YOUTUBE.as_bytes(), "https://youtu.be/Z4C82eyhwgU").unwrap();
        assert_eq!(info.title, "\"Caminandes 2: Gran Dillama\" - Blender Animated Short");
        assert_eq!(info.url, "https://www.youtube.com/watch?v=Z4C82eyhwgU");
        assert_eq!((info.duration, info.site.as_str()), (Some(146.0), "YouTube"));
        let labels: Vec<&str> = info.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["1080p MP4", "720p MP4", "480p MP4", "360p MP4", "Audio only (M4A)"]);
        // H.264 over HTTPS with AAC sound, joined
        let hd = &info.choices[0];
        assert_eq!((hd.format.as_str(), hd.merge, hd.ext.as_str()),
                   ("137+140/bv*[height<=?1080]+ba/b[height<=?1080]", true, "mp4"));
        assert_eq!(hd.size, Some(52740343 + 2365262)); // the two sizes, added
        assert_eq!(hd.parts, [("137".to_string(), Some(52740343)), ("140".to_string(), Some(2365262))]);
        // 360p comes in one file (format 18): no FFmpeg needed
        let small = &info.choices[3];
        assert_eq!((small.format.as_str(), small.merge, small.size), ("18/b[height<=?360]", false, Some(8827849)));
        let audio = &info.choices[4];
        assert_eq!((audio.format.as_str(), audio.ext.as_str(), audio.audio_only()), ("140/ba", "m4a", true));
    }

    #[test]
    fn youtube_without_a_javascript_runtime_joins_every_quality() {
        // then YouTube's players give no format 18: 360p is joined too, and FFmpeg is needed
        let mut json: Value = serde_json::from_str(YOUTUBE).unwrap();
        json["formats"].as_array_mut().unwrap().retain(|f| f["format_id"] != "18");
        let info = parse(json.to_string().as_bytes(), "").unwrap();
        let small = &info.choices[3];
        assert_eq!((small.label.as_str(), small.merge), ("360p MP4", true));
        assert!(small.format.starts_with("134+140/"));
        assert_eq!(info.choices[info.default_choice(false)].label, "1080p MP4"); // and the dialog offers FFmpeg
    }

    #[test]
    fn default_choice_needs_no_ffmpeg_unless_it_is_there() {
        let info = parse(YOUTUBE.as_bytes(), "").unwrap();
        assert_eq!(info.choices[info.default_choice(true)].label, "1080p MP4");
        assert_eq!(info.choices[info.default_choice(false)].label, "360p MP4");
        assert_eq!(info.choice_like("720p"), Some(1));
        assert_eq!(info.choice_like("1440p"), Some(0)); // the best below it
        assert_eq!(info.choice_like("Audio"), Some(4));
        assert_eq!(quality_of(&info.choices[2]), "480p");
    }

    #[test]
    fn a_page_with_one_plain_video() {
        // what yt-dlp's generic extractor says about a page with a <video src=…mp4>
        let json = r#"{"title": "clip", "webpage_url": "https://example.org/page", "extractor_key": "Generic",
                       "format_id": "mp4", "ext": "mp4", "url": "https://example.org/clip.mp4"}"#;
        let info = parse(json.as_bytes(), "https://example.org/page").unwrap();
        assert_eq!(info.choices.len(), 1);
        let only = &info.choices[0];
        assert_eq!((only.label.as_str(), only.format.as_str(), only.merge), ("Video MP4", "mp4/b", false));
    }

    #[test]
    fn heights_fps_and_odd_sizes() {
        let json = r#"{"title": "t", "duration": 10, "formats": [
            {"format_id": "a", "ext": "webm", "vcodec": "none", "acodec": "opus", "tbr": 128},
            {"format_id": "v60", "ext": "mp4", "vcodec": "avc1.64", "acodec": "none", "width": 1080, "height": 1920, "fps": 60, "tbr": 4000},
            {"format_id": "v4k", "ext": "webm", "vcodec": "vp9", "acodec": "none", "width": 3840, "height": 2160, "fps": 30},
            {"format_id": "hdr", "ext": "webm", "vcodec": "vp09.02", "acodec": "none", "width": 3840, "height": 2160, "dynamic_range": "HDR10"}
        ]}"#;
        let info = parse(json.as_bytes(), "").unwrap();
        let labels: Vec<&str> = info.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["2160p 4K MP4", "1080p60 MP4", "Audio only (WebM)"]);
        assert!(info.choices[0].format.starts_with("v4k+a/")); // SDR before HDR
        assert_eq!(info.choices[1].size, Some(4000 * 125 * 10 + 128 * 125 * 10)); // from the bit rates
        assert_eq!(info.choices[0].size, None); // nothing known about the video's size
    }

    #[test]
    fn playlist_pages_give_their_first_video() {
        let json = r#"{"_type": "playlist", "title": "list", "entries": [
            {"title": "first", "webpage_url": "https://x.org/v/1", "formats": [{"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a"}]}]}"#;
        let info = parse(json.as_bytes(), "https://x.org/list").unwrap();
        assert_eq!((info.title.as_str(), info.url.as_str()), ("first", "https://x.org/v/1"));
        assert_eq!(info.choices[0].label, "360p MP4");
    }
}
