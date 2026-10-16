//! Picking the streams for what's wanted, out of the many a site lists (YouTube: ~50, every
//! height in three codecs, the sound apart): for a video, the best one of that height that
//! suits the container, with the sound to join to it; for the sound alone, the best sound.

use super::formats::{codec, kind, quality, size, Format, Kind};
use super::info::{Container, Plan, Sound, Want};
use super::Output;

/// Plain HTTP(S) files: TurboDM's connections fetch them (HLS/DASH fragments: yt-dlp).
fn plain(f: &Format) -> bool {
    matches!(f.protocol.as_deref(), Some("https" | "http") | None)
}

fn sdr(f: &Format) -> bool {
    f.dynamic_range.as_deref().is_none_or(|r| r == "SDR")
}

/// How well a video codec suits a container, higher first; None: it can't go in it. MP4 plays
/// everywhere with H.264 (VP9: Windows has it; AV1 may need an add-on); WebM takes only VP9, AV1
/// and VP8; MKV takes anything, the most efficient first.
fn fit(f: &Format, container: Container) -> Option<u8> {
    let c = f.vcodec.as_deref().unwrap_or("");
    let (h264, vp9, av1, vp8) = (c.starts_with("avc1") || c.starts_with("h264"), c.starts_with("vp9") || c.starts_with("vp09"),
                                 c.starts_with("av01"), c.starts_with("vp8"));
    match container {
        Container::Mp4 if vp8 => None,
        Container::Mp4 => Some(if h264 { 3 } else if vp9 { 2 } else if av1 { 1 } else { 0 }),
        Container::WebM => if vp9 { Some(3) } else if av1 { Some(2) } else if vp8 { Some(1) } else { None },
        Container::Mkv => Some(if av1 { 3 } else if vp9 { 2 } else if h264 { 1 } else { 0 }),
    }
}

/// The best video among `formats` for `container` (one that fits it).
fn best_video<'a>(formats: impl Iterator<Item = &'a Format>, container: Container) -> Option<&'a Format> {
    formats.filter_map(|f| fit(f, container).map(|fit| ((sdr(f), plain(f), fit, f.fps.unwrap_or(0.0) as i64,
                                                          f.tbr.unwrap_or(0.0) as i64), f)))
        .max_by_key(|(order, _)| *order).map(|(_, f)| f)
}

fn is_aac(f: &Format) -> bool {
    f.ext == "m4a" || f.acodec.as_deref().is_some_and(|c| c.starts_with("mp4a") || c == "aac")
}

fn is_opus(f: &Format) -> bool {
    f.acodec.as_deref().is_some_and(|c| c == "opus" || c == "vorbis")
}

/// The best sound among `formats`: a plain download, in the site's own language, AAC first
/// when `aac_first` (for MP4), then the better bit rate.
fn best_sound<'a>(formats: impl Iterator<Item = &'a Format>, aac_first: bool) -> Option<&'a Format> {
    formats.max_by_key(|f| (plain(f), f.language_preference.unwrap_or(-1.0) as i64, aac_first && is_aac(f),
                            f.tbr.unwrap_or(0.0) as i64))
}

fn kinds(formats: &[Format]) -> Vec<(&Format, Kind)> {
    let sound_apart = formats.iter().any(|f| codec(&f.vcodec) == Some(false) && codec(&f.acodec) == Some(true));
    formats.iter().filter_map(|f| kind(f, sound_apart).map(|k| (f, k))).collect()
}

/// Video heights shown, at most; the smallest ones (144p, 240p) only when there's little else.
const MAX_VIDEOS: usize = 6;
const SMALL: u32 = 360;

/// The heights to offer, best first (None: a video the site gives no height for).
pub(super) fn heights(formats: &[Format]) -> Vec<Option<u32>> {
    let mut heights: Vec<Option<u32>> = kinds(formats).iter().filter(|(_, k)| *k != Kind::Audio)
        .map(|(f, _)| quality(f)).collect();
    heights.sort_unstable_by(|a, b| b.cmp(a)); // best first, unknown last
    heights.dedup();
    let big = heights.iter().filter(|h| h.is_none_or(|h| h >= SMALL)).count();
    let keep = if big >= 2 { big } else { heights.len() };
    heights.truncate(keep.min(MAX_VIDEOS));
    heights
}

