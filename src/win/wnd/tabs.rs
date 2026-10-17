//! Tabs as GTK draws them on Linux Mint: names in a row on a grey strip, the chosen one with a
//! line in Mint's green under it.

use super::canvas::{self, Mouse};
use super::{px, Window};
use crate::win::theme::{self, measure, Align, Font, Painter};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, RECT};

/// Height of the row of tabs (96-DPI pixels).
pub const TABS: i32 = 44;

#[derive(Default)]
struct State {
    picked: usize,
    hover: Option<usize>,
}

/// Each tab's area (canvas pixels).
fn areas(names: &[&str]) -> Vec<RECT> {
    let mut x = px(12);
    names.iter().map(|name| {
        let width = measure(name, Font::Body) + px(24);
        x += width + px(4);
        RECT { left: x - width - px(4), top: 0, right: x - px(4), bottom: px(TABS) }
    }).collect()
}

impl Window {
    /// A row of tabs; `on_pick` runs with the tab picked (the first one is, at first).
    pub fn tabs(&self, names: &'static [&'static str], on_pick: impl Fn(usize) + 'static) -> HWND {
        let state: Rc<RefCell<State>> = Rc::default();
        let s = state.clone();
        let hwnd = self.canvas(move |p| paint(p, names, &s.borrow()));
        canvas::on_mouse(hwnd, move |event| {
            let at = |x: i32, y: i32| areas(names).iter().position(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom);
            let (hover, picked) = match event {
                Mouse::Move(x, y) | Mouse::Up(x, y) => (at(x, y), None),
                Mouse::Down(x, y) | Mouse::Double(x, y) => (at(x, y), at(x, y)),
                Mouse::Leave => (None, None),
            };
            let mut state = state.borrow_mut();
            let changed = picked.filter(|&i| i != state.picked);
            if state.hover != hover || changed.is_some() {
                state.hover = hover;
                state.picked = changed.unwrap_or(state.picked);
                canvas::redraw(hwnd);
            }
            drop(state);
            if let Some(i) = changed {
                on_pick(i);
            }
        });
        hwnd
    }
}

fn paint(p: &Painter, names: &[&str], state: &State) {
    p.fill(p.rect, theme::chrome());
    let bottom = p.rect.bottom;
    for (i, r) in areas(names).into_iter().enumerate() {
        let (picked, hot) = (i == state.picked, state.hover == Some(i));
        p.text(r, names[i], Font::Body, if picked || hot { theme::text() } else { theme::dim() }, Align::Center);
        // a line in Mint's green under the chosen one (a grey one under the one the mouse is on)
        let line = |height: i32, color| p.fill(RECT { left: r.left, top: bottom - px(height), right: r.right, bottom }, color);
        if picked {
            line(3, theme::accent());
        } else if hot {
            line(2, theme::border());
        }
    }
}
