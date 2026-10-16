//! A video's parts downloaded by TurboDM's own connections, like any file: many at once, the
//! progress live, resumable after a restart. Their addresses come from yt-dlp and expire after
//! hours: when the site turns one down, the page is looked at again for fresh ones (fresh.rs),
//! and what's downloaded is kept if it's the same stream. The add dialog may move a download
//! that's under way (`Manager::confirm`): a part's files are where its name says when used.

use super::fresh::refresh;
use super::names::{part_file, stem};
use super::Source;
use crate::engine::http::{self, FetchError, Headers};
use crate::engine::runner::open_part;
use crate::engine::segments::SegmentMap;
use crate::engine::task::{Status, Task, TaskInfo};
use crate::engine::worker::WorkerCtx;
use crate::engine::{pool, Shared};
use crate::util::part_path;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(super) enum Ending {
    Done,
    Paused,
    Elsewhere(String), // TurboDM can't download it directly (fragments, a stream gone): yt-dlp can
}

enum PartError {
    Expired(String), // the site turned the address down: ask for a fresh one
    Failed(String),
}

/// Download the parts not in yet, one after the other.
pub(super) async fn run(task: &Arc<Task>, shared: &Shared, cancel: &CancellationToken) -> Result<Ending, String> {
    let mut asked = 0; // times fresh addresses were asked for
    loop {
        let sources = task.info.lock().unwrap().video.as_ref().map(|v| v.sources.clone()).unwrap_or_default();
        if sources.is_empty() || sources.iter().any(|s| !s.done && s.url.is_empty()) {
            // a download saved by TurboDM 1.8 (yt-dlp downloaded those): its addresses
            if asked > 0 || !refresh(task, shared, cancel).await? {
                return Ok(Ending::Elsewhere("no direct address".into()));
            }
            asked += 1;
            continue;
        }
        if let Some(s) = sources.iter().find(|s| !s.done && !s.plain()) {
            return Ok(Ending::Elsewhere(format!("format {} comes in fragments ({})", s.format_id, s.protocol)));
        }
        let Some(index) = sources.iter().position(|s| !s.done) else { return Ok(Ending::Done) };
        match part(task, shared, cancel, index).await {
            Ok(true) => {}
            Ok(false) => return Ok(Ending::Paused),
            Err(PartError::Expired(why)) if asked < 2 => {
                if std::env::var_os("TURBODM_DEBUG").is_some() {
                    eprintln!("video: part {index} turned down ({why}): asking for fresh addresses");
                }
                asked += 1;
                if !refresh(task, shared, cancel).await? {
                    return Ok(Ending::Elsewhere(why));
                }
            }
            Err(PartError::Expired(why)) => return Ok(Ending::Elsewhere(why)),
            Err(PartError::Failed(why)) => return Err(why),
        }
        if cancel.is_cancelled() {
            return Ok(Ending::Paused);
        }
    }
}

/// What a part's address wants sent with it.
fn headers_of(source: &Source) -> Headers {
    let mut headers = Headers::default();
    for (name, value) in &source.headers {
        match name.to_ascii_lowercase().as_str() {
            "user-agent" => headers.user_agent = Some(value.clone()),
            "referer" => headers.referrer = Some(value.clone()),
            "cookie" => headers.cookies = Some(value.clone()),
            _ => headers.extra.push((name.clone(), value.clone())),
        }
    }
    headers
}

fn expired(error: &str) -> bool {
    ["HTTP 403", "HTTP 404", "HTTP 410"].iter().any(|code| error.contains(code))
}

/// Where a part goes, as the video is named now (the add dialog may rename it, and move it to
/// another folder, while it downloads: so this is asked locked, and the files used at once).
fn file_of(info: &TaskInfo, source: &Source) -> PathBuf {
    part_file(&info.directory, stem(info.filename.as_deref().unwrap_or("video")), &source.format_id, &source.ext)
}

/// The part's file's size, if it's in.
fn size_of(file: &Path) -> Option<u64> {
    std::fs::metadata(file).ok().filter(|m| m.is_file()).map(|m| m.len())
}

