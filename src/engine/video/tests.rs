//! What pages offer (a real YouTube answer from yt-dlp 2026.08, trimmed: testdata/youtube.json,
//! and small made-up ones), names, sites and leftovers.

use super::formats::parse;
use super::*;
use serde_json::Value;

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
    assert_eq!((info.choices.len(), info.site.as_str()), (1, ""));
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
