//! Running one torrent download: its details (from the .torrent file, or from peers for a
//! magnet link), then the session downloads it while this keeps the task's numbers up to date,
//! until it's complete, paused or fails. Also removing a torrent, and sharing finished ones.

use super::source::{self, Meta};
use super::{TorrentFile, TorrentView};
use crate::engine::task::{unix_now, Status, Task, TaskInfo};
use crate::engine::Shared;
use crate::util::{sanitize_filename, unique_path};
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::Id20;
use librqbit::{AddTorrent, AddTorrentOptions, ManagedTorrent, Session, TorrentStats, TorrentStatsState};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// A torrent in the session.
type Handle = Arc<ManagedTorrent>;

/// How often the torrent's numbers are read.
const WATCH: Duration = Duration::from_millis(250);
/// Which pieces are on disk is written down every so many of those (30 s).
const SAVE_EVERY: u64 = 120;
/// Files listed in the progress window (a torrent can have many thousands).
pub const MAX_FILES_SHOWN: usize = 1000;

/// Drive a torrent download until it completes, is paused, or fails.
pub async fn run(task: Arc<Task>, shared: Arc<Shared>, cancel: CancellationToken) {
    let outcome = drive(&task, &shared, &cancel).await;
    {
        let mut view = task.torrent.lock().unwrap();
        (view.peers, view.known_peers, view.checking, view.fetched) = (0, 0, false, 0);
    }
    let mut info = task.info.lock().unwrap();
    match outcome {
        Ok(true) => {
            info.status = Status::Completed;
            info.completed = Some(unix_now());
        }
        Ok(false) => info.status = if info.error.is_some() { Status::Error } else { Status::Paused },
        Err(msg) => {
            info.error.get_or_insert(msg);
            info.status = Status::Error;
        }
    }
}

async fn drive(task: &Arc<Task>, shared: &Shared, cancel: &CancellationToken) -> Result<bool, String> {
    let Some(session) = until(cancel, shared.torrents.session()).await? else { return Ok(false) };
    let handle = match known(&session, task) {
        Some(handle) => handle, // the session remembers it (TurboDM was closed meanwhile)
        None => {
            let (url, headers) = {
                let info = task.info.lock().unwrap();
                (info.url.clone(), info.headers())
            };
            let client = shared.client();
            let fetch = source::fetch(&session, &url, &headers, &client);
            let Some(meta) = until(cancel, fetch).await? else { return Ok(false) };
            settle(task, shared, &meta)?;
            if !confirmed(task, cancel).await {
                return Ok(false); // the add dialog said "Download later", or was cancelled
            }
            add(&session, task, &meta).await?
        }
    };
    watch(task, shared, &session, &handle, cancel).await
}

/// `work`, unless the download is paused first (then None).
async fn until<T>(cancel: &CancellationToken, work: impl Future<Output = Result<T, String>>)
                  -> Result<Option<T>, String> {
    tokio::select! {
        result = work => result.map(Some),
        _ = cancel.cancelled() => Ok(None),
    }
}

fn id20(hash: &str) -> Option<TorrentIdOrHash> {
    Id20::from_str(hash).ok().map(TorrentIdOrHash::Hash)
}

/// Where the session writes a torrent's files: their folder, or the folder its one file is in.
fn output_folder(info: &TaskInfo) -> PathBuf {
    match &info.torrent {
        Some(t) if t.multi_file() => info.path(),
        _ => info.directory.clone(),
    }
}

/// The session's copy of this download's torrent, if it was given one already.
fn known(session: &Session, task: &Task) -> Option<Handle> {
    let info = task.info.lock().unwrap();
    let meta = info.torrent.as_ref().filter(|t| t.added)?;
    let handle = session.get(id20(meta.info_hash.as_deref()?)?)?;
    (handle.output_folder() == output_folder(&info)).then_some(handle)
}

/// Take in what the torrent is: its name, size and, if it's new, where it goes.
fn settle(task: &Task, shared: &Shared, meta: &Meta) -> Result<(), String> {
    if !shared.torrents.claim(&meta.info_hash, &task.id) {
        return Err("This torrent is in the download list already".into());
    }
    {
        let mut view = task.torrent.lock().unwrap();
        view.file_count = meta.files.len();
        view.files = Arc::new(meta.files.iter().take(MAX_FILES_SHOWN).cloned().collect());
    }
    let mut info = task.info.lock().unwrap();
    let mut t = info.torrent.take().unwrap_or_default();
    (t.info_hash, t.files) = (Some(meta.info_hash.clone()), meta.files.len());
    (info.size, info.resumable) = (Some(meta.size), true);
    if !t.added {
        if info.category_base.is_some() {
            info.directory = shared.settings().folder_for(&meta.main_file);
        }
        // a folder of files can take the name the user gave; one file keeps the torrent's
        let name = match &info.filename {
            Some(chosen) if t.multi_file() => chosen.clone(),
            _ if t.multi_file() => sanitize_filename(&meta.name),
            _ => meta.name.clone(),
        };
        (info.directory, info.filename) = free_place(&info.directory, &name, t.multi_file());
    }
    info.torrent = Some(t);
    Ok(())
}

