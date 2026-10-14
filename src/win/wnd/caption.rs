//! Title bars of TurboDM's own, on all its windows, as GTK's on Linux Mint: the window's title in
//! the middle, its buttons on the right (close in a green circle); dimmed while another window
//! is active. (The main window also puts its search and menu in its bar: main/mod.rs.)

use super::canvas::{self, Mouse};
use super::moves::move_window;
use super::{px, text, Window};
use crate::win::theme::{self, Align, Font, Painter};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The bar's height; a window button's width (96-DPI pixels).
pub const HEIGHT: i32 = 46;
const BUTTON: i32 = 40;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Minimize,
    Maximize,
    Close,
}

#[derive(Default)]
struct State {
    hover: Option<usize>,
    down: Option<usize>,
}

pub struct Caption {
    pub title: HWND,
    pub buttons: HWND,
    count: i32,
    width: Rc<Cell<i32>>, // the window's (the title is in its middle)
}

impl Caption {
    pub fn new(window: &Window, kinds: &'static [Button]) -> Self {
        let hwnd = window.hwnd();
        let (active, width) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(0)));
        let (a, w) = (active.clone(), width.clone());
        let title = window.canvas(move |p| {
            p.fill(p.rect, theme::CHROME);
            let r = RECT { left: px(12), right: w.get() - px(12), ..p.rect }; // centred in the window
            p.text(r, &text(hwnd), Font::Strong, if a.get() { theme::TEXT } else { theme::DIM }, Align::Center);
        });
        canvas::on_hit(title, |_, _| false); // all of it is the caption: drag, double-click
        let state: Rc<RefCell<State>> = Rc::default();
        let (s, a) = (state.clone(), active.clone());
        let buttons = window.canvas(move |p| paint(p, kinds, &s.borrow(), a.get(), hwnd));
        let at = move |x: i32, y: i32| (y >= 0 && y < px(HEIGHT) && x >= 0).then(|| (x / px(BUTTON)) as usize)
            .filter(|&i| i < kinds.len());
        canvas::on_hit(buttons, move |x, y| at(x, y).is_some());
        canvas::on_mouse(buttons, move |event| {
            let mut state = state.borrow_mut();
            let before = (state.hover, state.down);
            let mut pressed = None;
            match event {
                Mouse::Move(x, y) => state.hover = at(x, y),
                Mouse::Leave => state.hover = None,
                Mouse::Down(x, y) | Mouse::Double(x, y) => state.down = at(x, y),
                Mouse::Up(x, y) => pressed = state.down.take().filter(|&i| at(x, y) == Some(i)),
            }
            if (state.hover, state.down) != before {
                canvas::redraw(buttons);
            }
            drop(state);
            // SAFETY: plain query
            let zoomed = unsafe { IsZoomed(hwnd) }.as_bool();
            let command = match pressed.map(|i| kinds[i]) {
                Some(Button::Minimize) => SC_MINIMIZE,
                Some(Button::Maximize) if zoomed => SC_RESTORE,
                Some(Button::Maximize) => SC_MAXIMIZE,
                Some(Button::Close) => SC_CLOSE,
                None => return,
            };
            // SAFETY: asks our own window to do what Windows' caption buttons would
            unsafe { let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(command as usize), LPARAM(0)); }
        });
        // dimmed while another window is active, as Windows' own title bars are
        window.hook(move |msg, wparam, _| {
            if msg == WM_ACTIVATE {
                active.set((wparam.0 & 0xffff) as u32 != WA_INACTIVE);
                canvas::redraw(title);
                canvas::redraw(buttons);
            }
            None
        });
        Caption { title, buttons, count: kinds.len() as i32, width }
    }

    /// How wide the buttons are (screen pixels).
    pub fn buttons_width(&self) -> i32 {
        px(self.count * BUTTON + 8)
    }

    /// The title from the left to `title_right`, the buttons at the right end (screen pixels).
    pub fn place(&self, width: i32, title_right: i32) {
        self.width.set(width);
        move_window(self.title, RECT { left: 0, top: 0, right: title_right, bottom: px(HEIGHT) });
        move_window(self.buttons, RECT { left: width - self.buttons_width(), top: 0, right: width, bottom: px(HEIGHT) });
        canvas::redraw(self.title);
        canvas::redraw(self.buttons); // maximized or not: another symbol
    }
}

fn paint(p: &Painter, kinds: &[Button], state: &State, active: bool, window: HWND) {
    p.fill(p.rect, theme::CHROME);
    // SAFETY: plain query
    let zoomed = unsafe { IsZoomed(window) }.as_bool();
    for (i, kind) in kinds.iter().enumerate() {
        let r = RECT { left: px(BUTTON) * i as i32, top: 0, right: px(BUTTON) * (i as i32 + 1), bottom: px(HEIGHT) };
        let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
        let circle = |radius: i32| RECT { left: cx - radius, top: cy - radius, right: cx + radius, bottom: cy + radius };
        let (hot, down) = (state.hover == Some(i), state.down == Some(i));
        let glyph = match kind {
            Button::Minimize => '\u{E921}',
            Button::Maximize if zoomed => '\u{E923}',
            Button::Maximize => '\u{E922}',
            Button::Close => '\u{E8BB}',
        };
        if *kind == Button::Close {
            // Mint's close button: a circle in the accent colour
            let fill = match (active, hot || down) {
                (_, true) => theme::mix(theme::ACCENT, theme::WHITE, 0.15),
                (true, false) => theme::ACCENT,
                (false, false) => theme::mix(theme::ACCENT, theme::CHROME, 0.5),
            };
            p.round(circle(px(11)), px(11), Some(fill), None);
            p.icon(circle(px(11)), glyph, 8, theme::WHITE);
        } else {
            if hot {
                p.round(circle(px(14)), px(14), Some(if down { theme::pressed(theme::CHROME) } else { theme::hover(theme::CHROME) }), None);
            }
            p.icon(circle(px(14)), glyph, 9, if active { theme::TEXT } else { theme::DIM });
        }
    }
}
