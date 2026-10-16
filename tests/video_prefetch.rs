//! A video's early download (the quality picked in its dialog, downloading behind it): its
//! parts on one connection, then `confirm` moves them to the name and folder chosen there and
//! gives them all their connections, finished or not. Local servers only: no yt-dlp, no network.

mod common;

use common::harness::*;
use common::{random_bytes, start, Options, TestServer};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};
use turbodm::engine::manager::{Confirm, Manager};
use turbodm::engine::video::{tools, Output, Source, VideoJob};
use turbodm::engine::{AddRequest, Status};

const PAGE: &str = "https://example.org/watch?v=clip";

/// A data folder of this test's own (its tools: none, unless FFmpeg is on PATH). Each test
/// calls it first, before any thread of its own reads the environment.
fn sandbox() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let home = std::env::temp_dir().join(format!("turbodm-test-video-home-{}", std::process::id()));
        // SAFETY: once, before this process's tests start their threads (they all wait here)
        unsafe { std::env::set_var("TURBODM_HOME", home) };
    });
}

fn serve(env: &Env, data: Arc<Vec<u8>>, delay_ms: u64) -> TestServer {
    start(&env.rt, data, Options { ranges: true, no_length: false, max_conns: 0 }, delay_ms)
}

fn part(id: &str, server: &TestServer, ext: &str, size: usize) -> Source {
    Source { format_id: id.into(), url: server.url.clone(), protocol: "http".into(), ext: ext.into(),
             size: Some(size as u64), ..Default::default() }
}

/// What the dialog starts for a quality: the video's page, its parts, where it goes.
fn prefetch_video(manager: &Arc<Manager>, dir: &Path, name: &str, output: Output, sources: Vec<Source>) -> String {
    let job = VideoJob { format: "v+a/b".into(), quality: "720p MKV".into(), title: "clip".into(), output,
                         parts: sources.iter().map(|s| (s.format_id.clone(), s.size)).collect(), sources,
                         choice: "720p/mkv".into(), ..Default::default() };
    manager.prefetch(AddRequest { url: PAGE.into(), filename: Some(name.into()), directory: Some(dir.to_path_buf()),
                                  video: Some(job), ..Default::default() })
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).map(|d| d.flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}

fn confirm(manager: &Arc<Manager>, id: &str, dir: &Path, name: &str, start: bool) {
    manager.confirm(id, Confirm { filename: Some(name.into()), directory: dir.to_path_buf(), connections: 4, start });
}

#[test]
fn video_part_moves_while_it_downloads_and_grows() {
    sandbox();
    let env = Env::new("video-early");
    let data = random_bytes(8 << 20);
    let server = serve(&env, data.clone(), 20);
    let manager = env.manager(4);
    let early_dir = env.dir.join("early");
    let id = prefetch_video(&manager, &early_dir, "clip.mp4", Output::Single, vec![part("18", &server, "mp4", data.len())]);
    let early = wait_for(&manager, &id, |s| s.downloaded > 0);
    assert_eq!(early.segments.len(), 1, "the early download uses one connection");
    assert!(manager.snapshot().is_empty(), "listed before it was confirmed");
    assert_eq!(files_in(&early_dir), ["clip.f18.mp4.tdmpart"]);
    let dir = env.dir.join("chosen");
    confirm(&manager, &id, &dir, "my clip.mp4", true);
    wait_for(&manager, &id, |s| s.segments.len() >= 4 || finished(s)); // grew, nothing reconnected
    let snap = wait_for(&manager, &id, finished);
    assert_eq!((snap.status, snap.error.as_deref()), (Status::Completed, None));
    assert_eq!(snap.path, dir.join("my clip.mp4"));
    assert!(std::fs::read(&snap.path).unwrap() == *data, "the video isn't what was sent");
    assert_eq!(files_in(&dir), ["my clip.mp4"]);
    assert!(files_in(&early_dir).is_empty(), "left behind: {:?}", files_in(&early_dir));
    assert_eq!(manager.snapshot().len(), 1);
}

#[test]
fn video_finished_before_its_start_still_moves() {
    sandbox();
    let env = Env::new("video-early-done");
    let data = random_bytes(256 << 10);
    let server = serve(&env, data.clone(), 0);
    let manager = env.manager(4);
    let id = prefetch_video(&manager, &env.dir.join("early"), "clip.mp4", Output::Single,
                            vec![part("18", &server, "mp4", data.len())]);
    let early = wait_for(&manager, &id, finished);
    assert_eq!(early.status, Status::Completed, "{:?}", early.error);
    let dir = env.dir.join("chosen");
    confirm(&manager, &id, &dir, "my clip.mp4", true);
    let snap = wait_for(&manager, &id, |s| s.path == dir.join("my clip.mp4"));
    assert_eq!(snap.status, Status::Completed);
    assert!(std::fs::read(&snap.path).unwrap() == *data);
    assert!(!early.path.exists(), "left behind where it was downloaded first");
}

