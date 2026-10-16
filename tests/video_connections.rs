//! A video's parts on all the connections asked for, as a file: a part smaller than a file
//! would be split for (the sound, 2-3 MB) still on all of them, and an early download that's
//! confirmed spread over all of them at once, however fast its one connection was.

mod common;

use common::harness::*;
use common::random_bytes;
use common::video::*;
use turbodm::engine::manager::Confirm;
use turbodm::engine::video::Output;
use turbodm::engine::Status;

#[test]
fn small_part_uses_every_connection() {
    sandbox();
    let env = Env::new("video-small-part");
    let data = random_bytes(3 << 20); // a file this size has three connections (1 MB each, as usual)
    let server = serve(&env, data.clone(), 30);
    let (manager, _) = env.manager_with(8, |s| s.min_split_kib = 1024);
    let id = manager.add(turbodm::engine::AddRequest {
        start: true,
        ..video(&env.dir, "clip.m4a", Output::Single, vec![part("140", &server, "m4a", data.len())])
    });
    let under_way = wait_for(&manager, &id, |s| s.downloaded > 0 || finished(s));
    assert_eq!(under_way.segments.len(), 8, "{:?}", under_way.segments);
    let snap = wait_for(&manager, &id, finished);
    assert_eq!((snap.status, snap.error.as_deref()), (Status::Completed, None));
    assert!(std::fs::read(env.dir.join("clip.m4a")).unwrap() == *data);
    // and on the wire: eight ranges, from eight connections
    assert_eq!(server.requests.lock().unwrap().iter().filter(|r| r.as_deref().is_some_and(|r| r != "bytes=0-0")).count(), 8);
}

#[test]
fn confirmed_early_download_spreads_over_every_connection() {
    sandbox();
    let env = Env::new("video-spread");
    // one connection gets ~12 MB/s here: on its own speed, what's left would never be worth
    // a split (it'd be done within 2 s); the seven connections added at Start share it anyway
    let data = random_bytes(24 << 20);
    let server = serve(&env, data.clone(), 5);
    let manager = env.manager(8);
    let id = prefetch_video(&manager, &env.dir, "clip.mp4", Output::Single, vec![part("137", &server, "mp4", data.len())]);
    let early = wait_for(&manager, &id, |s| s.downloaded >= 3 << 20 || finished(s));
    assert_eq!(early.segments.len(), 1, "the early download uses one connection");
    manager.confirm(&id, Confirm { filename: Some("clip.mp4".into()), directory: env.dir.clone(), connections: 8, start: true });
    let most = std::cell::Cell::new(0);
    let snap = wait_for(&manager, &id, |s| {
        most.set(most.get().max(s.segments.iter().filter(|seg| seg.active).count()));
        finished(s)
    });
    assert_eq!((snap.status, snap.error.as_deref()), (Status::Completed, None));
    assert_eq!(most.get(), 8, "connections at once after Start");
    assert!(std::fs::read(env.dir.join("clip.mp4")).unwrap() == *data);
}