/// Where a new torrent goes, never over what's already there: a folder of files gets a number
/// ("name (1)"), as other downloads do; one file (named by the torrent) gets a numbered folder.
fn free_place(dir: &Path, name: &str, multi_file: bool) -> (PathBuf, Option<String>) {
    let wanted = dir.join(name);
    if !wanted.exists() {
        return (dir.to_path_buf(), Some(name.to_string()));
    }
    if multi_file {
        let free = unique_path(&wanted).file_name().map(|n| n.to_string_lossy().into_owned());
        return (dir.to_path_buf(), free);
    }
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).filter(|s| !s.is_empty()).unwrap_or(name);
    let folder = (1..).map(|n| dir.join(format!("{stem} ({n})"))).find(|f| !f.exists()).expect("unbounded");
    (folder, Some(name.to_string()))
}

/// Wait while the add dialog that started it is open; false if it's paused meanwhile.
async fn confirmed(task: &Task, cancel: &CancellationToken) -> bool {
    while task.pending.load(Ordering::Relaxed) {
        tokio::select! {
            _ = task.grow.notified() => {}
            _ = cancel.cancelled() => return false,
        }
    }
    !cancel.is_cancelled()
}

/// Give the torrent to the session, to download into its place.
async fn add(session: &Arc<Session>, task: &Task, meta: &Meta) -> Result<Handle, String> {
    let folder = {
        let mut info = task.info.lock().unwrap();
        let t = info.torrent.clone().unwrap_or_default();
        if !t.added {
            // still free? (the add dialog may have pointed it somewhere else meanwhile)
            let name = match &info.filename {
                Some(chosen) if t.multi_file() => chosen.clone(),
                _ => meta.name.clone(), // one file: named by the torrent
            };
            (info.directory, info.filename) = free_place(&info.directory, &name, t.multi_file());
        }
        output_folder(&info)
    };
    let id = id20(&meta.info_hash).ok_or("Bad torrent info hash")?;
    // an old copy of it the session still has (its removal didn't finish): start afresh
    if session.get(id).is_some() {
        let _ = session.delete(id, false).await;
    }
    let options = AddTorrentOptions {
        output_folder: Some(folder.to_str().ok_or("This folder's name can't be used for torrents")?.to_string()),
        overwrite: true, // its place was free (it can't spoil another file); from here on it's its own
        initial_peers: Some(meta.peers.clone()),
        ..Default::default()
    };
    let added = session.add_torrent(AddTorrent::from_bytes(meta.bytes.clone()), Some(options)).await
        .map_err(|e| format!("Cannot start the torrent: {e:#}"))?;
    let handle = added.into_handle().ok_or("Cannot start the torrent")?;
    if let Some(t) = task.info.lock().unwrap().torrent.as_mut() {
        t.added = true;
    }
    Ok(handle)
}

/// Keep the task's numbers up to date until the torrent is complete (true) or paused (false).
async fn watch(task: &Task, shared: &Shared, session: &Arc<Session>, handle: &Handle,
               cancel: &CancellationToken) -> Result<bool, String> {
    let files = files_of(handle);
    let mut tick = tokio::time::interval(WATCH);
    for ticks in 1u64.. {
        tokio::select! {
            _ = tick.tick() => {}
            _ = cancel.cancelled() => break,
        }
        let stats = handle.stats();
        if stats.finished && !matches!(stats.state, TorrentStatsState::Initializing { .. }) {
            show(task, &stats, &files);
            let seed = shared.settings().seed;
            if !seed {
                pause(shared, session, handle).await; // a download manager: done is done
            }
            task.torrent.lock().unwrap().seeding = seed;
            return Ok(true);
        }
        if ticks % SAVE_EVERY == 0 {
            shared.torrents.save_progress(session, handle.info_hash()); // little to fetch again after a crash
        }
        match stats.state {
            // paused (by the session starting, or a pause asked for while it checked its files),
            // or stopped by an error before this resume (it checks its files and starts again):
            // it should be running
            TorrentStatsState::Paused => {
                let _ = session.unpause(handle).await;
            }
            TorrentStatsState::Error if ticks == 1 => {
                let _ = session.unpause(handle).await;
            }
            TorrentStatsState::Error => return Err(failure(stats.error.as_deref())),
            _ => show(task, &stats, &files),
        }
    }
    pause(shared, session, handle).await;
    Ok(false)
}

