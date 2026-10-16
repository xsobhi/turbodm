//! What pages offer (a real YouTube answer from yt-dlp 2026.08, trimmed: testdata/youtube.json,
//! and small made-up ones), the plans made of it, names, sites, saved downloads.

use super::formats::parse;
use super::names::{is_leftover, part_file};
use super::*;
use serde_json::Value;

const YOUTUBE: &str = include_str!("testdata/youtube.json");

fn labels(plans: &[Plan]) -> Vec<&str> {
    plans.iter().map(|p| p.label.as_str()).collect()
}

fn ids(plan: &Plan) -> Vec<&str> {
    plan.sources.iter().map(|s| s.format_id.as_str()).collect()
}

#[test]
fn youtube_in_each_format() {
    let info = parse(YOUTUBE.as_bytes(), "https://youtu.be/Z4C82eyhwgU").unwrap();
    assert_eq!(info.title, "\"Caminandes 2: Gran Dillama\" - Blender Animated Short");
    assert_eq!(info.url, "https://www.youtube.com/watch?v=Z4C82eyhwgU");
    assert_eq!((info.duration, info.site.as_str()), (Some(146.0), "YouTube"));
    assert_eq!(info.containers(), [Container::Mp4, Container::WebM, Container::Mkv]);
    let mp4 = info.plans(Container::Mp4);
    assert_eq!(labels(&mp4), ["1080p MP4", "720p MP4", "480p MP4", "360p MP4", "MP3", "M4A", "Opus"]);
    // H.264 over HTTPS with AAC sound, joined
    let hd = &mp4[0];
    assert_eq!((ids(hd), hd.output, hd.ext.as_str()), (vec!["137", "140"], Output::Mp4, "mp4"));
    assert_eq!(hd.format, "137+140/bv*[height<=?1080]+ba/b[height<=?1080]");
    assert_eq!(hd.size, Some(52740343 + 2365262));
    // 360p comes in one file (format 18): nothing to join
    assert_eq!((ids(&mp4[3]), mp4[3].output, mp4[3].needs_ffmpeg()), (vec!["18"], Output::Single, false));
    // the sound alone: MP3 made of the best (Opus, 139k), M4A and Opus as they come
    assert_eq!((ids(&mp4[4]), mp4[4].output, mp4[4].ext.as_str()), (vec!["251"], Output::Mp3, "mp3"));
    assert_eq!(mp4[4].size, Some(146 * 24_000));
    assert_eq!((ids(&mp4[5]), mp4[5].output, mp4[5].ext.as_str()), (vec!["140"], Output::Single, "m4a"));
    assert_eq!((ids(&mp4[6]), mp4[6].ext.as_str()), (vec!["251"], "webm"));
    // WebM: VP9 and Opus; MKV: the most efficient video (AV1) and the best sound
    let webm = info.plans(Container::WebM);
    assert_eq!((webm[0].label.as_str(), ids(&webm[0]), webm[0].output), ("1080p WebM", vec!["248", "251"], Output::WebM));
    let mkv = info.plans(Container::Mkv);
    assert_eq!((mkv[0].label.as_str(), ids(&mkv[0]), mkv[0].ext.as_str()), ("1080p MKV", vec!["399", "251"], "mkv"));
}

#[test]
fn what_is_offered_first() {
    let info = parse(YOUTUBE.as_bytes(), "").unwrap();
    assert_eq!(info.first("", "", true).label, "1080p MP4");
    assert_eq!(info.first("", "", false).label, "360p MP4"); // no FFmpeg: what needs none
    assert_eq!(info.first("720p", "WebM", true).label, "720p WebM");
    assert_eq!(info.first("1440p", "MP4", true).label, "1080p MP4"); // the best below it
    assert_eq!(info.first("MP3", "MKV", true).label, "MP3");
    assert_eq!(info.first("Audio", "", true).label, "M4A"); // TurboDM 1.8's "Audio"
    assert_eq!(remembered(info.first("480p", "MKV", true).want), ("480p".into(), Some("MKV")));
    assert_eq!(remembered(Want::Sound(Sound::Opus)), ("Opus".into(), None));
}

