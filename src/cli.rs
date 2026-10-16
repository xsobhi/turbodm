//! Headless mode: `turbodm get URL [-c CONNECTIONS] [-d DIR]` (a link, a magnet link or a
//! .torrent file), and `turbodm video URL` for a video on a web page.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use turbodm::config::Settings;
use turbodm::engine::http::Headers;
use turbodm::engine::video::{self, tools, Container, VideoJob};
use turbodm::engine::{AddRequest, Manager, Snapshot, Status};
use turbodm::util::{human_eta, human_size, human_speed};

pub fn get(args: &[String]) -> i32 {
    let mut settings = Settings::load();
    let mut url = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-c" | "--connections" => {
                settings.connections = iter.next().and_then(|n| n.parse().ok()).unwrap_or(32)
            }
            "-d" | "--dir" => settings.download_dir = iter.next().map(PathBuf::from).unwrap_or_default(),
            other if !other.starts_with('-') => url = Some(other.to_string()),
            other => eprintln!("ignoring unknown option {other}"),
        }
    }
    let Some(url) = url else {
        eprintln!("usage: turbodm get URL|MAGNET|FILE.torrent [-c CONNECTIONS] [-d DIR]");
        return 2;
    };
    settings.use_categories = false;
    let settings = settings.clamp();
    let connections = settings.connections;
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    // its own list (and torrent session) for this run only
    let home = std::env::temp_dir().join(format!("turbodm-cli-{}", std::process::id()));
    let (manager, _events) = Manager::new(settings, home.join("downloads.json"), rt.handle().clone());
    let id = manager.add(AddRequest { url, start: true, ..Default::default() });
    let started = Instant::now();
    let snap = follow(&manager, &id);
    let _ = std::fs::remove_dir_all(home);
    let secs = started.elapsed().as_secs_f64();
    if snap.status == Status::Completed {
        let avg = snap.downloaded as f64 / secs;
        println!("Done: {} in {secs:.1}s, average {} with {connections} connections",
                 snap.path.display(), human_speed(avg));
        0
    } else {
        println!("Failed: {}", snap.error.unwrap_or_default());
        1
    }
}

/// Show a download's progress on one line until it ends; then stop the engine.
fn follow(manager: &Manager, id: &str) -> Snapshot {
    let snap = loop {
        std::thread::sleep(Duration::from_millis(500));
        let snap = manager.get(id).expect("task");
        let pct = snap.progress.map_or(String::new(), |p| format!("{:5.1}%", p * 100.0));
        let doing = snap.activity.map(|a| format!("  {a}")).unwrap_or_default();
        print!("\r{pct} {} / {}  {}  {} conns  {}{doing}          ",
               human_size(Some(snap.downloaded)), human_size(snap.size), human_speed(snap.speed),
               snap.active_connections, human_eta(snap.eta));
        let _ = std::io::stdout().flush();
        if matches!(snap.status, Status::Completed | Status::Error) {
            break snap;
        }
    };
    manager.shutdown();
    println!();
    snap
}

/// `turbodm video URL [-q QUALITY] [-f FORMAT] [-c N] [-d DIR] [--list] [--ffmpeg]`: what a page's
/// video comes in (--list), or download it: -q 720p or MP3, M4A, Opus (else the usual), -f MP4,
/// WebM or MKV, on N connections. --ffmpeg: fetch FFmpeg when it's needed (Windows).
pub fn video(args: &[String]) -> i32 {
    let mut settings = Settings::load();
    let (mut url, mut quality, mut format, mut list, mut get_ffmpeg) = (None, String::new(), String::new(), false, false);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-q" | "--quality" => quality = iter.next().cloned().unwrap_or_default(),
            "-f" | "--format" => format = iter.next().cloned().unwrap_or_default(),
            "-c" | "--connections" => settings.connections = iter.next().and_then(|n| n.parse().ok()).unwrap_or(8),
            "-d" | "--dir" => settings.download_dir = iter.next().map(PathBuf::from).unwrap_or_default(),
            "--list" => list = true,
            "--ffmpeg" => get_ffmpeg = true,
            other if !other.starts_with('-') => url = Some(other.to_string()),
            other => eprintln!("ignoring unknown option {other}"),
        }
    }
    let Some(url) = url else {
        eprintln!("usage: turbodm video URL [-q QUALITY] [-f FORMAT] [-c N] [-d DIR] [--list] [--ffmpeg]");
        return 2;
    };
    settings.use_categories = false;
    let settings = settings.clamp();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    let client = turbodm::engine::http::build_client(Duration::from_secs(settings.timeout_secs));
    println!("Looking at the page…");
    let found = match rt.block_on(video::probe(&client, &url, &Headers::default(), settings.timeout_secs)) {
        Ok(found) => found,
        Err(err) => {
            println!("Failed: {err}");
            return 1;
        }
    };
    let ffmpeg = tools::ffmpeg().is_some();
    let container = Container::ALL.into_iter().find(|c| c.name().eq_ignore_ascii_case(&format)).unwrap_or(Container::Mp4);
    let formats: Vec<&str> = found.containers().iter().map(|c| c.name()).collect();
    println!("{}  (formats: {})", found.title, formats.join(", "));
    for plan in found.plans(container) {
        let needs = if plan.needs_ffmpeg() && !ffmpeg { "  (needs FFmpeg)" } else { "" };
        println!("  {}{needs}", plan.summary());
    }
    if list {
        return 0;
    }
    let plan = found.first(&quality, container.name(), ffmpeg);
    if plan.needs_ffmpeg() && !ffmpeg {
        if !(get_ffmpeg && tools::CAN_FETCH_FFMPEG) {
            println!("{} needs FFmpeg: {}", plan.label,
                     if tools::CAN_FETCH_FFMPEG { "add --ffmpeg to fetch it" } else { tools::NO_FFMPEG });
            return 1;
        }
        println!("Getting FFmpeg…");
        if let Err(err) = rt.block_on(tools::fetch_ffmpeg(&client, &Default::default())) {
            println!("Failed: {err}");
            return 1;
        }
    }
    println!("Downloading {} on {} connections", plan.label, settings.connections);
    let store = std::env::temp_dir().join(format!("turbodm-cli-{}.json", std::process::id()));
    let (manager, _events) = Manager::new(settings.clone(), store.clone(), rt.handle().clone());
    let name = video::filename(&found.title, &plan.ext);
    let started = Instant::now();
    let id = manager.add(AddRequest { url: found.url.clone(), filename: Some(name), start: true,
                                      directory: Some(settings.download_dir.clone()),
                                      video: Some(VideoJob::new(&found, &plan)), ..Default::default() });
    let snap = follow(&manager, &id);
    let _ = std::fs::remove_file(store);
    let secs = started.elapsed().as_secs_f64();
    if snap.status == Status::Completed {
        println!("Done: {} ({}) in {secs:.1}s, average {}", snap.path.display(), human_size(snap.size),
                 human_speed(snap.size.unwrap_or(0) as f64 / secs));
        0
    } else {
        println!("Failed: {}", snap.error.unwrap_or_default());
        1
    }
}
