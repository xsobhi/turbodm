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
        }
    }
    failed.map_or(Ok(()), Err)
}
