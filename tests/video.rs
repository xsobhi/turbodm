//! Videos end to end, with yt-dlp and a real site: not run by default (they need the network).
//!
//!   TURBODM_HOME=<scratch folder> cargo test --release --test video -- --ignored --test-threads 1
//!
//! TURBODM_HOME keeps the tools yt-dlp and FFmpeg out of your own TurboDM's folder. VIDEO_URL
//! picks another video than the short Creative Commons one used by default.

mod common;

use common::harness::Env;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use turbodm::config::Settings;
use turbodm::engine::http::{build_client, Headers};
use turbodm::engine::video::{self, tools, VideoInfo, VideoJob};
use turbodm::engine::{AddRequest, Manager, Snapshot, Status};

/// "Caminandes 2: Gran Dillama", Blender Foundation (CC BY), 2:26.
const DEFAULT_URL: &str = "https://www.youtube.com/watch?v=Z4C82eyhwgU";

fn url() -> String {
    std::env::var("VIDEO_URL").unwrap_or_else(|_| DEFAULT_URL.into())
}

fn sandboxed() {
    assert!(turbodm::config::sandbox().is_some(), "set TURBODM_HOME to a scratch folder (tools go there)");
}

fn manager(env: &Env, limit_kib: u64) -> Arc<Manager> {
    let settings = Settings { download_dir: env.dir.clone(), use_categories: false, speed_limit_kib: limit_kib,
                              timeout_secs: 20, ..Settings::default() };
    Manager::new(settings, env.dir.join("downloads.json"), env.rt.handle().clone()).0
}

fn probe(env: &Env) -> VideoInfo {
    let client = build_client(Duration::from_secs(20));
    env.rt.block_on(video::probe(&client, &url(), &Headers::default(), 20)).expect("probe")
}

fn add(manager: &Arc<Manager>, found: &VideoInfo, index: usize, dir: &Path) -> String {
    let choice = &found.choices[index];
    manager.add(AddRequest { url: found.url.clone(), filename: Some(video::filename(&found.title, &choice.ext)),
                             directory: Some(dir.to_path_buf()), start: true,
                             video: Some(VideoJob::new(found, choice)), ..Default::default() })
}

fn wait_for(manager: &Manager, id: &str, secs: u64, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let snap = manager.get(id).unwrap();
        if done(&snap) {
            return snap;
        }
        assert!(Instant::now() < deadline, "timeout: {:?} {:?} {}", snap.status, snap.error, snap.downloaded);
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// An MP4 (or M4A): its first box is "ftyp".
fn assert_mp4(path: &Path) {
    let head = std::fs::read(path).unwrap();
    assert_eq!(&head[4..8], b"ftyp", "{} isn't an MP4", path.display());
}

/// Nothing of yt-dlp's left beside it.
fn assert_no_leftovers(dir: &Path) {
    let names: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".part") || n.ends_with(".ytdl") || n.contains(".f") && !n.ends_with(".json")).collect();
    assert!(names.is_empty(), "left behind: {names:?}");
}

#[test]
#[ignore = "needs the network"]
fn pauses_resumes_after_a_restart_and_completes() {
    sandboxed();
    let env = Env::new("video-resume");
    let found = probe(&env);
    let ffmpeg = tools::ffmpeg().is_some();
    // the smallest video that needs no FFmpeg if there's none
    let index = found.choices.iter().rposition(|c| !c.audio_only() && (ffmpeg || !c.merge))
        .or_else(|| found.choices.iter().position(|c| c.audio_only())).expect("a choice");
    let manager = manager(&env, 300); // slowly: there's time to pause
    let id = add(&manager, &found, index, &env.dir);
    let live = wait_for(&manager, &id, 90, |s| s.downloaded > 600 << 10 || s.status == Status::Error);
    assert_eq!(live.status, Status::Downloading, "{:?}", live.error);
    assert!(live.size.is_some() && live.video.is_some());
    manager.pause(&id);
    let paused = wait_for(&manager, &id, 30, |s| s.status == Status::Paused);
    let parts: Vec<_> = std::fs::read_dir(&env.dir).unwrap().flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".part")).collect();
    assert!(!parts.is_empty(), "no partial file kept");
    manager.shutdown(); // as when TurboDM is closed
    drop(manager);

    let manager = self::manager(&env, 0);
    assert_eq!(manager.get(&id).unwrap().status, Status::Paused);
    manager.resume(&id);
    // it carries on: what was downloaded is counted from the start
    let again = wait_for(&manager, &id, 90, |s| s.status == Status::Downloading && s.downloaded > 0);
    assert!(again.downloaded + (64 << 10) >= paused.downloaded, "{} after {}", again.downloaded, paused.downloaded);
    let done = wait_for(&manager, &id, 180, |s| matches!(s.status, Status::Completed | Status::Error));
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert_eq!(std::fs::metadata(&done.path).unwrap().len(), done.size.unwrap());
    assert_eq!(done.downloaded, done.size.unwrap());
    assert_mp4(&done.path);
    assert_no_leftovers(&env.dir);
    manager.shutdown();
}

#[test]
#[ignore = "needs the network and FFmpeg"]
fn joins_video_and_sound_with_ffmpeg() {
    sandboxed();
    if tools::ffmpeg().is_none() {
        eprintln!("no FFmpeg: skipped (fetch it with `turbodm video URL -q 720p --ffmpeg`)");
        return;
    }
    let env = Env::new("video-join");
    let found = probe(&env);
    let index = found.choice_like("720p").expect("720p or less");
    assert!(found.choices[index].merge, "{:?}", found.choices[index]);
    let manager = manager(&env, 0);
    let id = add(&manager, &found, index, &env.dir);
    let done = wait_for(&manager, &id, 300, |s| matches!(s.status, Status::Completed | Status::Error));
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert!(done.filename.ends_with(".mp4"));
    assert_mp4(&done.path);
    assert_no_leftovers(&env.dir);
    manager.shutdown();
}

#[test]
#[ignore = "needs the network"]
fn remove_with_files_deletes_the_partial_video() {
    sandboxed();
    let env = Env::new("video-remove");
    let found = probe(&env);
    let index = found.choices.iter().position(|c| c.audio_only()).expect("sound only");
    let manager = manager(&env, 200);
    let id = add(&manager, &found, index, &env.dir);
    wait_for(&manager, &id, 90, |s| s.downloaded > 200 << 10);
    manager.remove(&id, true);
    let deadline = Instant::now() + Duration::from_secs(20);
    while manager.get(&id).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(manager.get(&id).is_none());
    let left: Vec<_> = std::fs::read_dir(&env.dir).unwrap().flatten().map(|e| e.file_name())
        .filter(|n| !n.to_string_lossy().ends_with(".json")).collect();
    assert!(left.is_empty(), "left behind: {left:?}");
    manager.shutdown();
}
