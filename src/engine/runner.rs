//! Running one download: probe, preallocate, start the connections, finish.

use super::check::Validators;
use super::segments::SegmentMap;
use super::task::{unix_now, Status, Task};
use super::worker::WorkerCtx;
use super::{pool, resume, retry, space, Shared};
use crate::util::{part_path, sanitize_filename, unique_path};
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Drive a download until it completes, is paused, or fails.
pub async fn run(task: Arc<Task>, shared: Arc<Shared>, cancel: CancellationToken) {
    if task.info.lock().unwrap().video.is_some() {
        return super::video::run(task, shared, cancel).await; // a web page's video (its parts)
    }
    let outcome = match prepare(&task, &shared, &cancel).await {
        Ok(()) if !cancel.is_cancelled() => download(&task, &shared, &cancel).await,
        Ok(()) => Ok(false),
        Err(msg) => Err(msg),
    };
    task.set_joining(None);
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

async fn prepare(task: &Task, shared: &Shared, cancel: &CancellationToken) -> Result<(), String> {
    let (url, headers) = {
        let info = task.info.lock().unwrap();
        (info.url.clone(), info.headers())
    };
    let (client, settings) = (shared.client(), shared.settings());
    let Some(probe) = retry::probe(&client, &url, &headers, settings.retries, cancel).await.map_err(|e| e.to_string())? else {
        return Ok(()); // paused meanwhile
    };
    let min_split = settings.min_split_kib * 1024;
    // going on with what's on the disk: the bytes just before where it goes on must still match
    let saved = task.segments.lock().unwrap().as_ref().map(|map| map.saved());
    let unchanged = match &saved {
        Some(segs) if probe.resumable => {
            let (part, mut h) = (part_path(&task.info.lock().unwrap().path()), headers.clone());
            h.user_agent = probe.user_agent.clone().or(h.user_agent);
            resume::same_bytes(&client, &probe.final_url, &h, &part, segs).await
        }
        _ => true,
    };
    let mut info = task.info.lock().unwrap();
    let mut segments = task.segments.lock().unwrap();
    info.download_url = probe.final_url.clone();
    if probe.user_agent.is_some() {
        info.user_agent = probe.user_agent.clone();
    }
    // (saved by an older TurboDM: only its ETag, maybe a weak one)
    let before = match &info.validators {
        v if *v == Validators::default() => Validators::from_headers(info.etag.as_deref(), None, None),
        v => v.clone(),
    };
    let same_file = probe.size == info.size && !before.changed(&probe.validators) && unchanged;
    let part = part_path(&info.path());
    if segments.is_some() && !(probe.resumable && same_file && part.exists()) {
        *segments = None; // can't continue this file: start from scratch
        let _ = std::fs::remove_file(&part);
    }
    if info.filename.is_none() {
        let name = sanitize_filename(&probe.filename);
        if info.category_base.is_some() {
            info.directory = settings.folder_for(&name);
        }
        let unique = unique_path(&info.directory.join(&name));
        info.filename = unique.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    info.size = probe.size;
    info.etag = probe.etag;
    info.validators = probe.validators;
    info.server_checksum = probe.checksum;
    info.content_type = Some(probe.content_type).filter(|t| !t.is_empty());
    info.resumable = probe.resumable && probe.size.is_some();
    let parts = if info.resumable { info.connections } else { 1 };
    let map = segments.get_or_insert_with(|| Arc::new(SegmentMap::create(info.size, parts, min_split)));
    map.set_min_split(min_split);
    Ok(())
}

/// Open (or create) a download's unfinished file, `size` long; new ones are sparse, so far
/// writes don't fill the gap first.
pub(super) fn open_part(part: &Path, size: Option<u64>) -> Result<File, String> {
    let is_new = !part.exists();
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(part)
        .map_err(|e| format!("Cannot open the file: {e}"))?;
    if is_new {
        super::disk::make_sparse(&file);
    }
    if let (true, Some(size)) = (is_new, size) {
        file.set_len(size).map_err(|e| format!("Cannot reserve disk space: {e}"))?;
    }
    Ok(file)
}

async fn download(task: &Arc<Task>, shared: &Shared, cancel: &CancellationToken) -> Result<bool, String> {
    let (info, file) = {
        // locked while opening: the add dialog may move the file to another name or folder
        let info = task.info.lock().unwrap();
        let segments = task.segments.lock().unwrap().clone().expect("prepared");
        // (its file is sparse: the disk fills as the data arrives, so only what's left counts)
        let left = info.size.map_or(0, |size| size.saturating_sub(segments.total_done()));
        if let Some(why) = space::shortage(&info.directory, left, info.size) {
            return Err(why);
        }
        std::fs::create_dir_all(&info.directory).map_err(|e| format!("Cannot create folder: {e}"))?;
        let file = open_part(&part_path(&info.path()), info.size)?;
        (info.clone(), file)
    };
    let segments = task.segments.lock().unwrap().clone().expect("prepared");
    let settings = shared.settings();
    let count = if info.resumable { info.connections } else { 1 };
    let ctx = Arc::new(WorkerCtx {
        file: Arc::new(file),
        segments: segments.clone(),
        client: shared.client(),
        url: info.download_url.clone(),
        headers: info.headers(),
        size: info.size,
        resumable: info.resumable,
        chunk: 0,
        retries: settings.retries,
        limiters: [shared.limiter.clone(), task.limiter.clone()],
        cancel: cancel.clone(),
        live: std::sync::atomic::AtomicUsize::new(count),
    });
    task.set_status(Status::Downloading);
    if let Err(msg) = pool::run(task, &ctx, count).await {
        task.fail(&msg);
    }
    if cancel.is_cancelled() {
        return Ok(false); // paused, or a connection failed (error already recorded)
    }
    if !segments.all_finished() {
        return Err("Download stopped unexpectedly".into());
    }
    // every part is in: put together into the finished file (the download window says so). Each
    // connection wrote its part straight into place, so what's left is getting all of it onto
    // the disk (Windows keeps what was written in memory a while: gigabytes, from a fast
    // connection), closing the file and giving it its name
    task.set_joining(Some(1.0));
    let file = ctx.file.clone();
    drop(ctx);
    // (a disk that can't be asked, as some network ones, has it written all the same)
    let _ = tokio::task::spawn_blocking(move || file.sync_all()).await;
    verify(task, &segments).await?;
    let mut info = task.info.lock().unwrap(); // its name/folder now, not when it started
    let path = info.path();
    let final_path = if path.exists() { unique_path(&path) } else { path.clone() };
    std::fs::rename(part_path(&path), &final_path).map_err(|e| format!("Cannot rename the file: {e}"))?;
    info.filename = final_path.file_name().map(|n| n.to_string_lossy().into_owned());
    if info.size.is_none() {
        info.size = Some(segments.total_done());
    }
    // done, in the same moment as the rename: whatever looks at it next (the add dialog moving it
    // to the folder chosen there) finds the finished file, never a name that's already gone
    info.status = Status::Completed;
    info.completed = Some(unix_now());
    Ok(true)
}

/// The finished file against its checksum (the user's, else the server's), if it has one. A
/// damaged file is thrown away, so Resume downloads it again.
async fn verify(task: &Arc<Task>, segments: &SegmentMap) -> Result<(), String> {
    let (sum, part, size) = {
        let info = task.info.lock().unwrap();
        let Some(sum) = info.checksum.clone().or_else(|| info.server_checksum.clone()) else { return Ok(()) };
        (sum, part_path(&info.path()), info.size.unwrap_or_else(|| segments.total_done()))
    };
    task.video.set_activity(Some("Checking the file…"));
    task.set_joining(Some(0.0));
    let t = task.clone();
    let checked = tokio::task::spawn_blocking(move || sum.verify(&part, size, |done| t.set_joining(Some(done))))
        .await.unwrap_or_else(|e| Err(format!("Cannot check the file: {e}")));
    task.video.set_activity(None);
    if let Err(why) = &checked && why.contains("damaged") {
        *task.segments.lock().unwrap() = None;
        let _ = std::fs::remove_file(part_path(&task.info.lock().unwrap().path()));
    }
    checked
}