#[test]
fn video_put_off_keeps_its_part_where_it_goes() {
    sandbox();
    let env = Env::new("video-later");
    let data = random_bytes(8 << 20);
    let server = serve(&env, data.clone(), 40);
    let manager = env.manager(4);
    let id = prefetch_video(&manager, &env.dir, "clip.mp4", Output::Single, vec![part("18", &server, "mp4", data.len())]);
    wait_for(&manager, &id, |s| s.downloaded > 0);
    let dir = env.dir.join("later");
    confirm(&manager, &id, &dir, "clip.mp4", false); // "Download later"
    let snap = wait_for(&manager, &id, |s| s.status == Status::Paused && s.directory == dir);
    assert!(snap.downloaded > 0 && snap.downloaded < data.len() as u64);
    assert_eq!(files_in(&dir), ["clip.f18.mp4.tdmpart"]);
    manager.resume(&id);
    let snap = wait_for(&manager, &id, finished);
    assert_eq!(snap.status, Status::Completed, "{:?}", snap.error);
    assert!(std::fs::read(dir.join("clip.mp4")).unwrap() == *data);
}

#[test]
fn video_cancelled_leaves_nothing() {
    sandbox();
    let env = Env::new("video-cancel");
    let data = random_bytes(8 << 20);
    let server = serve(&env, data.clone(), 40);
    let manager = env.manager(4);
    let dir = env.dir.join("early");
    let id = prefetch_video(&manager, &dir, "clip.mp4", Output::Single, vec![part("18", &server, "mp4", data.len())]);
    wait_for(&manager, &id, |s| s.downloaded > 0);
    manager.remove(&id, true);
    let deadline = Instant::now() + Duration::from_secs(15);
    while manager.get(&id).is_some() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(files_in(&dir).is_empty(), "left behind: {:?}", files_in(&dir));
}

/// Two seconds of a test picture (noisy: 2.7 MB, so it takes a while) and of a tone, by FFmpeg.
fn media(ffmpeg: &Path, dir: &Path) -> (Vec<u8>, Vec<u8>) {
    let make = |input: &str, codec: &[&str], file: &str| -> Vec<u8> {
        let path: PathBuf = dir.join(file);
        let status = std::process::Command::new(ffmpeg).args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", input])
            .args(codec).arg(&path).status().unwrap();
        assert!(status.success(), "FFmpeg couldn't make {file}");
        std::fs::read(&path).unwrap()
    };
    std::fs::create_dir_all(dir).unwrap();
    (make("testsrc=duration=2:size=320x240:rate=25,noise=alls=60:allf=t", &["-c:v", "mpeg4", "-q:v", "2"], "v.mp4"),
     make("sine=frequency=440:duration=2", &["-c:a", "aac"], "a.m4a"))
}

#[test]
fn video_in_two_parts_moves_and_is_joined() {
    sandbox();
    let env = Env::new("video-two-parts");
    let manager = env.manager(4);
    let Some(ffmpeg) = tools::ffmpeg_exe() else {
        // no FFmpeg (it isn't on PATH): what's made of two parts says it needs it, at once
        let blank = serve(&env, random_bytes(1 << 10), 0);
        let sources = vec![part("v", &blank, "mp4", 1 << 10), part("a", &blank, "m4a", 1 << 10)];
        let id = prefetch_video(&manager, &env.dir, "clip.mkv", Output::Mkv, sources);
        let snap = wait_for(&manager, &id, finished);
        assert!(snap.error.as_deref().is_some_and(|e| e.contains("FFmpeg")), "{:?}", snap.error);
        eprintln!("(no FFmpeg on PATH: joining not tried)");
        return;
    };
    let (video, sound) = media(&ffmpeg, &env.dir.join("made"));
    let (video, sound) = (Arc::new(video), Arc::new(sound));
    let (v, a) = (serve(&env, video.clone(), 20), serve(&env, sound.clone(), 20));
    let early_dir = env.dir.join("early");
    let sources = vec![part("v", &v, "mp4", video.len()), part("a", &a, "m4a", sound.len())];
    let id = prefetch_video(&manager, &early_dir, "clip.mkv", Output::Mkv, sources);
    let early = wait_for(&manager, &id, |s| s.downloaded > 0);
    assert_eq!(early.segments.len(), 1, "the early download uses one connection");
    assert_eq!(files_in(&early_dir), ["clip.fv.mp4.tdmpart"]); // its video, under way
    let dir = env.dir.join("chosen");
    confirm(&manager, &id, &dir, "two parts.mkv", true);
    wait_for(&manager, &id, |s| s.segments.len() >= 4 || finished(s)); // grew, nothing reconnected
    let snap = wait_for(&manager, &id, finished);
    assert_eq!((snap.status, snap.error.as_deref()), (Status::Completed, None));
    assert_eq!(snap.path, dir.join("two parts.mkv"));
    let made = std::fs::read(&snap.path).unwrap();
    assert!(made.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]), "not a Matroska file");
    assert_eq!(files_in(&dir), ["two parts.mkv"], "its parts are left beside it");
    assert!(files_in(&early_dir).is_empty(), "left behind: {:?}", files_in(&early_dir));
}
