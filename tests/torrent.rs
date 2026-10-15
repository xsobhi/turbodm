//! Torrents end to end, without the internet: a librqbit session on this computer shares the
//! files through a tiny tracker, also here, and TurboDM downloads them. TurboDM's session is
//! passive (`TURBODM_TORRENT_PASSIVE`: no DHT, nothing listening), as the seeder only listens on
//! 127.0.0.1, so no firewall is asked anything.
//!
//! One more, ignored by default, uses the internet: `cargo test --release --test torrent --
//! --ignored --nocapture`.

mod common;

use common::harness::*;
use common::random_bytes;
use librqbit::limits::LimitsConfig;
use librqbit::{CreateTorrentOptions, ListenerOptions, Session, SessionOptions};
use sha2::{Digest, Sha256};
use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use turbodm::config::Settings;
use turbodm::engine::manager::Confirm;
use turbodm::engine::{AddRequest, Manager, Snapshot, Status};
use turbodm::text::{peers_text, status_line};
use turbodm::util::{human_size, human_speed};

/// Before anything runs: every test calls it first.
fn passive() {
    static ONCE: Once = Once::new();
    // SAFETY: each test waits here before starting anything, so no other thread is reading the
    // environment meanwhile
    ONCE.call_once(|| unsafe { std::env::set_var("TURBODM_TORRENT_PASSIVE", "1") });
}

struct Seeder {
    session: Arc<Session>,
    torrent_file: PathBuf,
    magnet: String,
}

/// Share `files` (one: a single-file torrent "name"; more: a folder "name" of them), uploading
/// at most `upload_kib` KiB/s (0: no limit), announced to a tracker that knows only the seeder.
fn seed(env: &Env, name: &str, files: &[(&str, &[u8])], upload_kib: u32) -> Seeder {
    let source = env.dir.join("seeder");
    let root = source.join(name);
    if let [(_, data)] = files {
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(&root, data).unwrap();
    } else {
        for (file, data) in files {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), data).unwrap();
        }
    }
    env.rt.block_on(async {
        let tracker = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let announce = format!("http://{}/announce", tracker.local_addr().unwrap());
        let session = Session::new_with_opts(source.clone(), SessionOptions {
            dht: None,
            listen: Some(ListenerOptions { listen_addr: (Ipv4Addr::LOCALHOST, 0).into(), ipv4_only: true,
                                           ..Default::default() }),
            disable_local_service_discovery: true,
            ratelimits: LimitsConfig { upload_bps: NonZeroU32::new(upload_kib * 1024), download_bps: None },
            ..Default::default()
        }).await.unwrap();
        let peer = session.listen_addr().unwrap();
        tokio::spawn(serve_tracker(tracker, peer));
        let options = CreateTorrentOptions { name: None, trackers: vec![announce], piece_length: Some(32 << 10) };
        let (torrent, handle) = session.create_and_serve_torrent(&root, options).await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !handle.stats().finished {
            assert!(Instant::now() < deadline, "the seeder never got ready");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let torrent_file = env.dir.join(format!("{name}.torrent"));
        std::fs::write(&torrent_file, torrent.as_bytes().unwrap()).unwrap();
        Seeder { session, torrent_file, magnet: torrent.as_magnet().to_string() }
    })
}

/// An HTTP tracker that answers every announce with the seeder's address.
async fn serve_tracker(listener: TcpListener, peer: SocketAddr) {
    let SocketAddr::V4(peer) = peer else { panic!("the seeder listens on IPv4") };
    let mut body = b"d8:intervali5e5:peers6:".to_vec();
    body.extend_from_slice(&peer.ip().octets());
    body.extend_from_slice(&peer.port().to_be_bytes());
    body.push(b'e');
    while let Ok((mut stream, _)) = listener.accept().await {
        let body = body.clone();
        tokio::spawn(async move {
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request).await;
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(&[head.as_bytes(), &body].concat()).await;
        });
    }
}

fn same(path: &Path, data: &[u8]) -> bool {
    std::fs::read(path).is_ok_and(|written| Sha256::digest(written) == Sha256::digest(data))
}

fn add_url(manager: &Arc<Manager>, url: &str) -> String {
    manager.add(AddRequest { url: url.into(), start: true, ..Default::default() })
}