/// Download part `index` into its own file beside the video. False: paused.
async fn part(task: &Arc<Task>, shared: &Shared, cancel: &CancellationToken, index: usize) -> Result<bool, PartError> {
    let (source, connections, got) = {
        let info = task.info.lock().unwrap();
        let source = info.video.as_ref().and_then(|v| v.sources.get(index).cloned()).unwrap_or_default();
        std::fs::create_dir_all(&info.directory).map_err(|e| PartError::Failed(format!("Cannot create folder: {e}")))?;
        let got = size_of(&file_of(&info, &source));
        (source, info.connections, got)
    };
    if let Some(got) = got {
        return Ok(done(task, index, got)); // in already (TurboDM closed just as it finished)
    }
    let (client, settings) = (shared.client(), shared.settings());
    let mut headers = headers_of(&source);
    let probe = tokio::select! {
        r = http::probe(&client, &source.url, &headers) => r,
        _ = cancel.cancelled() => return Ok(false),
    };
    let probe = match probe {
        Ok(probe) => probe,
        Err(FetchError::Http(code @ (403 | 404 | 410), reason)) => return Err(PartError::Expired(format!("HTTP {code} {reason}"))),
        Err(err) => return Err(PartError::Failed(err.to_string())),
    };
    if probe.user_agent.is_some() {
        headers.user_agent = probe.user_agent.clone(); // (it only answered TurboDM's own)
    }
    let size = probe.size;
    let resumable = probe.resumable && size.is_some();
    // what's downloaded of it already (saved with the download) is kept if it's the same stream
    let saved = task.segments.lock().unwrap().clone();
    let same = |map: &SegmentMap| map.snapshot().iter().filter_map(|s| s.end).max() == size;
    let (map, opened) = {
        let mut info = task.info.lock().unwrap();
        let partial = part_path(&file_of(&info, &source));
        let map = match saved.filter(|map| resumable && same(map) && partial.exists()) {
            Some(map) => map,
            None => {
                let _ = std::fs::remove_file(&partial);
                let count = if resumable { connections } else { 1 };
                Arc::new(SegmentMap::create(size, count, settings.min_split_kib * 1024))
            }
        };
        if let Some(job) = info.video.as_mut() {
            job.sources[index].size = size;
            let total = job.sources.iter().map(|s| s.size).sum::<Option<u64>>();
            info.size = total;
        }
        (map, open_part(&partial, size))
    };
    map.set_min_split(settings.min_split_kib * 1024);
    *task.segments.lock().unwrap() = Some(map.clone());
    let count = if resumable { connections } else { 1 };
    let ctx = Arc::new(WorkerCtx {
        file: Arc::new(opened.map_err(PartError::Failed)?),
        segments: map.clone(),
        client,
        url: probe.final_url,
        headers,
        resumable,
        chunk: source.chunk,
        retries: settings.retries,
        limiters: [shared.limiter.clone(), task.limiter.clone()],
        cancel: cancel.child_token(), // a part's failure stops its connections, not the download
        live: AtomicUsize::new(count),
    });
    task.set_status(Status::Downloading);
    let result = pool::run(task, &ctx, count).await;
    drop(ctx); // (the file, closed)
    if cancel.is_cancelled() {
        return Ok(false);
    }
    if let Err(why) = result {
        return Err(if expired(&why) { PartError::Expired(why) } else { PartError::Failed(why) });
    }
    if !map.all_finished() {
        return Err(PartError::Failed("Download stopped unexpectedly".into()));
    }
    let got = {
        let info = task.info.lock().unwrap();
        let file = file_of(&info, &source);
        std::fs::rename(part_path(&file), &file).map_err(|e| PartError::Failed(format!("Cannot rename the file: {e}")))?;
        size_of(&file).unwrap_or(0)
    };
    Ok(done(task, index, got))
}

/// Part `index` is in (`got` bytes): counted once, its connections no longer shown.
fn done(task: &Task, index: usize, got: u64) -> bool {
    *task.segments.lock().unwrap() = None; // (before it's counted as done: never twice)
    if let Some(job) = task.info.lock().unwrap().video.as_mut() {
        job.sources[index].done = true;
        job.sources[index].size = Some(got);
        job.done += got;
    }
    true
}
