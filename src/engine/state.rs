//! Saving downloads to disk, and read-only snapshots for the UI.

use super::segments::{Segment, SegmentMap};
use super::task::{Status, Task, TaskInfo};
use super::torrent::{name_hint, TorrentView};
use crate::categories::category_for;
use crate::config::write_json_atomic;
use crate::util::part_path;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
struct Saved {
    id: String,
    #[serde(flatten)]
    info: TaskInfo,
    segments: Option<Vec<Segment>>,
}

pub fn save(path: &Path, tasks: &[Arc<Task>]) -> std::io::Result<()> {
    let saved: Vec<Saved> = tasks
        .iter()
        .map(|task| Saved {
            id: task.id.clone(),
            info: task.info.lock().unwrap().clone(),
            segments: task.segments.lock().unwrap().as_ref().map(|s| s.snapshot()),
        })
        .collect();
    write_json_atomic(path, &saved)
}

pub fn load(path: &Path, auto_resume: bool, min_split: u64) -> Vec<Arc<Task>> {
    let Ok(bytes) = std::fs::read(path) else { return vec![] };
    let Ok(saved) = serde_json::from_slice::<Vec<Saved>>(&bytes) else { return vec![] };
    saved
        .into_iter()
        .map(|mut s| {
            if s.info.status.is_active() {
                // the app was closed mid-download
                s.info.status = if auto_resume { Status::Queued } else { Status::Paused };
            }
            let segments = s.segments.map(|v| SegmentMap::from_segments(v, min_split));
            Task::new(s.id, s.info, segments)
        })
        .collect()
}

/// Delete what a download wrote (a torrent's files are the torrent session's to delete).
pub fn remove_files(task: &Task) {
    let info = task.info.lock().unwrap();
    if info.video.is_some() {
        super::video::remove_files(&info); // yt-dlp's own partial files
        return;
    }
    if info.torrent.is_some() {
        return;
    }
    let _ = std::fs::remove_file(part_path(&info.path()));
    if info.status == Status::Completed {
        let _ = std::fs::remove_file(info.path());
    }
}

/// Plain-data view of a download for the GUI.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub path: PathBuf,
    pub directory: PathBuf,
    pub category: &'static str,
    pub size: Option<u64>,
    pub downloaded: u64,
    pub progress: Option<f64>,
    pub speed: f64,
    pub eta: Option<f64>,
    pub status: Status,
    pub error: Option<String>,
    pub resumable: bool,
    pub connections: usize,
    pub speed_limit_kib: u64,
    pub active_connections: usize,
    pub segments: Vec<Segment>,
    pub added: u64,
    pub completed: Option<u64>,
    pub content_type: Option<String>, // as the server said ("text/html": a web page)
    pub video: Option<String>, // a video from a web page: its quality ("720p MP4")
    pub activity: Option<&'static str>, // what it's doing besides downloading ("Joining video and sound…")
    /// A torrent's own numbers (peers, files…); None for other downloads.
    pub torrent: Option<TorrentView>,
}

pub fn snapshot(task: &Task) -> Snapshot {
    let downloaded = task.downloaded();
    let speed = task.speed();
    let eta = task.eta();
    let torrent = task.is_torrent().then(|| task.torrent.lock().unwrap().clone());
    let (segments, active_connections) = match task.segments.lock().unwrap().clone() {
        Some(map) => (map.snapshot(), map.active_count()),
        None => {
            let parts = task.video.parts(); // a video's parts
            let active = parts.iter().filter(|p| p.active).count();
            (parts, active)
        }
    };
    let info = task.info.lock().unwrap();
    let filename = info.filename.clone().unwrap_or_else(|| match &torrent {
        Some(_) => name_hint(&info.url).unwrap_or_else(|| "(getting the torrent's details…)".into()),
        None => "(connecting…)".into(),
    });
    // a torrent's connections are its peers
    let peers = torrent.as_ref().map(|t| t.peers);
    let progress = match (info.size, info.status) {
        (Some(size), _) if size > 0 => Some(downloaded as f64 / size as f64),
        (_, Status::Completed) => Some(1.0),
        _ => None,
    };
    Snapshot {
        id: task.id.clone(),
        url: info.url.clone(),
        category: category_for(&filename),
        path: info.path(),
        directory: info.directory.clone(),
        filename,
        size: info.size,
        downloaded,
        progress,
        speed,
        eta,
        status: info.status,
        error: info.error.clone(),
        resumable: info.resumable || torrent.is_some(),
        connections: peers.unwrap_or(info.connections),
        speed_limit_kib: info.speed_limit_kib,
        active_connections: peers.unwrap_or(active_connections),
        segments,
        added: info.added,
        completed: info.completed,
        content_type: info.content_type.clone(),
        video: info.video.as_ref().map(|v| v.quality.clone()),
        activity: task.video.activity(),
        torrent,
    }
}
