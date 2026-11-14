//! A download that gets the file it was meant to: checksums, a file changed on the server before
//! resuming, a busy server at the start.

mod common;

use common::harness::*;
use common::{random_bytes, start, Options};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use turbodm::engine::check::Checksum;
use turbodm::engine::{AddRequest, Status};

const RANGES: Options = Options { ranges: true, no_length: false, max_conns: 0 };

fn sha256(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn checks_the_checksum_it_was_given() {
    let env = Env::new("checksum");
    let data = random_bytes(4 << 20);
    let server = start(&env.rt, data.clone(), RANGES, 0);
    let manager = env.manager(4);
    let good = manager.add(AddRequest { url: server.url.clone(), start: true,
                                        checksum: Checksum::parse(&sha256(&data)), ..Default::default() });
    assert_file(&wait_for(&manager, &good, finished), &data);

    let bad = manager.add(AddRequest { url: server.url.clone(), start: true,
                                       checksum: Checksum::parse(&"ab".repeat(32)), ..Default::default() });
    let snap = wait_for(&manager, &bad, finished);
    assert_eq!(snap.status, Status::Error);
    assert!(snap.error.as_deref().unwrap_or("").contains("damaged"), "{:?}", snap.error);
    assert!(!snap.path.exists(), "a damaged file was kept under its name");
}

#[test]
fn checks_the_servers_checksum() {
    let env = Env::new("repr-digest");
    let data = random_bytes(2 << 20);
    let server = start(&env.rt, data.clone(), RANGES, 0);
    // (base64 of the digest, as RFC 9530 has it)
    let digest = Sha256::digest(&*data);
    let b64 = base64(&digest);
    *server.headers.lock().unwrap() = format!("Repr-Digest: sha-256=:{b64}:\r\n");
    let manager = env.manager(4);
    assert_file(&wait_for(&manager, &add(&manager, &server.url), finished), &data);

    // the server's file is another than its checksum says: caught
    *server.headers.lock().unwrap() = format!("Repr-Digest: sha-256=:{}:\r\n", base64(&[7u8; 32]));
    let snap = wait_for(&manager, &add(&manager, &server.url), finished);
    assert_eq!(snap.status, Status::Error, "{:?}", snap.error);
}

#[test]
fn a_file_changed_on_the_server_starts_over() {
    // no ETag, no Last-Modified, the same size: only the bytes before the resume point can tell
    let env = Env::new("changed");
    let (old, new) = (random_bytes(16 << 20), std::sync::Arc::new(random_bytes(16 << 20).iter().map(|b| !b).collect::<Vec<_>>()));
    let server = start(&env.rt, old.clone(), RANGES, 40);
    let manager = env.manager(4);
    let id = add(&manager, &server.url);
    wait_for(&manager, &id, |s| s.downloaded as f64 > old.len() as f64 * 0.3);
    manager.pause(&id);
    wait_for(&manager, &id, |s| s.status == Status::Paused);
    *server.content.lock().unwrap() = new.clone();
    server.delay_ms.store(0, Ordering::Relaxed);
    manager.resume(&id);
    assert_file(&wait_for(&manager, &id, finished), &new); // the new file, whole: not half of each
}

#[test]
fn a_busy_server_at_the_start_is_waited_for() {
    let env = Env::new("busy");
    let data = random_bytes(1 << 20);
    let server = start(&env.rt, data.clone(), RANGES, 0);
    server.busy.store(2, Ordering::SeqCst); // the first two requests: 503, Retry-After: 1
    let manager = env.manager(4);
    let started = std::time::Instant::now();
    assert_file(&wait_for(&manager, &add(&manager, &server.url), finished), &data);
    assert!(started.elapsed().as_secs_f64() >= 2.0, "didn't wait as asked: {:?}", started.elapsed());
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}
