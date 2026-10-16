//! A video's parts downloaded by TurboDM's own connections, like any file: many at once, the
//! progress live, resumable after a restart. Their addresses come from yt-dlp and expire after
//! hours: when the site turns one down, the page is looked at again for fresh ones, and what's
//! downloaded is kept if it's the same stream.

use super::names::{part_file, stem};
use super::{probe, Source};
use crate::engine::http::{self, FetchError, Headers};
use crate::engine::runner::open_part;
use crate::engine::segments::SegmentMap;
use crate::engine::task::{Status, Task};
use crate::engine::worker::WorkerCtx;
use crate::engine::{pool, Shared};
use crate::util::part_path;
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

/// Look at the page again for fresh addresses of the same streams. False: it no longer has
/// them (yt-dlp, which can fall back to others, takes over).
async fn refresh(task: &Task, shared: &Shared, cancel: &CancellationToken) -> Result<bool, String> {
    task.video.set_activity(Some("Looking at the page again…"));
    let (url, headers, ids) = {
        let info = task.info.lock().unwrap();
        let job = info.video.clone().unwrap_or_default();
        let ids: Vec<String> = if job.sources.is_empty() { job.parts.iter().map(|(id, _)| id.clone()).collect() }
                               else { job.sources.iter().map(|s| s.format_id.clone()).collect() };
        (info.url.clone(), info.headers(), ids)
    };
    let client = shared.client();
    let looked = tokio::select! {
        found = probe(&client, &url, &headers, shared.settings().timeout_secs) => found,
        _ = cancel.cancelled() => return Ok(true), // (paused: the loop sees it)
    };
    task.video.set_activity(None);
    let found = looked?;
    let fresh: Option<Vec<Source>> = ids.iter().map(|id| found.formats.iter().find(|f| f.format_id == *id)
        .map(|f| f.source(found.duration))).collect();
    let Some(mut fresh) = fresh.filter(|f| !f.is_empty()) else { return Ok(false) };
    let mut info = task.info.lock().unwrap();
    if let Some(job) = info.video.as_mut() {
        for (new, old) in fresh.iter_mut().zip(&job.sources) {
            new.done = old.done;
        }
        job.sources = fresh;
        job.with_cookies = found.with_cookies;
    }
    Ok(true)
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

/// Download part `index` into its own file beside the video. False: paused.
async fn part(task: &Arc<Task>, shared: &Shared, cancel: &CancellationToken, index: usize) -> Result<bool, PartError> {
    let (dir, name, source, connections) = {
        let info = task.info.lock().unwrap();
        let source = info.video.as_ref().and_then(|v| v.sources.get(index).cloned()).unwrap_or_default();
        (info.directory.clone(), info.filename.clone().unwrap_or_else(|| "video".into()), source, info.connections)
    };
    let file = part_file(&dir, stem(&name), &source.format_id, &source.ext);
    if file.is_file() {
        return Ok(done(task, index, &file)); // in already (TurboDM closed just as it finished)
    }
    std::fs::create_dir_all(&dir).map_err(|e| PartError::Failed(format!("Cannot create folder: {e}")))?;
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
    let partial = part_path(&file);
    let saved = task.segments.lock().unwrap().clone();
    let same = |map: &SegmentMap| map.snapshot().iter().filter_map(|s| s.end).max() == size;
    let map = match saved.filter(|map| resumable && same(map) && partial.exists()) {
        Some(map) => map,
        None => {
            let _ = std::fs::remove_file(&partial);
            let count = if resumable { connections } else { 1 };
            Arc::new(SegmentMap::create(size, count, settings.min_split_kib * 1024))
        }
    };
    map.set_min_split(settings.min_split_kib * 1024);
    {
        let mut info = task.info.lock().unwrap();
        if let Some(job) = info.video.as_mut() {
            job.sources[index].size = size;
            let total = job.sources.iter().map(|s| s.size).sum::<Option<u64>>();
            info.size = total;
        }
    }
    *task.segments.lock().unwrap() = Some(map.clone());
    let count = if resumable { connections } else { 1 };
    let ctx = Arc::new(WorkerCtx {
        file: Arc::new(open_part(&partial, size).map_err(PartError::Failed)?),
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
    std::fs::rename(&partial, &file).map_err(|e| PartError::Failed(format!("Cannot rename the file: {e}")))?;
    Ok(done(task, index, &file))
}

/// Part `index` is in: counted once, its connections no longer shown.
fn done(task: &Task, index: usize, file: &std::path::Path) -> bool {
    let got = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    *task.segments.lock().unwrap() = None; // (before it's counted as done: never twice)
    if let Some(job) = task.info.lock().unwrap().video.as_mut() {
        job.sources[index].done = true;
        job.sources[index].size = Some(got);
        job.done += got;
    }
    true
}
