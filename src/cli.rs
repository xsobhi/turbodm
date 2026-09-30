//! Headless mode: `turbodm get URL [-c CONNECTIONS] [-d DIR]`.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use turbodm::config::Settings;
use turbodm::engine::{AddRequest, Manager, Status};
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
    let snap = loop {
        std::thread::sleep(Duration::from_millis(500));
        let snap = manager.get(&id).expect("task");
        let pct = snap.progress.map_or(String::new(), |p| format!("{:5.1}%", p * 100.0));
        print!("\r{pct} {} / {}  {}  {} conns  {}          ",
               human_size(Some(snap.downloaded)), human_size(snap.size), human_speed(snap.speed),
               snap.active_connections, human_eta(snap.eta));
        let _ = std::io::stdout().flush();
        if matches!(snap.status, Status::Completed | Status::Error) {
            break snap;
        }
    };
    manager.shutdown();
    let _ = std::fs::remove_file(store);
    let secs = started.elapsed().as_secs_f64();
    println!();
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
