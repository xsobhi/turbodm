//! Running one download: probe, preallocate, start the connections, finish.

use super::http;
use super::segments::SegmentMap;
use super::task::{unix_now, Status, Task};
use super::worker::{connection_loop, WorkerCtx};
use super::Shared;
use crate::categories::target_dir;
use crate::util::{part_path, sanitize_filename, unique_path};
use std::fs::OpenOptions;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Drive a download until it completes, is paused, or fails.
pub async fn run(task: Arc<Task>, shared: Arc<Shared>, cancel: CancellationToken) {
    let outcome = match prepare(&task, &shared, &cancel).await {
        Ok(()) if !cancel.is_cancelled() => download(&task, &shared, &cancel).await,
        Ok(()) => Ok(false),
        Err(msg) => Err(msg),
    };
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
    let client = shared.client();
    let probe = tokio::select! {
        r = http::probe(&client, &url, &headers) => r.map_err(|e| e.to_string())?,
        _ = cancel.cancelled() => return Ok(()),
    };
    let settings = shared.settings();
    let min_split = settings.min_split_kib * 1024;
    let mut info = task.info.lock().unwrap();
    let mut segments = task.segments.lock().unwrap();
    info.download_url = probe.final_url.clone();
    if probe.user_agent.is_some() {
        info.user_agent = probe.user_agent.clone();
    }
    let same_file = probe.size == info.size
        && (info.etag.is_none() || probe.etag.is_none() || probe.etag == info.etag);
    let part = part_path(&info.path());
    if segments.is_some() && !(probe.resumable && same_file && part.exists()) {
        *segments = None; // can't continue this file: start from scratch
        let _ = std::fs::remove_file(&part);
    }
    if info.filename.is_none() {
        let name = sanitize_filename(&probe.filename);
        if let Some(base) = info.category_base.clone() {
            info.directory = target_dir(&base, &name, true);
        }
        let unique = unique_path(&info.directory.join(&name));
        info.filename = unique.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    info.size = probe.size;
    info.etag = probe.etag;
    info.resumable = probe.resumable && probe.size.is_some();
    let parts = if info.resumable { info.connections } else { 1 };
    let map = segments.get_or_insert_with(|| Arc::new(SegmentMap::create(info.size, parts, min_split)));
    map.set_min_split(min_split);
    Ok(())
}

async fn download(task: &Arc<Task>, shared: &Shared, cancel: &CancellationToken) -> Result<bool, String> {
    let (path, info) = {
        let info = task.info.lock().unwrap();
        (info.path(), info.clone())
    };
    let part = part_path(&path);
    std::fs::create_dir_all(&info.directory).map_err(|e| format!("Cannot create folder: {e}"))?;
    let is_new = !part.exists();
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&part)
        .map_err(|e| format!("Cannot open the file: {e}"))?;
    if let (true, Some(size)) = (is_new, info.size) {
        file.set_len(size).map_err(|e| format!("Cannot reserve disk space: {e}"))?;
    }
    let segments = task.segments.lock().unwrap().clone().expect("prepared");
    let settings = shared.settings();
    let count = if info.resumable { info.connections } else { 1 };
    let ctx = Arc::new(WorkerCtx {
        file,
        segments: segments.clone(),
        client: shared.client(),
        url: info.download_url.clone(),
        headers: info.headers(),
        resumable: info.resumable,
        retries: settings.retries,
        limiter: shared.limiter.clone(),
        cancel: cancel.clone(),
        live: std::sync::atomic::AtomicUsize::new(count),
    });
    task.set_status(Status::Downloading);
    let workers: Vec<_> = (0..count).map(|_| tokio::spawn(connection_loop(ctx.clone()))).collect();
    for worker in workers {
        match worker.await {
            Ok(Err(msg)) => task.fail(&msg),
            Err(join_err) => task.fail(&format!("internal error: {join_err}")),
            Ok(Ok(())) => {}
        }
    }
    if cancel.is_cancelled() {
        return Ok(false); // paused, or a connection failed (error already recorded)
    }
    if !segments.all_finished() {
        return Err("Download stopped unexpectedly".into());
    }
    let final_path = if path.exists() { unique_path(&path) } else { path };
    std::fs::rename(&part, &final_path).map_err(|e| format!("Cannot rename the file: {e}"))?;
    let mut info = task.info.lock().unwrap();
    info.filename = final_path.file_name().map(|n| n.to_string_lossy().into_owned());
    if info.size.is_none() {
        info.size = Some(segments.total_done());
    }
    Ok(true)
}
