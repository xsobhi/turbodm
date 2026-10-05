//! End-to-end engine tests against a local server: `cargo test --release`.

mod common;

use common::{random_bytes, start, Options};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use turbodm::config::Settings;
use turbodm::engine::manager::Confirm;
use turbodm::engine::{AddRequest, Manager, Snapshot, Status};

struct Env {
    rt: Runtime,
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("turbodm-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Env { rt: tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap(), dir }
    }

    fn manager(&self, connections: usize) -> Arc<Manager> {
        let settings = Settings {
            download_dir: self.dir.clone(),
            use_categories: false,
            connections,
            min_split_kib: 256,
            retries: 3,
            timeout_secs: 10,
            ..Settings::default()
        };
        Manager::new(settings, self.dir.join("downloads.json"), self.rt.handle().clone()).0
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn add(manager: &Arc<Manager>, url: &str) -> String {
    manager.add(AddRequest { url: url.into(), start: true, ..Default::default() })
}

fn wait_for(manager: &Manager, id: &str, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let snap = manager.get(id).unwrap();
        if done(&snap) {
            return snap;
        }
        assert!(Instant::now() < deadline, "timeout: {:?} {:?}", snap.status, snap.error);
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn finished(s: &Snapshot) -> bool {
    matches!(s.status, Status::Completed | Status::Error)
}

fn assert_file(snap: &Snapshot, data: &[u8]) {
    assert_eq!(snap.status, Status::Completed, "{:?}", snap.error);
    assert_eq!(snap.filename, "test file.bin");
    let written = std::fs::read(&snap.path).unwrap();
    assert_eq!(Sha256::digest(&written), Sha256::digest(data));
    assert!(!Path::new(&format!("{}.tdmpart", snap.path.display())).exists());
}

#[test]
fn downloads_with_32_connections() {
    let env = Env::new("32");
    let data = random_bytes(40 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 0);
    let manager = env.manager(32);
    let id = add(&manager, &server.url);
    assert_file(&wait_for(&manager, &id, finished), &data);
    let ranged = server.requests.lock().unwrap().iter().flatten()
        .filter(|r| r.as_str() != "bytes=0-0").count();
    assert!(ranged >= 32, "only {ranged} ranged requests");
}

#[test]
fn pause_restart_resume_continues() {
    let env = Env::new("resume");
    let data = random_bytes(16 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 40);
    let manager = env.manager(4);
    let id = add(&manager, &server.url);
    let live = wait_for(&manager, &id, |s| s.speed > 0.0 && s.eta.is_some());
    assert_eq!(live.status, Status::Downloading);
    wait_for(&manager, &id, |s| s.downloaded as f64 > data.len() as f64 * 0.3);
    manager.pause(&id);
    let before = wait_for(&manager, &id, |s| s.status == Status::Paused).downloaded;
    manager.shutdown(); // simulate closing the app
    drop(manager);

    server.delay_ms.store(0, std::sync::atomic::Ordering::Relaxed);
    server.requests.lock().unwrap().clear();
    let manager = env.manager(4);
    let snap = manager.get(&id).expect("download restored from disk");
    assert_eq!((snap.status, snap.downloaded), (Status::Paused, before));
    manager.resume(&id);
    assert_file(&wait_for(&manager, &id, finished), &data);
    let starts: Vec<u64> = server.requests.lock().unwrap().iter().flatten()
        .filter(|r| r.as_str() != "bytes=0-0")
        .map(|r| r[6..].split('-').next().unwrap().parse().unwrap())
        .collect();
    assert!(!starts.is_empty() && starts.iter().all(|&s| s > 0), "restarted from zero: {starts:?}");
}

#[test]
fn server_without_ranges_uses_one_connection() {
    let env = Env::new("norange");
    let data = random_bytes(3 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: false, no_length: false, max_conns: 0 }, 0);
    let manager = env.manager(32);
    let id = add(&manager, &server.url);
    let snap = wait_for(&manager, &id, finished);
    assert_file(&snap, &data);
    assert!(!snap.resumable);
}

#[test]
fn unknown_size_stream() {
    let env = Env::new("nolen");
    let data = random_bytes(2 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: false, no_length: true, max_conns: 0 }, 0);
    let manager = env.manager(32);
    let id = add(&manager, &server.url);
    let snap = wait_for(&manager, &id, finished);
    assert_file(&snap, &data);
    assert_eq!(snap.size, Some(data.len() as u64));
}

#[test]
fn server_connection_limit_retires_extra_connections() {
    let env = Env::new("limit");
    let data = random_bytes(24 << 20);
    let opts = Options { ranges: true, no_length: false, max_conns: 2 };
    let server = start(&env.rt, data.clone(), opts, 1);
    let manager = env.manager(8);
    let id = add(&manager, &server.url);
    assert_file(&wait_for(&manager, &id, finished), &data);
}

#[test]
fn saves_progress_periodically_while_downloading() {
    let env = Env::new("autosave");
    let data = random_bytes(16 << 20);
    let server = start(&env.rt, data, Options { ranges: true, no_length: false, max_conns: 0 }, 40);
    let manager = env.manager(4);
    let id = add(&manager, &server.url);
    wait_for(&manager, &id, |s| s.status == Status::Downloading && s.downloaded > 0);
    std::thread::sleep(Duration::from_millis(3000)); // no shutdown(): like a crash or kill
    let saved = std::fs::read_to_string(env.dir.join("downloads.json")).expect("state file written");
    assert!(saved.contains(&id), "download missing from saved state: {saved}");
    assert!(saved.contains("\"done\""), "segment progress not saved");
}

#[test]
fn per_download_speed_limit() {
    let env = Env::new("speedlimit");
    let data = random_bytes(3 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 0);
    let manager = env.manager(4);
    // limit first, then start: a fast machine finishes 3 MiB from localhost in no time
    let id = manager.add(AddRequest { url: server.url.clone(), start: false, ..Default::default() });
    manager.set_speed_limit(&id, 1024); // 1 MiB/s: 3 MiB takes about 3 s
    let started = Instant::now();
    manager.resume(&id);
    let snap = wait_for(&manager, &id, finished);
    assert_file(&snap, &data);
    assert_eq!(snap.speed_limit_kib, 1024);
    assert!(started.elapsed() > Duration::from_secs(2), "not limited: {:?}", started.elapsed());
}

fn prefetch(manager: &Arc<Manager>, url: &str) -> String {
    manager.prefetch(AddRequest { url: url.into(), ..Default::default() })
}

#[test]
fn prefetch_moves_to_the_chosen_name_and_folder() {
    let env = Env::new("prefetch");
    let data = random_bytes(8 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 20);
    let manager = env.manager(4);
    let id = prefetch(&manager, &server.url);
    let early = wait_for(&manager, &id, |s| s.downloaded > 0); // downloading while the dialog is open
    assert_eq!(early.segments.len(), 1, "the early download uses one connection");
    assert!(manager.snapshot().is_empty(), "listed before it was confirmed");
    let dir = env.dir.join("chosen");
    manager.confirm(&id, Confirm { filename: Some("test file.bin".into()), directory: dir.clone(),
                                   connections: 4, start: true });
    wait_for(&manager, &id, |s| s.segments.len() >= 4 || finished(s)); // grew from the early single connection
    let snap = wait_for(&manager, &id, finished);
    assert_eq!(snap.path, dir.join("test file.bin"));
    assert_file(&snap, &data);
    assert_eq!(manager.snapshot().len(), 1);
}

#[test]
fn prefetch_cancelled_leaves_nothing() {
    let env = Env::new("prefetch-cancel");
    let data = random_bytes(8 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 40);
    let manager = env.manager(4);
    let id = prefetch(&manager, &server.url);
    let part = PathBuf::from(format!("{}.tdmpart", wait_for(&manager, &id, |s| s.downloaded > 0).path.display()));
    assert!(part.exists());
    manager.remove(&id, true);
    let deadline = Instant::now() + Duration::from_secs(15);
    while manager.get(&id).is_some() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!part.exists());
}
