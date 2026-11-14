//! The scheduler's timer (Options → Scheduler), beside the monitor: at the start time the
//! unfinished downloads join the queue, at the stop time they're paused, and once all it started
//! are done the UI is told what to do then (sleep or shut down, after its countdown).

use super::Manager;
use crate::engine::task::Status;
use crate::power::Power;
use crate::schedule::clock::{self, Minute};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// A queue the scheduler started: the downloads in it, and when it's to stop.
struct Run {
    ids: Vec<String>,
    stop: Option<Minute>,
}

impl Manager {
    /// Put every unfinished download in the queue and start as many as may run; the ones it holds.
    pub fn start_queue(&self) -> Vec<String> {
        for task in self.listed() {
            if matches!(task.status(), Status::Paused | Status::Error) {
                task.set_status(Status::Queued);
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        self.schedule();
        self.listed().iter().filter(|t| t.status() == Status::Queued || t.status().is_active())
            .map(|t| t.id.clone()).collect()
    }

    /// Pause what's downloading or waiting in the queue (resumable as ever).
    pub fn stop_queue(&self) {
        for task in self.listed() {
            if task.status() == Status::Queued || task.status().is_active() {
                self.pause(&task.id);
            }
        }
    }

    /// What to do once a scheduled queue is done (Sleep, Shut down), as it happens.
    pub fn queue_finished(&self) -> async_channel::Receiver<Power> {
        self.finished.1.clone()
    }

    /// Look at the clock every few seconds. A start (or stop) that went by while the computer
    /// slept happens when it wakes; one before TurboDM started doesn't.
    pub(super) async fn scheduler(self: Arc<Self>) {
        let mut last = clock::now();
        let mut run: Option<Run> = None;
        while !self.stopped.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let now = clock::now();
            let plan = self.settings().schedule;
            if plan.next_start(last).is_some_and(|start| start <= now) {
                let ids = self.start_queue();
                run = (!ids.is_empty()).then(|| Run { ids, stop: plan.stop_after(now) });
            }
            if let Some(r) = &run {
                if r.stop.is_some_and(|stop| stop <= now) {
                    self.stop_queue();
                    run = None;
                } else if let Some(done) = self.run_over(r) {
                    if done && plan.when_done != Power::Nothing {
                        let _ = self.finished.0.try_send(plan.when_done);
                    }
                    run = None;
                }
            }
            last = now;
        }
    }

    /// Whether a run is over: nothing of it downloading or waiting any more. Then, whether it
    /// finished (each done, failed or deleted) rather than being paused by the user.
    fn run_over(&self, run: &Run) -> Option<bool> {
        let statuses: Vec<Status> = run.ids.iter().filter_map(|id| self.find(id)).map(|t| t.status()).collect();
        if statuses.iter().any(|s| *s == Status::Queued || s.is_active()) {
            return None;
        }
        Some(!statuses.contains(&Status::Paused))
    }
}
