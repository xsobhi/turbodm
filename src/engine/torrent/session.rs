//! The one librqbit session every torrent shares, started when a torrent first needs it, and
//! which download owns which torrent.

use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::limits::LimitsConfig;
use librqbit::{Api, DhtSessionConfig, ListenerOptions, Session, SessionOptions, SessionPersistenceConfig};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The session's record of its torrents, in its folder (librqbit's name for it).
const RECORD: &str = "session.json";

pub struct Torrents {
    dir: PathBuf,
    session: tokio::sync::OnceCell<Arc<Session>>,
    limit: AtomicU64, // download speed for all downloads, bytes/s, 0 = unlimited
    owners: Mutex<HashMap<String, String>>, // info hash -> the download that has it
}

impl Torrents {
    /// Torrents kept in `dir`, downloading at most `limit` bytes/s (0 = unlimited).
    pub fn new(dir: PathBuf, limit: u64) -> Self {
        Torrents { dir, session: Default::default(), limit: AtomicU64::new(limit), owners: Default::default() }
    }

    /// The session, started on first use. The torrents it remembers start paused: each
    /// download's runner resumes its own.
    pub async fn session(&self) -> Result<Arc<Session>, String> {
        self.session.get_or_try_init(|| start(&self.dir, self.limit.load(Ordering::Relaxed))).await.cloned()
    }

    /// The session if it's running.
    pub fn running(&self) -> Option<Arc<Session>> {
        self.session.get().cloned()
    }

    /// The speed limit for all downloads (bytes/s, 0 = unlimited); applies right away.
    pub fn set_speed_limit(&self, bytes_per_sec: u64) {
        self.limit.store(bytes_per_sec, Ordering::Relaxed);
        if let Some(session) = self.running() {
            session.ratelimits.set_download_bps(bps(bytes_per_sec));
        }
    }

    /// Stop every torrent and the session (closing TurboDM).
    pub async fn stop(&self) {
        if let Some(session) = self.running() {
            for hash in session.with_torrents(|all| all.map(|(_, t)| t.info_hash()).collect::<Vec<_>>()) {
                self.save_progress(&session, hash);
            }
            session.stop().await;
        }
    }

    /// Write down which of a torrent's pieces are on disk. The session does it ("fast resume")
    /// only every 16 MB and when a torrent completes, so after a pause, or TurboDM closing, a
    /// restart would fetch up to 16 MB again. The same file, in the same form, as its own.
    pub fn save_progress(&self, session: &Arc<Session>, hash: Id20) {
        let Ok((pieces, _)) = Api::new(session.clone(), None).api_dump_haves(hash.into()) else {
            return; // still checking its files: nothing new to say
        };
        let path = self.dir.join(format!("{}.bitv", hash.as_string()));
        // in place, not replaced: the session keeps it open to write to
        let write = || OpenOptions::new().write(true).create(true).truncate(false).open(&path)?
            .write_all(pieces.as_raw_slice());
        let _ = write();
    }

    /// Note that download `id` has this torrent; false if another download already has it.
    pub fn claim(&self, hash: &str, id: &str) -> bool {
        let mut owners = self.owners.lock().unwrap();
        owners.entry(hash.to_string()).or_insert_with(|| id.to_string()) == id
    }

    /// Download `id` is gone: its torrent may be added again.
    pub fn release(&self, hash: &str, id: &str) {
        let mut owners = self.owners.lock().unwrap();
        if owners.get(hash).is_some_and(|owner| owner == id) {
            owners.remove(hash);
        }
    }
}

fn bps(bytes_per_sec: u64) -> Option<NonZeroU32> {
    NonZeroU32::new(bytes_per_sec.min(u32::MAX as u64) as u32)
}

/// `TURBODM_TORRENT_PASSIVE=1`: no DHT, no local peer discovery and no incoming connections,
/// so nothing listens: torrents find peers through their trackers and connect out. For tests,
/// and for trying a build without Windows Firewall asking about it.
fn passive() -> bool {
    std::env::var_os("TURBODM_TORRENT_PASSIVE").is_some_and(|v| !v.is_empty() && v != "0")
}

fn options(dir: &Path, limit: u64) -> SessionOptions {
    let passive = passive();
    let dht = DhtSessionConfig {
        persistence: Some(DhtPersistenceConfig { config_filename: Some(dir.join("dht.json")), dump_interval: None }),
        ..Default::default()
    };
    SessionOptions {
        dht: (!passive).then_some(dht),
        listen: (!passive).then(ListenerOptions::default), // incoming peers, on a port of the system's choosing
        disable_local_service_discovery: passive,
        persistence: Some(SessionPersistenceConfig::Json { folder: Some(dir.to_path_buf()) }),
        fastresume: true,
        ratelimits: LimitsConfig { download_bps: bps(limit), upload_bps: None },
        ..Default::default()
    }
}

async fn start(dir: &Path, limit: u64) -> Result<Arc<Session>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create the torrents folder: {e}"))?;
    let session = match Session::new_with_opts(dir.to_path_buf(), options(dir, limit)).await {
        Ok(session) => session,
        // a damaged record mustn't stop torrents for good: set it aside and start afresh
        // (the downloads then add their torrents again, checking what's on disk)
        Err(err) if format!("{err:#}").contains("session database") => {
            let _ = std::fs::rename(dir.join(RECORD), dir.join(format!("{RECORD}.damaged")));
            Session::new_with_opts(dir.to_path_buf(), options(dir, limit)).await.map_err(failed)?
        }
        Err(err) => return Err(failed(err)),
    };
    // whatever was running when TurboDM last closed waits for its download to resume it
    let restored: Vec<_> = session.with_torrents(|all| all.map(|(_, handle)| handle.clone()).collect());
    for handle in restored.iter().filter(|h| !h.is_paused()) {
        let _ = session.pause(handle).await;
    }
    Ok(session)
}

fn failed(err: impl std::fmt::Display) -> String {
    format!("Cannot start torrents: {err:#}")
}
