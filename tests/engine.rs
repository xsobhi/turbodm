//! End-to-end engine tests against a local server: `cargo test --release`.

mod common;

use common::harness::*;
use common::{random_bytes, start, Options};
use std::time::{Duration, Instant};
use turbodm::engine::{AddRequest, Status};

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
    // the probe's answer was the whole file: downloaded from it, not asked for again (a server
    // that makes the file when asked would make it twice)
    assert_eq!(server.requests.lock().unwrap().len(), 1, "asked again");
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

#[test]
fn refreshed_address_goes_on_from_where_it_was() {
    // IDM's "Refresh download address": a fresh link (and cookies) from the browser for a paused
    // download, which goes on from where it was, from the new link
    let env = Env::new("refresh");
    let data = random_bytes(16 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 40);
    let manager = env.manager(4);
    let id = add(&manager, &server.url);
    wait_for(&manager, &id, |s| s.downloaded as f64 > data.len() as f64 * 0.3);
    manager.pause(&id);
    wait_for(&manager, &id, |s| s.status == Status::Paused);
    server.delay_ms.store(0, std::sync::atomic::Ordering::Relaxed);
    server.requests.lock().unwrap().clear();
    let fresh = format!("{}?token=new", server.url);
    manager.refresh_from(&id, AddRequest { url: fresh.clone(), cookies: Some("session=new".into()), ..AddRequest::default() });
    let done = wait_for(&manager, &id, finished);
    assert_file(&done, &data);
    assert_eq!(done.url, fresh);
    assert_eq!(manager.request_of(&id).unwrap().cookies.as_deref(), Some("session=new"));
    let starts: Vec<u64> = server.requests.lock().unwrap().iter().flatten()
        .filter(|r| r.as_str() != "bytes=0-0")
        .map(|r| r[6..].split('-').next().unwrap().parse().unwrap())
        .collect();
    assert!(!starts.is_empty() && starts.iter().all(|&s| s > 0), "restarted from zero: {starts:?}");
}

#[test]
fn whole_file_answered_with_200_to_its_range() {
    // a small file (one part) asks for all of it as a range; many servers then answer 200 with
    // the file rather than 206: that is the file, not a server that stopped allowing resume
    let env = Env::new("whole200");
    let data = random_bytes(900 << 10);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 0);
    server.whole_as_200.store(true, std::sync::atomic::Ordering::SeqCst);
    let manager = env.manager(8);
    let id = add(&manager, &server.url);
    assert_file(&wait_for(&manager, &id, finished), &data);
}

#[test]
fn refused_at_first_is_asked_again() {
    // the browser's own request still open when TurboDM's first one arrives: a host allowing one
    // connection per link refuses it (403), and lets it in a moment later
    let env = Env::new("refused");
    let data = random_bytes(2 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 0);
    server.forbidden.store(1, std::sync::atomic::Ordering::SeqCst);
    let manager = env.manager(4);
    let id = add(&manager, &server.url);
    assert_file(&wait_for(&manager, &id, finished), &data);
}

#[test]
fn connection_limit_with_403_retires_extra_connections() {
    let env = Env::new("limit403");
    let data = random_bytes(24 << 20);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 1);
    server.forbid_above.store(2, std::sync::atomic::Ordering::SeqCst);
    let manager = env.manager(8);
    let id = add(&manager, &server.url);
    assert_file(&wait_for(&manager, &id, finished), &data);
}
