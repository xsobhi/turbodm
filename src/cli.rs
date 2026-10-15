//! Headless mode: `turbodm get URL [-c CONNECTIONS] [-d DIR]`, and `turbodm video URL` for a
//! video on a web page.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use turbodm::config::Settings;
use turbodm::engine::http::Headers;
use turbodm::engine::video::{self, tools, VideoJob};
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
        eprintln!("usage: turbodm get URL [-c CONNECTIONS] [-d DIR]");
        return 2;
    };
    settings.use_categories = false;
    let settings = settings.clamp();
    let connections = settings.connections;
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    let store = std::env::temp_dir().join(format!("turbodm-cli-{}.json", std::process::id()));
    let (manager, _events) = Manager::new(settings, store.clone(), rt.handle().clone());
    let id = manager.add(AddRequest { url, start: true, ..Default::default() });
    let started = Instant::now();
    let snap = follow(&manager, &id);
    let _ = std::fs::remove_file(store);
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

/// `turbodm video URL [-q QUALITY] [-d DIR] [--list] [--ffmpeg]`: the qualities a page's video
/// comes in (--list), or download one (-q 720p, -q Audio; else the usual one). --ffmpeg: fetch
/// FFmpeg when the quality needs it (Windows).
pub fn video(args: &[String]) -> i32 {
    let mut settings = Settings::load();
    let (mut url, mut quality, mut list, mut get_ffmpeg) = (None, String::new(), false, false);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-q" | "--quality" => quality = iter.next().cloned().unwrap_or_default(),
            "-d" | "--dir" => settings.download_dir = iter.next().map(PathBuf::from).unwrap_or_default(),
            "--list" => list = true,
            "--ffmpeg" => get_ffmpeg = true,
            other if !other.starts_with('-') => url = Some(other.to_string()),
            other => eprintln!("ignoring unknown option {other}"),
        }
    }
    let Some(url) = url else {
        eprintln!("usage: turbodm video URL [-q QUALITY] [-d DIR] [--list] [--ffmpeg]");
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
    println!("{}", found.title);
    for (i, choice) in found.choices.iter().enumerate() {
        let needs = if choice.merge && !ffmpeg { "  (needs FFmpeg)" } else { "" };
        println!("  {}. {}{needs}", i + 1, choice.summary());
    }
    if list {
        return 0;
    }
    let index = found.choice_like(&quality).unwrap_or_else(|| found.default_choice(ffmpeg));
    let choice = &found.choices[index];
    if choice.merge && !ffmpeg {
        if !(get_ffmpeg && tools::CAN_FETCH_FFMPEG) {
            println!("{} needs FFmpeg to join its video and sound: {}", choice.label,
                     if tools::CAN_FETCH_FFMPEG { "add --ffmpeg to fetch it" } else { tools::NO_FFMPEG });
            return 1;
        }
        let size = rt.block_on(tools::ffmpeg_size(&client));
        println!("Getting FFmpeg ({})…", human_size(size));
        if let Err(err) = rt.block_on(tools::fetch_ffmpeg(&client, &Default::default())) {
            println!("Failed: {err}");
            return 1;
        }
    }
    println!("Downloading {}", choice.label);
    let store = std::env::temp_dir().join(format!("turbodm-cli-{}.json", std::process::id()));
    let (manager, _events) = Manager::new(settings.clone(), store.clone(), rt.handle().clone());
    let name = video::filename(&found.title, &choice.ext);
    let id = manager.add(AddRequest { url: found.url.clone(), filename: Some(name), start: true,
                                      directory: Some(settings.download_dir.clone()),
                                      video: Some(VideoJob::new(&found, choice)), ..Default::default() });
    let snap = follow(&manager, &id);
    let _ = std::fs::remove_file(store);
    if snap.status == Status::Completed {
        println!("Done: {} ({})", snap.path.display(), human_size(snap.size));
        0
    } else {
        println!("Failed: {}", snap.error.unwrap_or_default());
        1
    }
}
