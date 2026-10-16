//! Where a menu's lines go (their heights, the menu's width, the line under the mouse), where on
//! the screen it opens, and which line the keyboard goes to. No drawing here: tested below.

use super::Item;
use windows::Win32::Foundation::{POINT, RECT};

/// Sizes in 96-DPI pixels: a line, a separator, the room above the first line and below the
/// last, the corners, and the room around the menu for its shadow.
pub const ROW: i32 = 28;
pub const LINE: i32 = 9;
pub const PAD: i32 = 6;
pub const RADIUS: i32 = 4;
pub const SHADOW: i32 = 14;
/// Columns: the text starts here (after the ticks' column when there's one); the gap before a
/// shortcut; the room after the text (with a sub-menu's arrow in it); the widest a menu gets.
pub const TEXT: i32 = 14;
pub const TICKS: i32 = 34;
pub const GAP: i32 = 32;
pub const END: i32 = 20;
pub const ARROW: i32 = 30;
pub const MIN: i32 = 180;
pub const MAX: i32 = 520;

/// A menu's measurements (screen pixels, from the top left of its border).
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub rows: Vec<(i32, i32)>, // each item's top and bottom
    pub width: i32,
    pub height: i32,
    pub text: i32,     // where the names start
    pub shortcut: i32, // where the shortcuts end (right-aligned)
}

/// The name before the tab, the shortcut (or a download's state) after it.
pub fn split(text: &str) -> (&str, &str) {
    text.split_once('\t').unwrap_or((text, ""))
}

/// The text as shown: without Windows' & marks (&& is an &).
pub fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
        } else if chars.peek() == Some(&'&') {
            out.push('&');
            chars.next();
        }
    }
    out
}

/// The letter after a single &, lower case: typed, it picks the item.
pub fn mnemonic(text: &str) -> Option<char> {
    let mut chars = split(text).0.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.next() {
                Some('&') => {}
                Some(letter) => return letter.to_lowercase().next(),
                None => return None,
            }
        }
    }
    None
}

/// Lay out `items`, measuring text with `measure(text, bold)`; `scale` turns 96-DPI pixels into
/// the screen's; at least `min_width` wide (a drop-down's button).
pub fn measure(items: &[Item], measure: impl Fn(&str, bool) -> i32, scale: impl Fn(i32) -> i32, min_width: i32) -> Geometry {
    let ticks = items.iter().any(|i| matches!(i, Item::Tick(..)));
    let subs = items.iter().any(|i| matches!(i, Item::Sub(..)));
    let (mut names, mut shortcuts) = (0, 0);
    for item in items {
        let (name, shortcut) = split(item.text());
        names = names.max(measure(&plain(name), matches!(item, Item::Bold(..))));
        shortcuts = shortcuts.max(measure(shortcut, false));
    }
    let text = scale(if ticks { TICKS } else { TEXT });
    let end = scale(if subs { ARROW } else { END });
    let gap = if shortcuts > 0 { scale(GAP) } else { 0 };
    let width = (text + names + gap + shortcuts + end).clamp(scale(MIN).max(min_width), scale(MAX).max(min_width));
    let mut y = 1 + scale(PAD); // (inside the border)
    let rows = items.iter().map(|item| {
        let top = y;
        y += scale(if matches!(item, Item::Line) { LINE } else { ROW });
        (top, y)
    }).collect();
    Geometry { rows, width, height: y + scale(PAD) + 1, text, shortcut: width - end }
}

impl Geometry {
    /// The item at (x, y) in the menu's pixels.
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        if x < 1 || x >= self.width - 1 {
            return None;
        }
        self.rows.iter().position(|&(top, bottom)| (top..bottom).contains(&y))
    }
}

/// Where a menu opens (screen pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum At {
    /// At a point (the mouse): to its bottom right, or wherever there's room.
    Point(POINT),
    /// Under something (a menu bar's name, a drop-down's button), or over it if there's no room.
    Below(RECT),
    /// Beside a menu's line (a sub-menu): `RECT` is the parent menu, `i32` the line's top.
    Beside(RECT, i32),
}

/// The top left of a menu `size` big that opens `at`, kept inside `work` (the screen's work area).
pub fn place(at: At, size: (i32, i32), work: RECT, pad: i32) -> POINT {
    let (w, h) = size;
    let (mut x, mut y) = match at {
        At::Point(p) => (if p.x + w > work.right { p.x - w } else { p.x }, if p.y + h > work.bottom { p.y - h } else { p.y }),
        At::Below(r) => (r.left, if r.bottom + h > work.bottom && r.top - h >= work.top { r.top - h } else { r.bottom }),
        At::Beside(parent, top) => {
            let x = if parent.right + w > work.right && parent.left - w >= work.left { parent.left - w } else { parent.right };
            (x, top - pad - 1)
        }
    };
    x = x.min(work.right - w).max(work.left);
    y = y.min(work.bottom - h).max(work.top);
    POINT { x, y }
}

/// The next item the keyboard can go to, down or up from `from` (round the end), if any.
pub fn step(items: &[Item], from: Option<usize>, down: bool) -> Option<usize> {
    let n = items.len();
    let start = match (from, down) {
        (Some(i), true) => i + 1,
        (Some(i), false) => i + n - 1,
        (None, true) => 0,
        (None, false) => n.checked_sub(1)?,
    };
    (0..n).map(|k| if down { (start + k) % n } else { (start + n - k) % n }).find(|&i| items[i].enabled())
}

