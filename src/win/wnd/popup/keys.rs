//! The keyboard in a menu, as in Windows': Up and Down go along the lines (over greyed ones),
//! Right opens a sub-menu (or the bar's next menu), Left goes back, Enter chooses, Escape closes
//! one menu, Alt or F10 all of them; a line's underlined letter chooses it.

use super::layout::split;
use super::track::Menu;
use super::{Item, Outcome};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{TranslateMessage, MSG};

impl Menu<'_> {
    /// A key pressed (WM_KEYDOWN, WM_SYSKEYDOWN); `Some`: the menus are done.
    pub fn key(&mut self, msg: &MSG) -> Option<Outcome> {
        let vk = VIRTUAL_KEY(msg.wParam.0 as u16);
        let first_press = (msg.lParam.0 >> 30) & 1 == 0;
        self.alt = vk == VK_MENU && first_press;
        if !self.cues {
            self.cues = true; // the letters to type show from now on, as in Windows
            (0..self.levels.len()).for_each(|l| self.paint(l));
        }
        let l = self.levels.len() - 1;
        let (items, selected) = (self.levels[l].items, self.levels[l].selected);
        let bar = self.options.bar.as_ref().map(|bar| (bar.index, bar.count));
        match vk {
            VK_DOWN | VK_UP => self.select(l, step(items, selected, vk == VK_DOWN).or(selected)),
            VK_HOME => self.select(l, step(items, None, true)),
            VK_END => self.select(l, step(items, None, false)),
            VK_RIGHT => match (selected, bar) {
                (Some(i), _) if matches!(items[i], Item::Sub(..)) => self.open_sub(l, i, true),
                (_, Some((index, count))) => return Some(Outcome::Switch((index + 1) % count, true)),
                _ => {}
            },
            VK_LEFT => match bar {
                _ if l > 0 => self.close_after(l - 1),
                Some((index, count)) => return Some(Outcome::Switch((index + count - 1) % count, true)),
                None => {}
            },
            VK_RETURN | VK_SPACE => return selected.and_then(|i| self.activate(l, i)),
            VK_ESCAPE if l > 0 => self.close_after(l - 1),
            VK_ESCAPE | VK_F10 => return Some(Outcome::Closed),
            VK_MENU => {}
            _ => {
                // SAFETY: the letter comes as WM_CHAR (see `letter`)
                unsafe { let _ = TranslateMessage(msg); }
            }
        }
        None
    }

    /// A letter typed: the line it's underlined in chooses it (if more have it: the next of them).
    pub fn letter(&mut self, c: char) -> Option<Outcome> {
        if c.is_control() {
            return None;
        }
        let l = self.levels.len() - 1;
        let (i, only) = by_letter(self.levels[l].items, self.levels[l].selected, c)?;
        self.select(l, Some(i));
        if only { self.activate(l, i) } else { None }
    }

    /// Line `i` of menu `l` chosen: its sub-menu opens, or it's what was chosen.
    fn activate(&mut self, l: usize, i: usize) -> Option<Outcome> {
        let item = &self.levels[l].items[i];
        if let Item::Sub(..) = item {
            self.open_sub(l, i, true);
            return None;
        }
        item.command().map(Outcome::Chose)
    }
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
    fn letters() {
        assert_eq!(mnemonic("&Speed && limits"), Some('s'));
        assert_eq!(mnemonic("Fish && chips"), None);
        assert_eq!(mnemonic("Open &with...\tW"), Some('w'));
        assert_eq!(mnemonic("a.zip\t50% &done"), None); // (only the name counts)
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
}
