//! The connections of one download (a file, or a video's part), run until every part of it is
//! in, one fails for good, or the download is paused: more of them when asked for (the add
//! dialog's early single connection, confirmed), and another go now and then for those that
//! bowed out.

use super::task::Task;
use super::worker::{connection_loop, WorkerCtx};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// How often a connection that bowed out is tried again.
const REGROW: Duration = Duration::from_secs(30);
/// How often what was written is flushed to the disk, and the progress saved with it counted:
/// after a crash or a power cut the download goes on from there, never from data that was
/// only in the system's cache (and so, lost: a file with holes that looks finished).
const CHECKPOINT: Duration = Duration::from_secs(5);

/// Run `count` connections over `ctx` (more if `task.grow` asks for them). A lasting error stops
/// them all (`ctx.cancel`) and is returned once they have.
pub(super) async fn run(task: &Task, ctx: &Arc<WorkerCtx>, count: usize) -> Result<(), String> {
    let mut failed: Option<String> = None;
    let mut stop = |msg: String| {
        failed.get_or_insert(msg);
        ctx.cancel.cancel();
    };
    let mut workers = tokio::task::JoinSet::new();
    let (mut spawned, mut want) = (0, count);
    let mut regrow = tokio::time::interval(REGROW);
    regrow.reset(); // the first try after a full period
    let mut flush = tokio::time::interval(CHECKPOINT);
    flush.reset();
    let mut flushing = None; // (one flush at a time, beside the connections, never holding them up)
    loop {
        while spawned < want {
            workers.spawn(connection_loop(ctx.clone()));
            spawned += 1;
        }
        tokio::select! {
            done = workers.join_next() => match done {
                None => break,
                Some(Ok(Err(msg))) => stop(msg),
                Some(Err(join_err)) => stop(format!("internal error: {join_err}")),
                Some(Ok(Ok(()))) => {}
            },
            // more connections asked for (the add dialog's early single connection was confirmed):
            // they split the busy parts, nothing reconnects
            _ = task.grow.notified(), if ctx.resumable => {
                let target = task.info.lock().unwrap().connections;
                if target > want && !ctx.cancel.is_cancelled() {
                    ctx.live.fetch_add(target - want, Ordering::SeqCst);
                    ctx.segments.spread(target - want);
                    want = target;
                }
            }
            // connections that bowed out (refused by the server, or a bad moment of the network)
            // get another go now and then, so a long download doesn't stay on fewer of them; one
            // the server still refuses just bows out again
            _ = regrow.tick(), if ctx.resumable => {
                let live = ctx.live.load(Ordering::SeqCst);
                if live < want && !ctx.cancel.is_cancelled() && !ctx.segments.all_finished() {
                    ctx.live.fetch_add(1, Ordering::SeqCst);
                    workers.spawn(connection_loop(ctx.clone()));
                }
            }
            _ = flush.tick(), if flushing.as_ref().is_none_or(|f: &tokio::task::JoinHandle<()>| f.is_finished()) => {
                flushing = Some(tokio::spawn(checkpoint(ctx.clone())));
            }
        }
    }
    if let Some(f) = flushing {
        let _ = f.await;
    }
    checkpoint(ctx.clone()).await; // paused or done: everything it got, kept
    failed.map_or(Ok(()), Err)
}

/// Flush what the connections wrote to the disk, then count it as saved.
async fn checkpoint(ctx: Arc<WorkerCtx>) {
    let segments = ctx.segments.snapshot(); // (all of it written before the flush starts)
    let file = ctx.file.clone();
    // a disk that can't be asked (some network ones) has it written all the same, in time
    let _ = tokio::task::spawn_blocking(move || file.sync_data()).await;
    ctx.segments.mark_durable(segments);
}