/// The items `letter` picks: the next one after `from`, and whether it's the only one.
pub fn by_letter(items: &[Item], from: Option<usize>, letter: char) -> Option<(usize, bool)> {
    let letter = letter.to_lowercase().next()?;
    let matching: Vec<usize> = (0..items.len()).filter(|&i| mnemonic(items[i].text()) == Some(letter)).collect();
    let next = matching.iter().copied().find(|&i| from.is_some_and(|f| i > f)).or(matching.first().copied())?;
    Some((next, matching.len() == 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Item::{Do, Line, Sub, Tick};

    fn items() -> Vec<Item> {
        vec![Do(1, "&Open\tCtrl+O".into(), true), Do(2, "Open &with...".into(), false), Line,
             Tick(3, "&Toolbar".into(), true), Sub("&Speed && limits".into(), vec![Do(4, "&Off".into(), true)]),
             Do(5, "&Other".into(), true)]
    }

    #[test]
    fn texts() {
        assert_eq!(split("&Open\tCtrl+O"), ("&Open", "Ctrl+O"));
        assert_eq!(plain("&Speed && limits"), "Speed & limits");
        assert_eq!(mnemonic("&Speed && limits"), Some('s'));
        assert_eq!(mnemonic("Fish && chips"), None);
        assert_eq!(mnemonic("Open &with...\tW"), Some('w'));
        assert_eq!(mnemonic("a.zip\t50% &done"), None); // (only the name counts)
    }

    #[test]
    fn rows_and_width() {
        let g = measure(&items(), |t, _| t.chars().count() as i32 * 7, |v| v, 0);
        assert_eq!(g.rows[0], (1 + PAD, 1 + PAD + ROW));
        assert_eq!(g.rows[2], (1 + PAD + 2 * ROW, 1 + PAD + 2 * ROW + LINE));
        assert_eq!(g.height, 1 + PAD + 5 * ROW + LINE + PAD + 1);
        assert_eq!(g.text, TICKS); // a tick in the menu: names after the ticks' column
        // the longest name ("Speed & limits", 14) and shortcut ("Ctrl+O", 6), with the arrow's room
        assert_eq!(g.width, (TICKS + 14 * 7 + GAP + 6 * 7 + ARROW).max(MIN));
        assert_eq!(g.shortcut, g.width - ARROW);
        let wide = measure(&items(), |t, _| t.len() as i32 * 100, |v| v, 0);
        assert_eq!(wide.width, MAX);
        assert_eq!(measure(&items(), |_, _| 1, |v| v, 400).width, 400);
    }

    #[test]
    fn hit() {
        let g = measure(&items(), |_, _| 50, |v| v, 0);
        assert_eq!(g.hit(10, 1 + PAD), Some(0));
        assert_eq!(g.hit(10, 1 + PAD + ROW), Some(1));
        assert_eq!(g.hit(10, PAD), None); // the padding
        assert_eq!(g.hit(0, 1 + PAD), None); // the border
        assert_eq!(g.hit(10, g.height - 2), None);
    }

    #[test]
    fn keys() {
        let items = items();
        assert_eq!(step(&items, None, true), Some(0));
        assert_eq!(step(&items, Some(0), true), Some(3)); // over the greyed one and the line
        assert_eq!(step(&items, Some(5), true), Some(0)); // round the end
        assert_eq!(step(&items, Some(0), false), Some(5));
        assert_eq!(step(&items, None, false), Some(5));
        assert_eq!(step(&[Line], None, true), None);
        assert_eq!(by_letter(&items, None, 'O'), Some((0, false))); // Open and Other
        assert_eq!(by_letter(&items, Some(0), 'o'), Some((5, false)));
        assert_eq!(by_letter(&items, Some(5), 'o'), Some((0, false)));
        assert_eq!(by_letter(&items, None, 't'), Some((3, true)));
        assert_eq!(by_letter(&items, None, 'w'), Some((1, true))); // (greyed: found, but it does nothing)
        assert_eq!(by_letter(&items, None, 'z'), None);
    }

    #[test]
    fn placing() {
        let work = RECT { left: 0, top: 0, right: 1000, bottom: 700 };
        let size = (200, 300);
        assert_eq!(place(At::Point(POINT { x: 10, y: 20 }), size, work, 6), POINT { x: 10, y: 20 });
        // by the bottom right corner (the notification area): to the left and above
        assert_eq!(place(At::Point(POINT { x: 950, y: 720 }), size, work, 6), POINT { x: 750, y: 400 });
        let name = RECT { left: 900, top: 30, right: 960, bottom: 52 };
        assert_eq!(place(At::Below(name), size, work, 6), POINT { x: 800, y: 52 });
        let button = RECT { left: 100, top: 600, right: 300, bottom: 630 };
        assert_eq!(place(At::Below(button), size, work, 6), POINT { x: 100, y: 300 }); // above it
        let parent = RECT { left: 100, top: 100, right: 300, bottom: 400 };
        assert_eq!(place(At::Beside(parent, 150), size, work, 6), POINT { x: 300, y: 143 });
        let parent = RECT { left: 700, top: 500, right: 900, bottom: 690 };
        assert_eq!(place(At::Beside(parent, 600), size, work, 6), POINT { x: 500, y: 400 }); // left, up
    }
}
