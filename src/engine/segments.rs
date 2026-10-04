//! Byte-range bookkeeping with IDM-style dynamic splitting.
//!
//! The file is cut into one segment per connection. When a connection finishes, it
//! takes the second half of the largest segment still downloading, so every
//! connection stays busy until the very end.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: Option<u64>, // one past the last byte (exclusive); None while size unknown
    pub done: u64,        // bytes written, counted from start
    #[serde(skip)]
    pub active: bool,
}

impl Segment {
    fn new(start: u64, end: Option<u64>) -> Self {
        Segment { start, end, done: 0, active: false }
    }
    pub fn pos(&self) -> u64 {
        self.start + self.done
    }
    pub fn remaining(&self) -> Option<u64> {
        self.end.map(|end| end.saturating_sub(self.pos()))
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
}

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
        let segs = segs.into_iter().map(|s| Segment { active: false, ..s }).collect();
        SegmentMap { segs: Mutex::new(segs), min_split: Mutex::new(min_split) }
    }

    pub fn set_min_split(&self, bytes: u64) {
        *self.min_split.lock().unwrap() = bytes;
    }

    /// Work for a connection: an idle segment, or half of the biggest busy one.
    pub fn claim(&self) -> Option<usize> {
        let min_split = *self.min_split.lock().unwrap();
        let mut segs = self.segs.lock().unwrap();
        if let Some(i) = segs.iter().position(|s| !s.active && !s.finished()) {
            segs[i].active = true;
            return Some(i);
        }
        let (best, remaining) = segs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.active)
            .filter_map(|(i, s)| s.remaining().map(|r| (i, r)))
            .max_by_key(|&(_, r)| r)?;
        if remaining < 2 * min_split {
            return None;
        }
        let middle = segs[best].pos() + remaining / 2;
        let end = segs[best].end;
        segs[best].end = Some(middle);
        segs.push(Segment { active: true, ..Segment::new(middle, end) });
        Some(segs.len() - 1)
    }

    pub fn release(&self, i: usize) {
        self.segs.lock().unwrap()[i].active = false;
    }

    pub fn get(&self, i: usize) -> Segment {
        self.segs.lock().unwrap()[i]
    }

    /// How many of `n` bytes may be written for segment i, and at which offset.
    pub fn reserve(&self, i: usize, n: usize) -> (usize, u64) {
        let seg = self.segs.lock().unwrap()[i];
        let allowed = seg.remaining().map_or(n as u64, |r| r.min(n as u64));
        (allowed as usize, seg.pos())
    }

    pub fn commit(&self, i: usize, n: usize) {
        self.segs.lock().unwrap()[i].done += n as u64;
    }

    /// Throw away progress (a non-resumable stream broke and must start over).
    pub fn restart(&self, i: usize) {
        self.segs.lock().unwrap()[i].done = 0;
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
mod tests {
    use super::*;
    const MB: u64 = 1 << 20;

    #[test]
    fn splits_initially_and_dynamically() {
        let map = SegmentMap::create(Some(100 * MB), 32, MB);
        assert_eq!(map.snapshot().len(), 32);
        assert_eq!(map.snapshot().last().unwrap().end, Some(100 * MB));
        assert_eq!(SegmentMap::create(Some(3 * MB), 32, MB).snapshot().len(), 3);

        let map = SegmentMap::create(Some(10 * MB), 2, MB);
        let (a, b) = (map.claim().unwrap(), map.claim().unwrap());
        map.commit(a, (5 * MB) as usize);
        map.release(a);
        let stolen = map.claim().unwrap();
        assert_eq!(map.get(stolen).start, 5 * MB + (5 * MB) / 2);
        assert_eq!(map.get(b).end.unwrap(), map.get(stolen).start);
        assert_eq!(map.reserve(b, usize::MAX).0 as u64, (5 * MB) / 2);
    }

    #[test]
    fn empty_unknown_stream_finishes() {
        let map = SegmentMap::create(None, 1, MB);
        let i = map.claim().unwrap();
        map.mark_eof(i);
        map.release(i);
        assert!(map.all_finished());
        assert_eq!(map.total_done(), 0);
    }

    #[test]
    fn no_split_below_twice_min() {
        let map = SegmentMap::create(Some(2 * MB - 1), 1, MB);
        map.claim();
        assert!(map.claim().is_none());
    }
}