pub(super) fn video(formats: &[Format], duration: Option<f64>, height: Option<u32>, container: Container) -> Option<Plan> {
    let kinds = kinds(formats);
    let at = |kind: Kind| kinds.iter().filter(move |(f, k)| *k == kind && quality(f) == height).map(|(f, _)| *f);
    let sounds = || kinds.iter().filter(|(_, k)| *k == Kind::Audio).map(|(f, _)| *f);
    let want = Want::Video(height, container);
    let wanted_ext = |f: &&Format| f.ext == container.name().to_ascii_lowercase();
    // one file with its sound, already in that container, beats joining two
    if let Some(both) = best_video(at(Kind::Both).filter(wanted_ext), container) {
        return Some(single(both, height, duration, want));
    }
    let video = best_video(at(Kind::Video), container);
    let sound = match container {
        Container::WebM => best_sound(sounds().filter(|f| is_opus(f)), false),
        other => best_sound(sounds(), other == Container::Mp4),
    };
    match (video, sound) {
        (Some(video), Some(sound)) => Some(joined(video, sound, height, duration, container)),
        (Some(video), None) if sounds().next().is_none() => Some(single(video, height, duration, want)), // no sound at all
        _ if container == Container::Mp4 => best_video(at(Kind::Both), Container::Mkv).map(|f| single(f, height, duration, want)),
        _ => None,
    }
}

pub(super) fn sound(formats: &[Format], duration: Option<f64>, sound: Sound) -> Option<Plan> {
    let kinds = kinds(formats);
    let of = |kind: Kind| kinds.iter().filter(move |(_, k)| *k == kind).map(|(f, _)| *f);
    let want = Want::Sound(sound);
    let alone = |f: &Format| Plan { want, label: sound.name().into(), sources: vec![f.source(duration)], output: Output::Single,
                                    ext: f.ext.clone(), size: size(f, duration), format: format!("{}/ba", f.format_id) };
    match sound {
        Sound::M4a => best_sound(of(Kind::Audio).filter(|f| is_aac(f)), true).map(alone),
        Sound::Opus => best_sound(of(Kind::Audio).filter(|f| is_opus(f)), false).map(alone),
        Sound::Mp3 => {
            // the best sound there is; a site that only has videos with their sound: the smallest
            // of those (their sound is mostly the same)
            let from = best_sound(of(Kind::Audio), false)
                .or_else(|| of(Kind::Both).max_by_key(|f| (plain(f), -(quality(f).unwrap_or(0) as i64))))?;
            let mp3 = duration.map(|d| (d * 24_000.0) as u64).or(size(from, duration)); // ~192 kbit/s
            Some(Plan { output: Output::Mp3, ext: "mp3".into(), size: mp3, format: format!("{}/ba/b", from.format_id), ..alone(from) })
        }
    }
}

/// When yt-dlp described nothing well enough: let it pick, and download it itself.
pub(super) fn anything() -> Plan {
    Plan { want: Want::Video(None, Container::Mp4), label: "Best quality".into(), sources: vec![], output: Output::Single,
           ext: "mp4".into(), size: None, format: "b/bv*+ba".into() }
}

/// "MP4", "WebM": a file type as people write it.
fn kind_name(ext: &str) -> String {
    match ext {
        "webm" => "WebM".into(),
        "m4a" => "M4A".into(),
        other => other.to_uppercase(),
    }
}

fn height_label(height: Option<u32>, fps: Option<f64>, note: Option<&str>) -> String {
    let Some(p) = height else { return note.filter(|n| !n.is_empty()).unwrap_or("Video").to_string() };
    let fps = fps.filter(|f| *f > 30.5).map(|f| format!("{}", f.round() as u32)).unwrap_or_default();
    let k = match p {
        4320.. => " 8K",
        2160.. => " 4K",
        _ => "",
    };
    format!("{p}p{fps}{k}")
}

/// A format with both video and sound (or a video without sound), as it comes.
fn single(f: &Format, height: Option<u32>, duration: Option<f64>, want: Want) -> Plan {
    let fallback = f.height.map_or("b".into(), |h| format!("b[height<=?{}]", h as u32));
    Plan {
        want,
        label: format!("{} {}", height_label(height, f.fps, f.format_note.as_deref()), kind_name(&f.ext)),
        sources: vec![f.source(duration)],
        output: Output::Single,
        ext: f.ext.clone(),
        size: size(f, duration),
        format: format!("{}/{fallback}", f.format_id),
    }
}

/// Video and sound apart, joined into `container` by FFmpeg.
fn joined(video: &Format, sound: &Format, height: Option<u32>, duration: Option<f64>, container: Container) -> Plan {
    let (v, a) = (size(video, duration), size(sound, duration));
    let limit = video.height.map(|h| format!("[height<=?{}]", h as u32)).unwrap_or_default();
    let output = container.output();
    Plan {
        want: Want::Video(height, container),
        label: format!("{} {}", height_label(height, video.fps, None), container.name()),
        sources: vec![video.source(duration), sound.source(duration)],
        output,
        ext: output.ext().unwrap_or("mp4").into(),
        size: v.map(|v| v + a.unwrap_or(0)),
        format: format!("{}+{}/bv*{limit}+ba/b{limit}", video.format_id, sound.format_id),
    }
}
