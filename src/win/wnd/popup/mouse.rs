//! The mouse in a menu, as in Windows' (and GTK's): the line under it lights up, a sub-menu opens
//! after a moment on its line (at once when clicked), releasing a button on a line chooses it,
//! a click anywhere else closes the menus. From a menu bar, the mouse on another of its names
//! opens that menu instead.

use super::track::Menu;
use super::{Item, Outcome};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::*;

fn inside(r: RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

impl Menu<'_> {
    /// A mouse message (`msg`) at `p` on the screen; `Some`: the menus are done.
    pub fn mouse(&mut self, msg: u32, p: POINT) -> Option<Outcome> {
        match msg {
            WM_MOUSEMOVE | WM_NCMOUSEMOVE => self.moved(p),
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK
            | WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN | WM_NCMBUTTONDOWN | WM_NCLBUTTONDBLCLK | WM_NCRBUTTONDBLCLK => self.pressed(p),
            WM_LBUTTONUP | WM_RBUTTONUP | WM_NCLBUTTONUP | WM_NCRBUTTONUP => self.released(p),
            _ => None, // (the wheel, the other buttons: nothing)
        }
    }

    /// The menu under `p` (the last opened, if they overlap).
    fn level_at(&self, p: POINT) -> Option<usize> {
        self.levels.iter().rposition(|level| inside(level.surface.body, p))
    }

    /// The line of menu `l` under `p`, if it can be chosen.
    fn item_at(&self, l: usize, p: POINT) -> Option<usize> {
        let level = &self.levels[l];
        let body = level.surface.body;
        level.geo.hit(p.x - body.left, p.y - body.top).filter(|&i| level.items[i].enabled())
    }

    /// The bar's other menu under `p`, if this one came from a menu bar.
    fn other_on_bar(&self, p: POINT) -> Option<usize> {
        let bar = self.options.bar.as_ref()?;
        (bar.at)(p).filter(|&i| i != bar.index)
    }

    fn moved(&mut self, p: POINT) -> Option<Outcome> {
        if p == self.last {
            return None; // (Windows says so when a window appears under a still mouse)
        }
        self.last = p;
        let Some(l) = self.level_at(p) else {
            if let Some(i) = self.other_on_bar(p) {
                return Some(Outcome::Switch(i, false));
            }
            // off the menus: lit only the lines whose sub-menus are open
            self.not_later();
            let last = self.levels.len() - 1;
            for k in 0..last {
                self.select(k, self.levels[k + 1].opener);
            }
            self.select(last, None);
            return None;
        };
        let item = self.item_at(l, p);
        // the parents' lines that opened this one stay lit (the mouse went across to it)
        for k in 0..l {
            self.select(k, self.levels[k + 1].opener);
        }
        if self.pending.is_some_and(|(pl, _)| pl < l) {
            self.not_later();
        }
        let sub_open = self.levels.get(l + 1).and_then(|sub| sub.opener);
        let is_sub = item.is_some_and(|i| matches!(self.levels[l].items[i], Item::Sub(..)));
        if sub_open.is_some() && sub_open == item {
            self.not_later(); // back on the line whose sub-menu is open
        } else if sub_open.is_some() || is_sub {
            self.later(l, item); // the open one closes, this one opens, in a moment
        }
        self.select(l, item);
        None
    }

    fn pressed(&mut self, p: POINT) -> Option<Outcome> {
        let Some(l) = self.level_at(p) else {
            // another of the bar's menus, or closed (a click on its own name too)
            return Some(self.other_on_bar(p).map_or(Outcome::Closed, |i| Outcome::Switch(i, false)));
        };
        if let Some(i) = self.item_at(l, p).filter(|&i| matches!(self.levels[l].items[i], Item::Sub(..))) {
            self.not_later();
            if self.levels.get(l + 1).and_then(|sub| sub.opener) != Some(i) {
                self.open_sub(l, i, false);
            }
        }
        None
    }

    fn released(&mut self, p: POINT) -> Option<Outcome> {
        let l = self.level_at(p)?;
        let i = self.item_at(l, p)?;
        self.levels[l].items[i].command().map(Outcome::Chose)
    }
}