fn gone(manager: &Manager, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while manager.get(id).is_some() {
        assert!(Instant::now() < deadline, "not removed");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What the seeder has sent (to tell a download that continued from one that started over).
fn uploaded(seeder: &Seeder) -> u64 {
    seeder.session.with_torrents(|all| all.map(|(_, t)| t.stats().uploaded_bytes).sum())
}

#[test]
fn downloads_a_torrent_file_then_removes_it_with_its_file() {
    passive();
    let env = Env::new("torrent-file");
    let data = random_bytes(3 << 20);
    let seeder = seed(&env, "test file.bin", &[("test file.bin", &data)], 0);
    let downloads = env.dir.join("Downloads"); // nothing else in it
    let settings = Settings { download_dir: downloads.clone(), use_categories: false, ..Settings::default() };
    let (manager, _events) = Manager::new(settings, env.dir.join("downloads.json"), env.rt.handle().clone());
    let id = add_url(&manager, &seeder.torrent_file.display().to_string());
    let snap = wait_for(&manager, &id, finished);
    assert_file(&snap, &data);
    assert_eq!(snap.path, downloads.join("test file.bin"));
    let torrent = snap.torrent.expect("a torrent");
    assert_eq!((torrent.files.len(), torrent.seeding), (1, false));
    manager.remove(&id, true);
    gone(&manager, &id);
    assert!(!snap.path.exists(), "its file is still there");
    assert!(downloads.is_dir(), "the download folder went with it");
    manager.shutdown();
}

#[test]
fn magnet_paused_continues_after_a_restart() {
    passive();
    let env = Env::new("torrent-resume");
    let data = random_bytes(6 << 20);
    let seeder = seed(&env, "test file.bin", &[("test file.bin", &data)], 1024);
    let manager = env.manager(4);
    let id = add_url(&manager, &seeder.magnet);
    let live = wait_for(&manager, &id, |s| s.downloaded as f64 > data.len() as f64 * 0.3);
    assert_eq!((live.status, live.filename.as_str()), (Status::Downloading, "test file.bin"));
    assert!(live.active_connections >= 1, "no peer connected");
    manager.pause(&id);
    let before = wait_for(&manager, &id, |s| s.status == Status::Paused).downloaded;
    manager.shutdown(); // closing the app
    // not dropped: in the app the runtime is gone by then, so nothing librqbit does on drop
    // (such as writing down which pieces it has) gets to run
    std::mem::forget(manager);

    let sent = uploaded(&seeder);
    let manager = env.manager(4);
    let snap = manager.get(&id).expect("download restored from disk");
    assert_eq!((snap.status, snap.downloaded), (Status::Paused, before));
    manager.resume(&id);
    let snap = wait_for(&manager, &id, finished);
    assert_file(&snap, &data);
    let more = uploaded(&seeder) - sent;
    assert!(more < data.len() as u64 - before / 2, "started over: {more} bytes more for {} left",
            data.len() as u64 - before);
    manager.shutdown();
}

#[test]
fn a_folder_of_files_goes_where_the_add_dialog_says() {
    passive();
    let env = Env::new("torrent-folder");
    let (a, b) = (random_bytes(1 << 20), random_bytes(300 << 10));
    let seeder = seed(&env, "Album", &[("one.bin", &a), ("disc 2/two.bin", &b)], 0);
    let manager = env.manager(4);
    // the dialog looks the torrent up first, unlisted
    let id = manager.prefetch(AddRequest { url: seeder.magnet.clone(), ..Default::default() });
    let early = wait_for(&manager, &id, |s| s.size.is_some() || finished(s));
    assert_eq!((early.filename.as_str(), early.size), ("Album", Some((a.len() + b.len()) as u64)), "{:?}", early.error);
    assert_eq!(early.torrent.as_ref().map(|t| t.peers), Some(0), "downloading before it's confirmed");
    assert!(manager.snapshot().is_empty(), "listed before it was confirmed");
    let chosen = env.dir.join("chosen");
    manager.confirm(&id, Confirm { filename: Some("My album".into()), directory: chosen.clone(), connections: 4,
                                   start: true });
    let snap = wait_for(&manager, &id, finished);
    assert_eq!(snap.status, Status::Completed, "{:?}", snap.error);
    assert_eq!(snap.path, chosen.join("My album"));
    assert!(same(&chosen.join("My album/one.bin"), &a) && same(&chosen.join("My album/disc 2/two.bin"), &b));
    assert_eq!(snap.torrent.map(|t| t.files.len()), Some(2));

    // the same torrent again: refused, not mixed with the first
    let again = add_url(&manager, &seeder.torrent_file.display().to_string());
    let refused = wait_for(&manager, &again, finished);
    assert_eq!(refused.status, Status::Error);
    assert!(refused.error.unwrap_or_default().contains("already"));

    // removed with its files: their folder goes, the one chosen for it stays
    manager.remove(&again, true);
    manager.remove(&id, true);
    gone(&manager, &id);
    assert!(!chosen.join("My album").exists() && chosen.is_dir());
    manager.shutdown();
}

/// Debian's installer: legal, well shared, and its tracker hands out plenty of peers (Ubuntu's
/// gives a client without DHT, as this test's, only Canonical's own seed). Debian replaces
/// "current" now and then: TURBODM_TEST_TORRENT and TURBODM_TEST_TRACKER give another.
const TORRENT: &str = "https://cdimage.debian.org/debian-cd/current/amd64/bt-cd/debian-13.7.0-amd64-netinst.iso.torrent";
const TRACKER: &str = "http://bttracker.debian.org:6969/announce";

/// A real torrent at 1 MiB/s: looked up, a few MB downloaded, paused, resumed, removed with its
/// file; then the same as a magnet link, looked up from its peers. About 15 MB in all.
#[test]
#[ignore = "uses the internet"]
fn real_torrent_from_the_internet() {
    passive();
    let url = std::env::var("TURBODM_TEST_TORRENT").unwrap_or_else(|_| TORRENT.into());
    let tracker = std::env::var("TURBODM_TEST_TRACKER").unwrap_or_else(|_| TRACKER.into());
    let env = Env::new("torrent-internet");
    let settings = Settings { download_dir: env.dir.clone(), use_categories: false, speed_limit_kib: 1024,
                              ..Settings::default() };
    let (manager, _events) = Manager::new(settings, env.dir.join("downloads.json"), env.rt.handle().clone());
    let start = Instant::now();
    let id = add_url(&manager, &url);
    let found = watch(&manager, &id, start, "details", |s| s.size.is_some());
    let live = watch(&manager, &id, start, "6 MB", |s| s.downloaded >= found.downloaded + (6 << 20));
    assert!(live.speed < 1.5 * 1024.0 * 1024.0, "over the 1 MiB/s limit: {}", human_speed(live.speed));
    manager.pause(&id);
    let paused = watch(&manager, &id, start, "paused", |s| s.status == Status::Paused);
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(manager.get(&id).unwrap().downloaded, paused.downloaded, "it went on while paused");
    manager.resume(&id);
    let more = watch(&manager, &id, start, "2 MB more", |s| s.downloaded >= paused.downloaded + (2 << 20));
    assert!(more.path.exists());

    manager.save();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(env.dir.join("downloads.json")).unwrap()).unwrap();
    let hash = saved[0]["torrent"]["info_hash"].as_str().expect("info hash saved").to_string();
    manager.remove(&id, true);
    gone(&manager, &id);
    assert!(!more.path.exists(), "its file is still there");

    let magnet = format!("magnet:?xt=urn:btih:{hash}&tr={}", tracker.replace(':', "%3A").replace('/', "%2F"));
    let start = Instant::now();
    let id = add_url(&manager, &magnet);
    let found = watch(&manager, &id, start, "details from peers", |s| s.size.is_some());
    assert_eq!(found.size, more.size);
    watch(&manager, &id, start, "1 MB", |s| s.downloaded >= 1 << 20);
    manager.remove(&id, true);
    gone(&manager, &id);
    manager.shutdown();
}

/// Wait (up to 3 minutes) until `done`, saying how it goes every second.
fn watch(manager: &Manager, id: &str, start: Instant, what: &str, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut said = Instant::now();
    loop {
        let s = manager.get(id).unwrap();
        let line = || format!("{:5.1}s  {} / {}  {}  peers: {}  {}", start.elapsed().as_secs_f64(),
                              human_size(Some(s.downloaded)), human_size(s.size), human_speed(s.speed),
                              peers_text(&s), status_line(&s));
        if done(&s) {
            println!("{what}: {}  ({})", line(), s.filename);
            return s;
        }
        assert_ne!(s.status, Status::Error, "{:?}", s.error);
        assert!(Instant::now() < deadline, "no {what}: {}", line());
        if said.elapsed() >= Duration::from_secs(1) {
            println!("  {}", line());
            said = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
