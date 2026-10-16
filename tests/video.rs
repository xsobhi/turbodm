//! Videos end to end, with yt-dlp and a real site: not run by default (they need the network).
//!
//!   TURBODM_HOME=<scratch folder> cargo test --release --test video -- --ignored --test-threads 1
//!
//! TURBODM_HOME keeps the helpers (yt-dlp, FFmpeg) out of your own TurboDM's folder; put them in
//! its data/tools first, or the tests that need FFmpeg skip. VIDEO_URL picks another video than
//! the short Creative Commons one used by default (~25 MB in all).

mod common;

use common::harness::Env;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use turbodm::config::Settings;
use turbodm::engine::http::{build_client, Headers};
use turbodm::engine::video::{self, tools, Container, Plan, Sound, Source, VideoInfo, VideoJob, Want};
use turbodm::engine::{AddRequest, Manager, Snapshot, Status};

/// "Caminandes 2: Gran Dillama", Blender Foundation (CC BY), 2:26.
const DEFAULT_URL: &str = "https://www.youtube.com/watch?v=Z4C82eyhwgU";

fn url() -> String {
    std::env::var("VIDEO_URL").unwrap_or_else(|_| DEFAULT_URL.into())
}

fn sandboxed() {
    assert!(turbodm::config::sandbox().is_some(), "set TURBODM_HOME to a scratch folder (the helpers go there)");
}

fn manager(env: &Env, limit_kib: u64, quality: &str) -> Arc<Manager> {
    let settings = Settings { download_dir: env.dir.clone(), use_categories: false, speed_limit_kib: limit_kib,
                              timeout_secs: 20, video_quality: quality.into(), ..Settings::default() };
    Manager::new(settings, env.dir.join("downloads.json"), env.rt.handle().clone()).0
}

fn probe(env: &Env) -> VideoInfo {
    let client = build_client(Duration::from_secs(20));
    env.rt.block_on(video::probe(&client, &url(), &Headers::default(), 20)).expect("probe")
}

fn add(manager: &Arc<Manager>, found: &VideoInfo, plan: &Plan, dir: &Path) -> String {
    add_job(manager, found, VideoJob::new(found, plan), &video::filename(&found.title, &plan.ext), dir)
}

fn add_job(manager: &Arc<Manager>, found: &VideoInfo, job: VideoJob, name: &str, dir: &Path) -> String {
    manager.add(AddRequest { url: found.url.clone(), filename: Some(name.into()), directory: Some(dir.to_path_buf()),
                             start: true, video: Some(job), ..Default::default() })
}

fn wait_for(manager: &Manager, id: &str, secs: u64, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let snap = manager.get(id).unwrap();
        if done(&snap) {
            return snap;
        }
        assert!(Instant::now() < deadline, "timeout: {:?} {:?} {}", snap.status, snap.error, snap.downloaded);
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn ended(s: &Snapshot) -> bool {
    matches!(s.status, Status::Completed | Status::Error)
}

/// Its first bytes: an MP4/M4A ("ftyp"), a WebM ("\x1a\x45\xdf\xa3"), an MP3 (an ID3 tag or a frame).
fn assert_kind(path: &Path, kind: &str) {
    let head = std::fs::read(path).unwrap();
    let ok = match kind {
        "mp4" => &head[4..8] == b"ftyp",
        "webm" => head.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
        "mp3" => head.starts_with(b"ID3") || (head[0] == 0xff && head[1] & 0xe0 == 0xe0),
        _ => false,
    };
    assert!(ok, "{} isn't {kind}", path.display());
}

/// Only the video and the list left: no parts, no partial files.
fn assert_clean(dir: &Path, video: &Path) {
    let left: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path())
        .filter(|p| p != video && !p.to_string_lossy().contains("downloads.json")).collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}

fn ffmpeg_or_skip() -> bool {
    let here = tools::ffmpeg().is_some();
    if !here {
        eprintln!("no FFmpeg in TURBODM_HOME's tools: skipped");
    }
    here
}

#[test]
#[ignore = "needs the network"]
fn downloaded_on_many_connections_paused_restarted_and_finished() {
    sandboxed();
    let env = Env::new("video-resume");
    let found = probe(&env);
    // a video that needs no joining (YouTube's 360p in one file), slowly: there's time to pause
    let plan = found.plans(Container::Mp4).into_iter().rfind(|p| matches!(p.want, Want::Video(..)) && !p.needs_ffmpeg())
        .expect("a video in one file (needs Node for YouTube's)");
    let manager = manager(&env, 400, "");
    let id = add(&manager, &found, &plan, &env.dir);
    let live = wait_for(&manager, &id, 90, |s| (s.downloaded > 1 << 20 && s.active_connections > 1) || ended(s));
    assert_eq!(live.status, Status::Downloading, "{:?}", live.error);
    assert!(!live.by_ytdlp && live.segments.len() > 1, "on one connection: {:?}", live.segments.len());
    manager.pause(&id);
    let paused = wait_for(&manager, &id, 30, |s| s.status == Status::Paused);
    let partial = std::fs::read_dir(&env.dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".tdmpart"));
    assert!(partial, "no partial file kept");
    manager.shutdown(); // as when TurboDM is closed
    drop(manager);

    let manager = self::manager(&env, 0, "");
    let again = manager.get(&id).unwrap();
    assert_eq!(again.status, Status::Paused);
    assert!(again.downloaded + (256 << 10) >= paused.downloaded, "{} after {}", again.downloaded, paused.downloaded);
    manager.resume(&id);
    let done = wait_for(&manager, &id, 180, ended);
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert_eq!((done.downloaded, std::fs::metadata(&done.path).unwrap().len()), (done.size.unwrap(), done.size.unwrap()));
    assert_kind(&done.path, "mp4");
    assert_clean(&env.dir, &done.path);
    manager.shutdown();
}

