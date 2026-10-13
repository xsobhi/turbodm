//! Splitting and bookkeeping tests.

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
    assert_eq!(map.receive(b, usize::MAX).0 as u64, (5 * MB) / 2);
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
fn no_split_when_the_rest_is_quick_at_this_speed() {
    let map = SegmentMap::create(Some(40 * MB), 1, MB);
    map.claim();
    map.set_speed((10 * MB) as f64); // 10 MB/s per connection: 40 MB left is worth a split
    assert!(map.claim().is_some());
    map.set_speed((50 * MB) as f64); // 50 MB/s: halves of ~20 MB are done in under 2 s
    assert!(map.claim().is_none());
}

#[test]
fn no_split_below_twice_min() {
    let map = SegmentMap::create(Some(2 * MB - 1), 1, MB);
    map.claim();
    assert!(map.claim().is_none());
}

#[test]
fn splits_after_what_was_received_but_not_written() {
    let map = SegmentMap::create(Some(10 * MB), 1, MB);
    let a = map.claim().unwrap();
    assert_eq!(map.receive(a, (4 * MB) as usize), ((4 * MB) as usize, 0)); // buffered, not on disk
    let b = map.claim().unwrap();
    assert_eq!(map.get(b).start, 7 * MB); // half of the 6 MB not received yet
    assert_eq!(map.receive(a, (5 * MB) as usize), ((3 * MB) as usize, 4 * MB));
    map.commit(a, (7 * MB) as usize);
    assert!(map.get(a).finished() && map.get(a).ahead == 0);
}

#[test]
fn progress_counts_what_arrived_before_it_is_written() {
    let map = SegmentMap::create(Some(10 * MB), 2, MB);
    let a = map.claim().unwrap();
    map.receive(a, 300_000);
    assert_eq!((map.total_received(), map.total_done()), (300_000, 0));
    map.commit(a, 300_000);
    assert_eq!((map.total_received(), map.total_done()), (300_000, 300_000));
}

#[test]
fn a_free_connection_takes_over_a_stuck_one() {
    let map = SegmentMap::create(Some(10 * MB), 2, MB);
    let (a, b) = (map.claim().unwrap(), map.claim().unwrap());
    map.receive(a, MB as usize);
    map.commit(a, MB as usize);
    map.segs.lock().unwrap()[a].heard = Some(Instant::now() - Duration::from_secs(60)); // silent since
    assert!(!map.others_flowing(b));
    map.receive(b, (5 * MB) as usize);
    assert!(map.others_flowing(a));
    map.commit(b, (5 * MB) as usize);
    map.release(b);
    let taken = map.claim().unwrap(); // all of a's rest, not half
    assert_eq!((map.get(taken).start, map.get(taken).end), (MB, Some(5 * MB)));
    assert!(map.get(a).received());
}
