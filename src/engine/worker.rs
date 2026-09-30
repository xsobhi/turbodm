//! One download connection: fetch byte ranges and write them straight into the file.

use super::http::{self, FetchError, Headers};
use super::limiter::RateLimiter;
use super::segments::SegmentMap;
use futures_util::StreamExt;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Everything the connections of one download share.
pub struct WorkerCtx {
    pub file: File,
    pub segments: Arc<SegmentMap>,
    pub client: reqwest::Client,
    pub url: String,
    pub headers: Headers,
    pub resumable: bool,
    pub retries: u32,
    pub limiter: Arc<RateLimiter>,
    pub cancel: CancellationToken,
    pub live: AtomicUsize, // connections still running
}

impl WorkerCtx {
    /// Let this connection bow out (the server limits connections per IP), but only
    /// while another one keeps going - the last connection always stays and retries.
    fn try_retire(&self) -> bool {
        self.live.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n > 1).then(|| n - 1)).is_ok()
    }
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Retire,
}

/// `TURBODM_DEBUG=1 turbodm ...` prints connection events to stderr.
macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var_os("TURBODM_DEBUG").is_some() {
            eprintln!("[{:?}] {}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() % 1_000_000,
                format!($($arg)*));
        }
    };
}

enum StreamError {
    Network(String),
    Write(String),
}

/// Keep claiming segments (splitting busy ones) until nothing is left.
pub async fn connection_loop(ctx: Arc<WorkerCtx>) -> Result<(), String> {
    while !ctx.cancel.is_cancelled() {
        let Some(index) = ctx.segments.claim() else { break };
        let result = fetch(&ctx, index).await;
        ctx.segments.release(index);
        if result? == Flow::Retire {
            debug!("seg {index}: connection retired, others take over");
            return Ok(()); // live count was already decremented
        }
    }
    ctx.live.fetch_sub(1, Ordering::SeqCst);
    Ok(())
}

async fn fetch(ctx: &WorkerCtx, index: usize) -> Result<Flow, String> {
    let mut failures = 0;
    while !ctx.segments.get(index).finished() && !ctx.cancel.is_cancelled() {
        if !ctx.resumable && ctx.segments.get(index).done > 0 {
            ctx.segments.restart(index); // a plain stream can't continue: start over
        }
        let seg = ctx.segments.get(index);
        let range = ctx.resumable.then_some((seg.pos(), seg.end));
        let opened = tokio::select! {
            r = http::open(&ctx.client, &ctx.url, &ctx.headers, range) => r,
            _ = ctx.cancel.cancelled() => return Ok(Flow::Continue),
        };
        debug!("seg {index} @{:?} open -> {}", range, match &opened { Ok(r) => r.status().to_string(), Err(e) => e.to_string() });
        let resp = match opened {
            Ok(resp) => resp,
            Err(FetchError::RangeNotSupported) => {
                return Err("The server no longer allows resuming; restart the download".into())
            }
            Err(FetchError::Http(code @ (400 | 401 | 403 | 404 | 405 | 410 | 451), reason)) => {
                return Err(format!("HTTP {code} {reason} - the link may have expired \
                                    (use Refresh address)"));
            }
            Err(FetchError::Http(429 | 503, _)) if ctx.try_retire() => return Ok(Flow::Retire),
            Err(FetchError::Network(_)) if failures >= 1 && ctx.try_retire() => return Ok(Flow::Retire),
            Err(err) => {
                failures = backoff(ctx, failures, err.to_string()).await?;
                continue;
            }
        };
        let streamed = stream(ctx, index, resp).await;
        debug!("seg {index} stream end: {}", match &streamed { Ok(done) => format!("complete={done}"), Err(StreamError::Network(m) | StreamError::Write(m)) => m.clone() });
        match streamed {
            Ok(true) => failures = 0,
            Ok(false) if ctx.cancel.is_cancelled() => return Ok(Flow::Continue),
            Ok(false) => failures = backoff(ctx, failures, "connection closed early".into()).await?,
            Err(StreamError::Write(msg)) => return Err(msg),
            Err(StreamError::Network(msg)) => failures = backoff(ctx, failures, msg).await?,
        }
    }
    Ok(Flow::Continue)
}

async fn backoff(ctx: &WorkerCtx, failures: u32, reason: String) -> Result<u32, String> {
    let failures = failures + 1;
    if failures > ctx.retries {
        return Err(format!("Gave up after {} retries: {reason}", ctx.retries));
    }
    let delay = Duration::from_secs_f64((0.5 * 2f64.powi(failures as i32)).min(30.0));
    tokio::select! {
        _ = tokio::time::sleep(delay) => {}
        _ = ctx.cancel.cancelled() => {}
    }
    Ok(failures)
}

/// Copy the body into the file. Ok(true) when the segment is complete.
async fn stream(ctx: &WorkerCtx, index: usize, resp: reqwest::Response) -> Result<bool, StreamError> {
    let mut body = resp.bytes_stream();
    loop {
        let chunk = tokio::select! {
            c = body.next() => c,
            _ = ctx.cancel.cancelled() => return Ok(false),
        };
        let bytes = match chunk {
            None => {
                if ctx.segments.get(index).end.is_none() {
                    ctx.segments.mark_eof(index); // unknown size: stream end = file end
                    return Ok(true);
                }
                return Ok(ctx.segments.get(index).finished());
            }
            Some(Err(err)) => return Err(StreamError::Network(FetchError::from(err).to_string())),
            Some(Ok(bytes)) => bytes,
        };
        let (allowed, offset) = ctx.segments.reserve(index, bytes.len());
        if allowed > 0 {
            ctx.file
                .write_all_at(&bytes[..allowed], offset)
                .map_err(|e| StreamError::Write(format!("Cannot write the file: {e}")))?;
            ctx.segments.commit(index, allowed);
            ctx.limiter.consume(allowed).await;
        }
        if allowed < bytes.len() || ctx.segments.get(index).finished() {
            return Ok(true); // reached our (possibly shortened) end
        }
    }
}
