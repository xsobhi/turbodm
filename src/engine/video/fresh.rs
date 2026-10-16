//! Fresh addresses for a video's parts: theirs expire after hours (or sooner, when the site
//! turns them down); the page is looked at again for the same streams, and what's downloaded of
//! them is kept.

use super::{probe, Source};
use crate::engine::task::Task;
use crate::engine::Shared;
use tokio_util::sync::CancellationToken;

/// Look at the page again for fresh addresses of the same streams. False: it no longer has
/// them (yt-dlp, which can fall back to others, takes over).
pub(super) async fn refresh(task: &Task, shared: &Shared, cancel: &CancellationToken) -> Result<bool, String> {
    task.video.set_activity(Some("Looking at the page again…"));
    let (url, headers, ids) = {
        let info = task.info.lock().unwrap();
        let job = info.video.clone().unwrap_or_default();
        let ids: Vec<String> = if job.sources.is_empty() { job.parts.iter().map(|(id, _)| id.clone()).collect() }
                               else { job.sources.iter().map(|s| s.format_id.clone()).collect() };
        (info.url.clone(), info.headers(), ids)
    };
    let client = shared.client();
    super::forget(&url);
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
