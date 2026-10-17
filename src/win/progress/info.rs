//! The "Download status" tab's rows (names on the left, values on the right, as on Linux) and
//! the overall progress bar: drawn by TurboDM, double-buffered, so they update many times a
//! second without flickering or going blank.

use super::super::theme::{self, Align, Font};
use super::super::wnd::canvas;
use super::super::wnd::{px, Window};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{DT_END_ELLIPSIS, DT_NOPREFIX, DT_PATH_ELLIPSIS, DT_SINGLELINE, DT_VCENTER};

pub const FIELDS: [&str; 8] = ["Address", "Saved to", "Status", "File size", "Downloaded", "Transfer rate",
                               "Time left", "Resume capability"];
/// A torrent's (it can always be resumed; its peers are what's worth knowing).
pub const TORRENT_FIELDS: [&str; 8] = ["Address", "Saved to", "Status", "Size", "Downloaded", "Transfer rate",
                                       "Time left", "Peers"];
/// A row's height, the names' width (96-DPI pixels).
pub const ROW: i32 = 24;
const NAMES: i32 = 128;

/// The rows: (value, its colour) for each of `names` (FIELDS or TORRENT_FIELDS).
pub struct Info {
    pub hwnd: HWND,
    values: Rc<RefCell<Vec<(String, COLORREF)>>>,
}

impl Info {
    pub fn new(window: &Window, names: &'static [&'static str; 8]) -> Self {
        let values: Rc<RefCell<Vec<(String, COLORREF)>>> = Rc::new(RefCell::new(vec![Default::default(); names.len()]));
        let v = values.clone();
        let hwnd = window.canvas(move |p| {
            p.fill(p.rect, theme::view());
            for (i, (name, (value, color))) in names.iter().zip(v.borrow().iter()).enumerate() {
                let (top, bottom) = (px(ROW) * i as i32, px(ROW) * (i as i32 + 1));
                p.text(RECT { left: 0, top, right: px(NAMES), bottom }, name, Font::Body, theme::dim(), Align::Right);
                let r = RECT { left: px(NAMES + 14), top, right: p.rect.right, bottom };
                let cut = if i == 1 { DT_PATH_ELLIPSIS } else { DT_END_ELLIPSIS }; // a path keeps its file name
                p.draw_text(r, value, Font::Body, *color, cut | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
            }
        });
        Info { hwnd, values }
    }

    pub fn set(&self, values: Vec<(String, COLORREF)>) {
        if *self.values.borrow() != values {
            *self.values.borrow_mut() = values;
            canvas::redraw(self.hwnd);
        }
    }
}

/// The overall progress: its words over a bar, like the Linux window's.
pub struct Bar {
    pub hwnd: HWND,
    shown: Rc<RefCell<(f64, String)>>,
}

impl Bar {
    pub fn new(window: &Window) -> Self {
        let shown: Rc<RefCell<(f64, String)>> = Rc::default();
        let s = shown.clone();
        let hwnd = window.canvas(move |p| {
            let (fraction, text) = &*s.borrow();
            p.fill(p.rect, theme::view());
            let r = p.rect;
            p.text(RECT { bottom: r.top + px(16), ..r }, text, Font::Caption, theme::dim(), Align::Center);
            let bar = RECT { top: r.bottom - px(4), ..r };
            p.round(bar, px(2), Some(theme::track()), None);
            let done = ((bar.right - bar.left) as f64 * fraction.clamp(0.0, 1.0)).round() as i32;
            if done > 0 {
                p.round(RECT { right: bar.left + done, ..bar }, px(2), Some(theme::accent()), None);
            }
        });
        Bar { hwnd, shown }
    }

    /// Straight to `fraction` (Windows' own bar creeps there, behind the real progress).
    pub fn set(&self, fraction: f64, text: String) {
        let width = self.width() as f64;
        let changed = {
            let (old, old_text) = &*self.shown.borrow();
            (old * width).round() != (fraction * width).round() || *old_text != text
        };
        if changed {
            *self.shown.borrow_mut() = (fraction, text);
            canvas::redraw(self.hwnd);
        }
    }

    fn width(&self) -> i32 {
        let mut r = RECT::default();
        // SAFETY: plain query
        unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(self.hwnd, &mut r); }
        r.right
    }
}