/// Pause it for good, noting which pieces are on disk. One paused while it checks its files
/// still starts once the check is done (librqbit's way), so then it's paused again, unless it
/// was resumed meanwhile.
async fn pause(shared: &Shared, session: &Arc<Session>, handle: &Handle) {
    let _ = session.pause(handle).await;
    shared.torrents.save_progress(session, handle.info_hash());
    if matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
        let (session, handle) = (session.clone(), handle.clone());
        tokio::spawn(async move {
            let _ = handle.wait_until_initialized().await;
            if handle.is_paused() && handle.live().is_some() {
                let _ = session.pause(&handle).await;
            }
        });
    }
}

fn failure(error: Option<&str>) -> String {
    let error = error.unwrap_or("unknown error");
    let first = error.lines().next().unwrap_or(error);
    format!("Torrent stopped: {first}")
}

/// The torrent's files, without the filler some have between them: how many, and the first
/// MAX_FILES_SHOWN (with their index in the torrent).
struct Files {
    count: usize,
    shown: Vec<(usize, TorrentFile)>,
}

fn files_of(handle: &Handle) -> Files {
    handle.with_metadata(|meta| {
        let files = || meta.file_infos.iter().enumerate().filter(|(_, file)| !file.attrs.padding);
        let shown = files().take(MAX_FILES_SHOWN)
            .map(|(i, file)| (i, TorrentFile {
                name: file.relative_filename.to_string_lossy().into_owned(),
                offset: file.offset_in_torrent,
                size: file.len,
                have: 0,
            }))
            .collect();
        Files { count: files().count(), shown }
    }).unwrap_or(Files { count: 0, shown: vec![] })
}

/// What the session says about it, into the task.
fn show(task: &Task, stats: &TorrentStats, files: &Files) {
    let live = stats.live.as_ref();
    let peers = live.map(|l| &l.snapshot.peer_stats);
    let checking = matches!(stats.state, TorrentStatsState::Initializing { .. });
    let shown = files.shown.iter()
        .map(|(i, file)| TorrentFile { have: stats.file_progress.get(*i).copied().unwrap_or(0), ..file.clone() })
        .collect();
    *task.torrent.lock().unwrap() = TorrentView {
        peers: peers.map_or(0, |p| p.live as usize),
        known_peers: peers.map_or(0, |p| p.seen as usize),
        uploaded: stats.uploaded_bytes,
        checking,
        seeding: false,
        file_count: files.count,
        files: Arc::new(shown),
        fetched: live.map_or(0, |l| l.snapshot.fetched_bytes),
    };
    let mut info = task.info.lock().unwrap();
    if stats.total_bytes > 0 {
        info.size = Some(stats.total_bytes);
    }
    if let Some(t) = info.torrent.as_mut().filter(|_| !checking) {
        t.have = stats.progress_bytes; // (while checking, that's how far the check got)
    }
    if info.status.is_active() {
        info.status = if live.is_some() { Status::Downloading } else { Status::Connecting };
    }
}

/// Forget a download's torrent: the session stops it and forgets it; with `delete_files`,
/// its files go too (only its own: others in its folder stay).
pub async fn remove(shared: &Shared, task: &Task, delete_files: bool) {
    let (meta, path) = {
        let info = task.info.lock().unwrap();
        (info.torrent.clone(), info.path())
    };
    let Some(meta) = meta else { return };
    let Some(hash) = meta.info_hash.clone() else { return };
    shared.torrents.release(&hash, &task.id);
    if !meta.added {
        return; // the session never had it, and nothing of it is on disk
    }
    // a folder of files: the session deletes them and then their folder, if that's empty. One
    // file is deleted here instead: its folder is the user's (Downloads), which the session
    // would remove too once empty.
    let session_deletes = delete_files && meta.multi_file();
    if let (Ok(session), Some(id)) = (shared.torrents.session().await, id20(&hash)) {
        let _ = session.delete(id, session_deletes).await;
    }
    if delete_files && !meta.multi_file() {
        let _ = std::fs::remove_file(path);
    }
}

/// Keep sharing these finished torrents (`on`, the "seed" setting), or stop.
pub async fn share(shared: &Shared, tasks: Vec<Arc<Task>>, on: bool) {
    let session = if on { shared.torrents.session().await.ok() } else { shared.torrents.running() };
    let Some(session) = session else { return };
    for task in tasks {
        let hash = task.info.lock().unwrap().torrent.as_ref().and_then(|t| t.info_hash.clone());
        let Some(handle) = hash.as_deref().and_then(id20).and_then(|id| session.get(id)) else { continue };
        if on {
            let _ = session.unpause(&handle).await;
        } else {
            pause(shared, &session, &handle).await;
        }
        task.torrent.lock().unwrap().seeding = on;
    }
}
