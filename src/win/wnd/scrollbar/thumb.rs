//! Where a scroll bar's thumb goes for a scroll position, and the position for where it's
//! dragged to (Windows' scroll info: positions `min..=max`, `page` of them shown at once).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Range {
    pub min: i32,
    pub max: i32,
    pub page: u32,
    pub pos: i32,
}

impl Range {
    /// How many positions the top (or left) can be at, after the first.
    fn travel(&self) -> i64 {
        (self.max as i64 - self.min as i64 + 1 - self.page as i64).max(0)
    }

    /// The thumb's start and length along a track `length` long, no shorter than `smallest`;
    /// None when everything shows (nothing to scroll).
    pub fn thumb(&self, length: i32, smallest: i32) -> Option<(i32, i32)> {
        let range = self.max as i64 - self.min as i64 + 1;
        if self.page == 0 || self.travel() == 0 || length <= 0 {
            return None;
        }
        let size = ((length as i64 * self.page as i64 / range) as i32).max(smallest).min(length);
        let room = (length - size) as i64;
        let at = ((self.pos - self.min) as i64).clamp(0, self.travel()) * room / self.travel();
        Some((at as i32, size))
    }

    /// The position that puts the thumb (`size` long) at `offset` along the track.
    pub fn at(&self, offset: i32, length: i32, size: i32) -> i32 {
        let room = (length - size) as i64;
        if room <= 0 {
            return self.min;
        }
        self.min + ((offset as i64).clamp(0, room) * self.travel() + room / 2).div_euclid(room) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::Range;

    #[test]
    fn thumbs() {
        // 100 rows, 20 shown: a fifth of the track
        let r = Range { min: 0, max: 99, page: 20, pos: 0 };
        assert_eq!(r.thumb(200, 24), Some((0, 40)));
        assert_eq!(Range { pos: 80, ..r }.thumb(200, 24), Some((160, 40))); // at the end
        assert_eq!(Range { pos: 40, ..r }.thumb(200, 24), Some((80, 40)));
        assert_eq!(Range { pos: 500, ..r }.thumb(200, 24), Some((160, 40))); // (never past it)
        // many rows: never smaller than `smallest`
        assert_eq!(Range { max: 9999, ..r }.thumb(200, 24), Some((0, 24)));
        // all shown: nothing to scroll
        assert_eq!(Range { max: 19, ..r }.thumb(200, 24), None);
        assert_eq!(Range { page: 0, ..r }.thumb(200, 24), None);
    }

    #[test]
    fn dragging() {
        let r = Range { min: 0, max: 99, page: 20, pos: 0 };
        assert_eq!(r.at(0, 200, 40), 0);
        assert_eq!(r.at(160, 200, 40), 80);
        assert_eq!(r.at(80, 200, 40), 40);
        assert_eq!(r.at(-30, 200, 40), 0);
        assert_eq!(r.at(999, 200, 40), 80);
        for pos in 0..=80 {
            let (at, size) = Range { pos, ..r }.thumb(200, 24).unwrap();
            assert_eq!(r.at(at, 200, size), pos); // there and back
        }
    }
}
