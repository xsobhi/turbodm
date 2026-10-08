//! Byte-range bookkeeping with IDM-style dynamic splitting.
//!
//! The file is cut into one segment per connection. When a connection finishes, it
//! takes the second half of the largest segment still downloading, so every
//! connection stays busy until the very end.

use serde::{Deserialize, Serialize};
use std::sync::{atomic::{AtomicU64, Ordering}, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: Option<u64>, // one past the last byte (exclusive); None while size unknown
    pub done: u64,        // bytes written, counted from start
    #[serde(skip)]
    pub ahead: u64,       // received after those, not written yet (disk::Pending)
    #[serde(skip)]
    pub active: bool,
}

impl Segment {
    fn new(start: u64, end: Option<u64>) -> Self {
        Segment { start, end, done: 0, ahead: 0, active: false }
    }
    pub fn pos(&self) -> u64 {
        self.start + self.done
    }
    pub fn remaining(&self) -> Option<u64> {
        self.end.map(|end| end.saturating_sub(self.pos()))
    }
    /// Not even received yet: what a split can take over.
    fn unreceived(&self) -> Option<u64> {
        self.end.map(|end| end.saturating_sub(self.pos() + self.ahead))
    }
    pub fn received(&self) -> bool {
        self.unreceived() == Some(0)
    }
    pub fn finished(&self) -> bool {
        self.remaining() == Some(0)
    }
    fn length(&self) -> u64 {
        self.end.map_or(self.done, |end| end - self.start)
    }
}

pub struct SegmentMap {
    segs: Mutex<Vec<Segment>>,
    min_split: Mutex<u64>,
    speed_floor: AtomicU64, // what one connection gets through in SPLIT_SECONDS now
}

/// Like IDM, a free connection takes over half a part only if that still takes this long.
pub const SPLIT_SECONDS: f64 = 2.0;

impl SegmentMap {
    pub fn create(size: Option<u64>, parts: usize, min_split: u64) -> Self {
        let segs = match size {
            None => vec![Segment::new(0, None)],
            Some(0) => vec![],
            Some(size) => {
                let parts = (parts as u64).min((size / min_split).max(1)).max(1);
                let chunk = size / parts;
                let mut v: Vec<_> = (0..parts)
                    .map(|i| Segment::new(i * chunk, Some((i + 1) * chunk)))
                    .collect();
                v.last_mut().unwrap().end = Some(size);
                v
            }
        };
        Self::from_segments(segs, min_split)
    }

    pub fn from_segments(segs: Vec<Segment>, min_split: u64) -> Self {
        let segs = segs.into_iter().map(|s| Segment { active: false, ahead: 0, ..s }).collect();
        SegmentMap { segs: Mutex::new(segs), min_split: Mutex::new(min_split), speed_floor: AtomicU64::new(0) }
    }

    pub fn set_min_split(&self, bytes: u64) {
        *self.min_split.lock().unwrap() = bytes;
    }

    /// The download's speed per connection now (bytes/s).
    pub fn set_speed(&self, per_connection: f64) {
        self.speed_floor.store((per_connection * SPLIT_SECONDS) as u64, Ordering::Relaxed);
    }

    /// Work for a connection: an idle segment, or half of the biggest busy one.
    pub fn claim(&self) -> Option<usize> {
        let min_split = (*self.min_split.lock().unwrap()).max(self.speed_floor.load(Ordering::Relaxed));
        let mut segs = self.segs.lock().unwrap();
        if let Some(i) = segs.iter().position(|s| !s.active && !s.finished()) {
            segs[i].active = true;
            return Some(i);
        }
        let (best, remaining) = segs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.active)
            .filter_map(|(i, s)| s.unreceived().map(|r| (i, r)))
            .max_by_key(|&(_, r)| r)?;
        if remaining < 2 * min_split {
            return None;
        }
        let middle = segs[best].pos() + segs[best].ahead + remaining / 2;
        let end = segs[best].end;
        segs[best].end = Some(middle);
        segs.push(Segment { active: true, ..Segment::new(middle, end) });
        Some(segs.len() - 1)
    }

    pub fn release(&self, i: usize) {
        let mut segs = self.segs.lock().unwrap();
        segs[i].active = false;
        segs[i].ahead = 0; // written, or dropped with a failed write
    }

    pub fn get(&self, i: usize) -> Segment {
        self.segs.lock().unwrap()[i]
    }

    /// `n` bytes arrived for segment i: how many of them belong to it (a split may have
    /// shortened it), and their offset in the file.
    pub fn receive(&self, i: usize, n: usize) -> (usize, u64) {
        let seg = &mut self.segs.lock().unwrap()[i];
        let allowed = seg.unreceived().map_or(n as u64, |r| r.min(n as u64));
        let offset = seg.pos() + seg.ahead;
        seg.ahead += allowed;
        (allowed as usize, offset)
    }

    /// `n` received bytes are on disk now.
    pub fn commit(&self, i: usize, n: usize) {
        let seg = &mut self.segs.lock().unwrap()[i];
        seg.done += n as u64;
        seg.ahead = seg.ahead.saturating_sub(n as u64);
    }

    /// Throw away progress (a non-resumable stream broke and must start over).
    pub fn restart(&self, i: usize) {
        let seg = &mut self.segs.lock().unwrap()[i];
        (seg.done, seg.ahead) = (0, 0);
    }

    /// An unknown-size stream ended normally: its position is the end of the file.
    pub fn mark_eof(&self, i: usize) {
        let mut segs = self.segs.lock().unwrap();
        segs[i].end = Some(segs[i].pos());
    }

    pub fn total_done(&self) -> u64 {
        self.segs.lock().unwrap().iter().map(|s| s.done.min(s.length())).sum()
    }

    pub fn all_finished(&self) -> bool {
        self.segs.lock().unwrap().iter().all(Segment::finished)
    }

    pub fn active_count(&self) -> usize {
        self.segs.lock().unwrap().iter().filter(|s| s.active).count()
    }

    /// Sorted copy of all segments (for drawing and saving).
    pub fn snapshot(&self) -> Vec<Segment> {
        let mut v = self.segs.lock().unwrap().clone();
        v.sort_by_key(|s| s.start);
        v
    }
}

#[cfg(test)]
mod tests;
