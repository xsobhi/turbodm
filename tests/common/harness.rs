//! What the end-to-end tests share: a temporary folder and runtime, a manager, waiting.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use turbodm::config::Settings;
use turbodm::engine::{AddRequest, Manager, Snapshot, Status};

pub struct Env {
    pub rt: Runtime,
    pub dir: PathBuf,
}

impl Env {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("turbodm-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Env { rt: tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap(), dir }
    }

    pub fn manager(&self, connections: usize) -> Arc<Manager> {
        self.manager_with_events(connections).0
    }

    /// The manager and its events (status changes: what shows the "Download complete" dialog).
    pub fn manager_with_events(&self, connections: usize) -> (Arc<Manager>, async_channel::Receiver<Snapshot>) {
        self.manager_with(connections, |_| {})
    }

    /// The manager with settings of its own (`change`d from the tests' usual ones).
    pub fn manager_with(&self, connections: usize, change: impl FnOnce(&mut Settings))
                        -> (Arc<Manager>, async_channel::Receiver<Snapshot>) {
        let mut settings = Settings {
            download_dir: self.dir.clone(),
            use_categories: false,
            connections,
            min_split_kib: 256,
            retries: 3,
            timeout_secs: 10,
            ..Settings::default()
        };
        change(&mut settings);
        Manager::new(settings, self.dir.join("downloads.json"), self.rt.handle().clone())
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn add(manager: &Arc<Manager>, url: &str) -> String {
    manager.add(AddRequest { url: url.into(), start: true, ..Default::default() })
}

pub fn wait_for(manager: &Manager, id: &str, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
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

pub fn finished(s: &Snapshot) -> bool {
    matches!(s.status, Status::Completed | Status::Error)
}

pub fn assert_file(snap: &Snapshot, data: &[u8]) {
    assert_eq!(snap.status, Status::Completed, "{:?}", snap.error);
    assert_eq!(snap.filename, "test file.bin");
    let written = std::fs::read(&snap.path).unwrap();
    assert_eq!(Sha256::digest(&written), Sha256::digest(data));
    assert!(!Path::new(&format!("{}.tdmpart", snap.path.display())).exists());
}

pub fn prefetch(manager: &Arc<Manager>, url: &str) -> String {
    manager.prefetch(AddRequest { url: url.into(), ..Default::default() })
}