#[test]
fn youtube_without_a_javascript_runtime_joins_every_quality() {
    // then YouTube's players give no format 18: 360p is joined too, and FFmpeg is needed
    let mut json: Value = serde_json::from_str(YOUTUBE).unwrap();
    json["formats"].as_array_mut().unwrap().retain(|f| f["format_id"] != "18");
    let info = parse(json.to_string().as_bytes(), "").unwrap();
    let small = &info.plans(Container::Mp4)[3];
    assert_eq!((small.label.as_str(), ids(small)), ("360p MP4", vec!["134", "140"]));
    assert_eq!(info.first("", "", false).label, "1080p MP4"); // and the dialog offers FFmpeg
}

#[test]
fn a_page_with_one_plain_video() {
    // what yt-dlp's generic extractor says about a page with a <video src=…mp4>
    let json = r#"{"title": "clip", "webpage_url": "https://example.org/page", "extractor_key": "Generic",
                   "format_id": "mp4", "ext": "mp4", "url": "https://example.org/clip.mp4", "protocol": "https"}"#;
    let info = parse(json.as_bytes(), "https://example.org/page").unwrap();
    assert_eq!((info.containers(), info.site.as_str()), (vec![Container::Mp4], ""));
    let plans = info.plans(Container::Mp4);
    assert_eq!(labels(&plans), ["Video MP4", "MP3"]); // (MP3: made of the video's sound)
    assert_eq!((plans[0].format.as_str(), plans[0].output), ("mp4/b", Output::Single));
    assert!(plans[0].sources[0].plain());
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
    let plans = info.plans(Container::Mp4);
    assert_eq!(labels(&plans), ["2160p 4K MP4", "1080p60 MP4", "MP3", "Opus"]);
    assert_eq!(ids(&plans[0]), ["v4k", "a"]); // SDR before HDR (and Opus in the MP4: there's no AAC)
    assert_eq!(plans[1].size, Some(4000 * 125 * 10 + 128 * 125 * 10)); // from the bit rates
    assert_eq!(plans[0].size, None); // nothing known about the video's size
}

#[test]
fn playlist_pages_give_their_first_video() {
    let json = r#"{"_type": "playlist", "title": "list", "entries": [
        {"title": "first", "webpage_url": "https://x.org/v/1", "formats": [{"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a"}]}]}"#;
    let info = parse(json.as_bytes(), "https://x.org/list").unwrap();
    assert_eq!((info.title.as_str(), info.url.as_str()), ("first", "https://x.org/v/1"));
    assert_eq!(info.plans(Container::Mp4)[0].label, "360p MP4");
}

#[test]
fn where_each_stream_is_and_what_to_send() {
    let json = r#"{"title": "t", "duration": 2, "formats": [
        {"format_id": "v", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 720, "protocol": "https",
         "url": "https://cdn.x/v", "downloader_options": {"http_chunk_size": 10485760},
         "http_headers": {"User-Agent": "UA", "Referer": "https://x.org/", "Accept": "*/*"},
         "cookies": "tt=1; Domain=.x.org; Path=/; Secure; Expires=1900000000; sid=a=b"},
        {"format_id": "hls", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 480, "protocol": "m3u8_native",
         "url": "https://cdn.x/index.m3u8"},
        {"format_id": "s", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "protocol": "https", "url": "https://cdn.x/s"}
    ]}"#;
    let info = parse(json.as_bytes(), "").unwrap();
    let hd = &info.plans(Container::Mp4)[0];
    let video = &hd.sources[0];
    assert_eq!((video.url.as_str(), video.chunk, video.plain()), ("https://cdn.x/v", 10485760, true));
    let header = |name: &str| video.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    assert_eq!((header("User-Agent"), header("Referer"), header("Accept")), (Some("UA"), Some("https://x.org/"), Some("*/*")));
    assert_eq!(header("Cookie"), Some("tt=1; sid=a=b"));
    assert!(!info.plans(Container::Mp4)[1].sources[0].plain()); // HLS: yt-dlp's to download
}

