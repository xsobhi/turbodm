//! The main window's title bar, TurboDM's own, as GTK's header bar on Linux Mint: the title in
//! the middle, the search and the menu on the right (placed by the window), then the window's
//! buttons, close in a green circle.

use super::super::theme::{self, Align, Font, Painter};
use super::super::wnd::canvas::{self, Mouse};
use super::super::wnd::{px, Window};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The bar's height; a window button's width (96-DPI pixels).
pub const HEIGHT: i32 = 46;
const BUTTON: i32 = 40;

const MINIMIZE: char = '\u{E921}';
const MAXIMIZE: char = '\u{E922}';
const RESTORE: char = '\u{E923}';
const CLOSE: char = '\u{E8BB}';

#[derive(Default)]
struct State {
    hover: Option<usize>,
    down: Option<usize>,
}

pub struct Caption {
    title: HWND,
    pub buttons: HWND,
    width: Rc<Cell<i32>>, // the window's (the title is in its middle)
}

/// The buttons' width (screen pixels).
pub fn buttons_width() -> i32 {
    px(3 * BUTTON + 8)
}

fn button_at(x: i32, y: i32) -> Option<usize> {
    (y >= 0 && y < px(HEIGHT) && x >= 0).then(|| (x / px(BUTTON)) as usize).filter(|&i| i < 3)
}

impl Caption {
    pub fn new(window: &Rc<Window>) -> Self {
        let hwnd = window.hwnd();
        let (active, width) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(0)));
        let (a, w) = (active.clone(), width.clone());
        let title = window.canvas(move |p| {
            p.fill(p.rect, theme::CHROME);
            let r = RECT { left: 0, right: w.get(), ..p.rect }; // centred in the window
            p.text(r, "TurboDM", Font::Strong, if a.get() { theme::TEXT } else { theme::DIM }, Align::Center);
        });
        canvas::on_hit(title, |_, _| false); // all of it is the caption
        let state: Rc<RefCell<State>> = Rc::default();
        let (s, a) = (state.clone(), active.clone());
        let buttons = window.canvas(move |p| paint(p, &s.borrow(), a.get(), hwnd));
        canvas::on_hit(buttons, |x, y| button_at(x, y).is_some());
        canvas::on_mouse(buttons, move |event| {
            let mut state = state.borrow_mut();
            let before = (state.hover, state.down);
            let mut pressed = None;
            match event {
                Mouse::Move(x, y) => state.hover = button_at(x, y),
                Mouse::Leave => state.hover = None,
                Mouse::Down(x, y) | Mouse::Double(x, y) => state.down = button_at(x, y),
                Mouse::Up(x, y) => pressed = state.down.take().filter(|&i| button_at(x, y) == Some(i)),
            }
            if (state.hover, state.down) != before {
                canvas::redraw(buttons);
            }
            drop(state);
            let command = match pressed {
                // SAFETY: plain query
                Some(0) => SC_MINIMIZE,
                Some(1) if unsafe { IsZoomed(hwnd) }.as_bool() => SC_RESTORE,
                Some(1) => SC_MAXIMIZE,
                Some(_) => SC_CLOSE,
                None => return,
            };
            // SAFETY: asks our own window to do what its caption buttons would
            unsafe { let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(command as usize), LPARAM(0)); }
        });
        // dimmed while another window is active, as Windows' own title bars are
        let a = active.clone();
        window.hook(move |msg, wparam, _| {
            if msg == WM_ACTIVATE {
                a.set((wparam.0 & 0xffff) as u32 != WA_INACTIVE);
                canvas::redraw(title);
                canvas::redraw(buttons);
            }
            None
        });
        Caption { title, buttons, width }
    }

    /// The title from the left to `title_right`, the buttons at the right end (screen pixels).
    pub fn place(&self, width: i32, title_right: i32) {
        self.width.set(width);
        let place = |hwnd: HWND, r: RECT| {
            // SAFETY: plain moves of our controls
            unsafe { let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER); }
        };
        place(self.title, RECT { left: 0, top: 0, right: title_right, bottom: px(HEIGHT) });
        place(self.buttons, RECT { left: width - buttons_width(), top: 0, right: width, bottom: px(HEIGHT) });
        canvas::redraw(self.title);
        canvas::redraw(self.buttons); // maximized or not: another symbol
    }
}

fn paint(p: &Painter, state: &State, active: bool, window: HWND) {
    p.fill(p.rect, theme::CHROME);
    // SAFETY: plain query
    let zoomed = unsafe { IsZoomed(window) }.as_bool();
    for (i, glyph) in [MINIMIZE, if zoomed { RESTORE } else { MAXIMIZE }, CLOSE].into_iter().enumerate() {
        let r = RECT { left: px(BUTTON) * i as i32, top: 0, right: px(BUTTON) * (i as i32 + 1), bottom: px(HEIGHT) };
        let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
        let circle = |radius: i32| RECT { left: cx - radius, top: cy - radius, right: cx + radius, bottom: cy + radius };
        let hot = state.hover == Some(i);
        if i == 2 {
            // Mint's close button: a circle in the accent colour
            let fill = match (active, hot || state.down == Some(i)) {
                (_, true) => theme::mix(theme::ACCENT, theme::WHITE, 0.15),
                (true, false) => theme::ACCENT,
                (false, false) => theme::mix(theme::ACCENT, theme::CHROME, 0.5),
            };
            p.round(circle(px(11)), px(11), Some(fill), None);
            p.icon(circle(px(11)), glyph, 8, theme::WHITE);
        } else {
            if hot {
                let fill = if state.down == Some(i) { theme::pressed(theme::CHROME) } else { theme::hover(theme::CHROME) };
                p.round(circle(px(14)), px(14), Some(fill), None);
            }
            p.icon(circle(px(14)), glyph, 9, if active { theme::TEXT } else { theme::DIM });
        }
    }
}
