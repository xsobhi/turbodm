//! Reading `yt-dlp -J`: the page's video and every format it comes in, as yt-dlp describes
//! them (numbers may come as floats or be missing); choices.rs makes the short list from them.

use super::choices::choices;
use super::info::VideoInfo;
use serde::Deserialize;
use serde_json::Value;

/// One of the site's formats.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Format {
    pub(super) format_id: String,
    pub(super) ext: String,
    pub(super) vcodec: Option<String>, // "none": no video; missing: unknown
    pub(super) acodec: Option<String>,
    pub(super) width: Option<f64>,
    pub(super) height: Option<f64>,
    pub(super) fps: Option<f64>,
    pub(super) filesize: Option<f64>,
    pub(super) filesize_approx: Option<f64>,
    pub(super) tbr: Option<f64>, // kbit/s
    pub(super) protocol: Option<String>,
    pub(super) format_note: Option<String>,
    pub(super) dynamic_range: Option<String>,
    pub(super) has_drm: Value, // true, false or "maybe"
    pub(super) language_preference: Option<f64>,
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
pub(super) enum Kind {
    Both, // video with its sound: one file, nothing to join
    Video,
    Audio,
}

const AUDIO_EXTS: [&str; 8] = ["m4a", "mp3", "aac", "opus", "ogg", "oga", "wav", "flac"];

/// Read `yt-dlp -J` output. `page_url` is used when yt-dlp doesn't say where the video is.
pub(super) fn parse(json: &[u8], page_url: &str) -> Result<VideoInfo, String> {
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
pub(super) fn codec(value: &Option<String>) -> Option<bool> {
    value.as_deref().map(|c| c != "none")
}

/// Video with sound, video only, sound only; None for what isn't wanted.
pub(super) fn kind(f: &Format, sound_apart: bool) -> Option<Kind> {
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
pub(super) fn quality(f: &Format) -> Option<u32> {
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
pub(super) fn size(f: &Format, duration: Option<f64>) -> Option<u64> {
    let bytes = f.filesize.or(f.filesize_approx).filter(|s| *s > 0.0)
        .or_else(|| Some(f.tbr? * 125.0 * duration?)); // kbit/s → bytes/s
    bytes.map(|b| b as u64)
}