#[test]
#[ignore = "needs the network and FFmpeg"]
fn video_and_sound_joined_into_webm() {
    sandboxed();
    if !ffmpeg_or_skip() {
        return;
    }
    let env = Env::new("video-webm");
    let found = probe(&env);
    let plan = found.plans(Container::WebM).into_iter().rfind(|p| matches!(p.want, Want::Video(..))).expect("a WebM");
    assert_eq!(plan.sources.len(), 2, "{plan:?}");
    let manager = manager(&env, 0, "");
    let id = add(&manager, &found, &plan, &env.dir);
    let done = wait_for(&manager, &id, 300, ended);
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert!(done.filename.ends_with(".webm"));
    assert_kind(&done.path, "webm");
    assert_clean(&env.dir, &done.path);
    manager.shutdown();
}

#[test]
#[ignore = "needs the network and FFmpeg"]
fn the_sound_alone_as_mp3() {
    sandboxed();
    if !ffmpeg_or_skip() {
        return;
    }
    let env = Env::new("video-mp3");
    let found = probe(&env);
    let plan = found.plan(Want::Sound(Sound::Mp3)).expect("MP3");
    let manager = manager(&env, 0, "");
    let id = add(&manager, &found, &plan, &env.dir);
    let done = wait_for(&manager, &id, 300, ended);
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert!(done.filename.ends_with(".mp3"));
    assert_kind(&done.path, "mp3");
    assert_clean(&env.dir, &done.path);
    manager.shutdown();
}

#[test]
#[ignore = "needs the network"]
fn without_its_dialog_it_picks_what_was_picked_last() {
    // (from the browser with the Add dialog turned off, and on Linux)
    sandboxed();
    let env = Env::new("video-auto");
    let manager = manager(&env, 0, "M4A");
    let id = manager.add(AddRequest { url: url(), start: true, video: Some(VideoJob::default()), ..Default::default() });
    let done = wait_for(&manager, &id, 180, ended);
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert_eq!(done.video.as_deref(), Some("M4A"));
    assert!(done.filename.ends_with(".m4a") && done.filename.starts_with("'Caminandes 2"), "{}", done.filename);
    assert_kind(&done.path, "mp4");
    manager.shutdown();
}

#[test]
#[ignore = "needs the network"]
fn an_expired_address_is_asked_for_again() {
    sandboxed();
    let env = Env::new("video-expired");
    let found = probe(&env);
    let plan = found.plan(Want::Sound(Sound::M4a)).expect("M4A");
    let mut job = VideoJob::new(&found, &plan);
    // as hours later: the site no longer takes it (its signature, spoilt)
    let url = &mut job.sources[0].url;
    let at = url.find("sig=").expect("a signed address") + 4;
    let spoilt = if url[at..].starts_with('A') { "B" } else { "A" };
    url.replace_range(at..at + 1, spoilt);
    let manager = manager(&env, 0, "");
    let id = add_job(&manager, &found, job, "expired.m4a", &env.dir);
    let done = wait_for(&manager, &id, 180, ended);
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert!(!done.by_ytdlp, "left to yt-dlp instead");
    assert_kind(&done.path, "mp4");
    manager.shutdown();
}

#[test]
#[ignore = "needs the network"]
fn streams_in_fragments_are_left_to_yt_dlp() {
    sandboxed();
    let env = Env::new("video-hls");
    let found = probe(&env);
    // YouTube's HLS sound (format 233): fragments TurboDM's connections don't fetch
    let hls = Source { format_id: "233".into(), protocol: "m3u8_native".into(), url: "https://x/index.m3u8".into(),
                       ext: "mp4".into(), ..Default::default() };
    let job = VideoJob { format: "233/ba".into(), quality: "HLS".into(), parts: vec![("233".into(), None)],
                         title: found.title.clone(), sources: vec![hls], ..Default::default() };
    let manager = manager(&env, 0, "");
    let id = add_job(&manager, &found, job, "hls.mp4", &env.dir);
    let busy = wait_for(&manager, &id, 120, |s| s.by_ytdlp || ended(s));
    assert!(busy.by_ytdlp, "{:?}", busy.error);
    let done = wait_for(&manager, &id, 180, ended);
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    manager.shutdown();
}

#[test]
#[ignore = "needs the network"]
fn remove_with_files_deletes_the_partial_video() {
    sandboxed();
    let env = Env::new("video-remove");
    let found = probe(&env);
    let plan = found.plan(Want::Sound(Sound::M4a)).expect("M4A");
    let manager = manager(&env, 150, "");
    let id = add(&manager, &found, &plan, &env.dir);
    wait_for(&manager, &id, 90, |s| s.downloaded > 200 << 10);
    manager.remove(&id, true);
    let deadline = Instant::now() + Duration::from_secs(20);
    while manager.get(&id).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(manager.get(&id).is_none());
    assert_clean(&env.dir, Path::new(""));
    manager.shutdown();
}
