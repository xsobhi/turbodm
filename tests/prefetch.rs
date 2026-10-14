//! The add dialog's early download (`Manager::prefetch`): it starts on one connection while
//! the dialog is open, and `confirm` moves it to the name and folder chosen there.

mod common;

use common::harness::*;
use common::{random_bytes, start, Options};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use turbodm::engine::manager::Confirm;
use turbodm::engine::Status;

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

#[test]
fn prefetch_finished_before_confirm_still_moves_and_reports() {
    let env = Env::new("prefetch-done");
    let data = random_bytes(256 << 10);
    let server = start(&env.rt, data.clone(), Options { ranges: true, no_length: false, max_conns: 0 }, 0);
    let (manager, events) = env.manager_with_events(4);
    let id = prefetch(&manager, &server.url);
    let early = wait_for(&manager, &id, finished); // all of it arrived while the dialog was open
    assert_eq!(early.status, Status::Completed, "{:?}", early.error);
    assert!(early.path.exists());
    let dir = env.dir.join("chosen");
    manager.confirm(&id, Confirm { filename: Some("test file.bin".into()), directory: dir.clone(),
                                   connections: 4, start: true });
    let snap = wait_for(&manager, &id, |s| s.path == dir.join("test file.bin"));
    assert_file(&snap, &data);
    assert!(!early.path.exists(), "left behind where it was downloaded first");
    // and it's said to be done, once: that's what opens the "Download complete" dialog
    let deadline = Instant::now() + Duration::from_secs(10);
    let done = loop {
        match events.try_recv() {
            Ok(event) if event.id == id => break event,
            Ok(_) => {}
            Err(_) => {
                assert!(Instant::now() < deadline, "no event for the finished download");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    assert_eq!((done.status, done.path), (Status::Completed, dir.join("test file.bin")));
}
