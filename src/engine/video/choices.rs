//! The short list of choices made from a page's many formats (YouTube lists ~50: every height
//! in three codecs, the sound apart): one per height, best first, then the sound alone.

use super::formats::{codec, kind, quality, size, Format, Kind};
use super::info::Choice;

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

fn best<'a>(formats: impl Iterator<Item = &'a Format>) -> Option<&'a Format> {
    formats.max_by_key(|f| video_order(f))
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

pub(super) fn choices(formats: &[Format], duration: Option<f64>) -> Vec<Choice> {
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
        let choice = match (best(at(Kind::Both)), best(at(Kind::Video)), audio) {
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