#[test]
fn knows_video_sites_and_pages() {
    for page in ["https://www.youtube.com/watch?v=Z4C82eyhwgU", "https://youtu.be/Z4C82eyhwgU?t=3",
                 "https://m.youtube.com/shorts/abc", "https://vimeo.com/76979871", "https://www.instagram.com/reel/C1/",
                 "https://www.facebook.com/user/videos/123/", "https://fb.watch/abc/", "https://www.tiktok.com/@u/video/1",
                 "https://vm.tiktok.com/ZM1/", "https://x.com/u/status/1", "https://www.bilibili.com/video/BV1",
                 "https://b23.tv/abc", "https://v.redd.it/abc", "https://clips.twitch.tv/Clip", "https://dai.ly/x1"] {
        assert!(is_video_site(page), "{page}");
    }
    assert!(!is_video_site("https://notyoutube.com/watch"));
    assert!(!is_video_site("https://example.org/movie.mp4"));
    assert!(!is_video_site("https://vimeo.com/files/movie.zip")); // a file is a file
    assert!(is_youtube("https://user@www.youtube.com:443/watch?v=x") && !is_youtube("https://vimeo.com/1"));
    assert!(is_web_page("text/html") && !is_web_page("video/mp4"));
    assert_eq!(super::sites::cookie_domain("https://www.instagram.com/p/x/"), ".instagram.com");
    assert_eq!(super::sites::cookie_domain("https://www.example.org/v"), ".example.org");
    assert_eq!(super::sites::alternative("https://vimeo.com/channels/staffpicks/76979871?x=1").as_deref(),
               Some("https://player.vimeo.com/video/76979871"));
    assert_eq!(super::sites::alternative("https://player.vimeo.com/video/1"), None);
}

#[test]
fn names_and_leftovers() {
    assert_eq!(filename("\"Caminandes 2: Gran Dillama\" | Blender?", "mp4"), "'Caminandes 2 - Gran Dillama' - Blender.mp4");
    assert_eq!(filename(&"x".repeat(300), "m4a").len(), 124);
    assert_eq!(part_file(std::path::Path::new("d"), "a b", "hls-720/x", "mp4"), std::path::Path::new("d").join("a b.fhls-720_x.mp4"));
    for left in ["a b.mp4.part", "a b.f137.mp4", "a b.f137.mp4.tdmpart", "a b.f251-drc.webm.part", "a b.mp4.ytdl",
                 "a b.mp4.part-Frag12", "a b.temp.mp4"] {
        assert!(is_leftover(left, "a b"), "{left}");
    }
    for keep in ["a b.mp4", "a b.srt", "a bc.mp4.part", "a b.final.cut.mp4", "a b.f.mp4"] {
        assert!(!is_leftover(keep, "a b"), "{keep}");
    }
}

#[test]
fn downloads_saved_by_older_versions_load() {
    // 1.8 saved neither the output nor the parts' addresses: joined meant an MP4
    let job: VideoJob = serde_json::from_str(r#"{"format": "137+140/b", "quality": "1080p MP4", "merge": true,
                                                 "parts": [["137", 52740343], ["140", null]], "done": 5}"#).unwrap();
    assert_eq!((job.output(), job.sources.len(), job.parts[1].1, job.by_ytdlp), (Output::Mp4, 0, None, false));
    let job: VideoJob = serde_json::from_str(r#"{"format": "18/b", "quality": "360p MP4"}"#).unwrap();
    assert_eq!((job.output(), job.done), (Output::Single, 0));
}

#[test]
fn parts_show_as_connections_one_after_the_other() {
    let source = |size, done| Source { size: Some(size), done, ..Default::default() };
    let job = VideoJob { sources: vec![source(100, true), source(50, false)], ..Default::default() };
    let current = vec![Segment { start: 0, end: Some(25), done: 25, ahead: 0, active: false, heard: None },
                       Segment { start: 25, end: Some(50), done: 10, ahead: 0, active: true, heard: None }];
    let shown = shown_segments(&job, current);
    let spans: Vec<_> = shown.iter().map(|s| (s.start, s.end, s.done, s.active)).collect();
    assert_eq!(spans, [(0, Some(100), 100, false), (100, Some(125), 25, false), (125, Some(150), 10, true)]);
}
