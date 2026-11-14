//! How fast each part's connection is going, for the two takeovers that go by time rather than
//! size: a connection that never got its first byte (a connect or TLS handshake that hangs), and
//! a slow one holding up the end. Near the end the halves left are too small for the usual split
//! (SPLIT_SECONDS), yet one slow connection on its last megabyte can take minutes while the
//! others sit idle: a free connection then takes over the share of its rest that makes both
//! finish together.

use super::Segment;
use std::time::{Duration, Instant};

/// A connection may wait this long for its first byte before a free one takes its part.
pub(super) const FIRST_BYTE: Duration = Duration::from_secs(8);
/// What starting a new connection costs (connect, TLS, the first byte), counted against a split.
const SETUP_SECONDS: f64 = 1.0;
/// Less isn't worth a new connection.
const SMALLEST_TAKE: u64 = 64 << 10;
/// Its speed is known once it's been receiving this long.
const MEASURED: f64 = 1.0;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Pace {
    claimed: Option<Instant>, // when its connection took it
    since: Option<Instant>,   // when that connection's first byte came
    base: u64,                // what the part had got by then
}

impl Pace {
    pub(super) fn claimed(now: Instant) -> Self {
        Pace { claimed: Some(now), ..Pace::default() }
    }

    /// Bytes arrived for its part, which had `got` before them.
    pub(super) fn received(&mut self, got: u64, now: Instant) {
        if self.since.is_none() {
            (self.since, self.base) = (Some(now), got);
        }
    }

    /// Its connection has had nothing at all, for longer than a first byte should take.
    pub(super) fn never_started(&self, now: Instant) -> bool {
        self.since.is_none() && self.claimed.is_some_and(|t| now.duration_since(t) > FIRST_BYTE)
    }

    /// Its connection's speed (bytes/s), once measured.
    fn rate(&self, seg: &Segment, now: Instant) -> Option<f64> {
        let elapsed = now.duration_since(self.since?).as_secs_f64();
        (elapsed >= MEASURED).then(|| seg.got().saturating_sub(self.base) as f64 / elapsed)
    }
}

/// The part to split for a free connection that gets `per_connection` bytes/s, because its own is
/// so slow the split ends the download sooner, and how much of its rest it keeps (the rest is the
/// free connection's): the one that would take longest to finish on its own.
pub(super) fn slow_split(segs: &[Segment], pace: &[Pace], per_connection: f64, now: Instant) -> Option<(usize, u64)> {
    if per_connection <= 0.0 {
        return None;
    }
    let mut best: Option<(usize, u64, f64)> = None;
    for (i, seg) in segs.iter().enumerate().filter(|(_, s)| s.active) {
        let (Some(rest), Some(rate)) = (seg.unreceived(), pace.get(i).and_then(|p| p.rate(seg, now))) else { continue };
        let (rest_f, rate) = (rest as f64, rate.max(1.0));
        let alone = rest_f / rate;
        // both finishing together, the new one starting SETUP_SECONDS late:
        // keep / rate = SETUP + (rest - keep) / per_connection
        let keep = ((rest_f + per_connection * SETUP_SECONDS) * rate / (rate + per_connection)).min(rest_f);
        let together = keep / rate;
        let worth = rest - keep as u64 >= SMALLEST_TAKE && alone - together >= super::SPLIT_SECONDS;
        if worth && best.is_none_or(|(_, _, longest)| alone > longest) {
            best = Some((i, keep as u64, alone));
        }
    }
    best.map(|(i, keep, _)| (i, keep))
}

#[cfg(test)]
impl Pace {
    pub(super) fn received_at_for_test(&mut self, since: Instant, base: u64) {
        (self.since, self.base) = (Some(since), base);
    }
}
