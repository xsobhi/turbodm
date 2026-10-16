//! The keyboard in a menu, as in Windows': Up and Down go along the lines (over greyed ones),
//! Right opens a sub-menu (or the bar's next menu), Left goes back, Enter chooses, Escape closes
//! one menu, Alt or F10 all of them; a line's underlined letter chooses it.

use super::layout;
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
            VK_DOWN | VK_UP => self.select(l, layout::step(items, selected, vk == VK_DOWN).or(selected)),
            VK_HOME => self.select(l, layout::step(items, None, true)),
            VK_END => self.select(l, layout::step(items, None, false)),
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
        let (i, only) = layout::by_letter(self.levels[l].items, self.levels[l].selected, c)?;
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
