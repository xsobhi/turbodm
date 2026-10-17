//! Placing controls (in 96-DPI pixels, scaled to the screen) and windows on the screen.

use super::moves::{content_top, edge, move_window};
use super::{field, px, send};
use windows::Win32::UI::Controls::BCM_GETIDEALSIZE;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::core::BOOL;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Standard heights (Fluent's): a text field, button or drop-down; a line of text.
pub const FIELD: i32 = 30;
pub const LINE: i32 = 18;
pub const BUTTON_WIDTH: i32 = 96;

/// An area given in 96-DPI pixels, in the screen's.
pub fn rect(x: i32, y: i32, width: i32, height: i32) -> RECT {
    RECT { left: px(x), top: px(y), right: px(x) + px(width), bottom: px(y) + px(height) }
}

/// Put a control at (x, y), `width` × `height` (96-DPI pixels, below its window's title bar);
/// a text field goes in its frame.
pub fn place(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
    // SAFETY: plain query
    let top = unsafe { GetParent(hwnd) }.map(content_top).unwrap_or(0);
    let mut r = rect(x, y, width, height);
    (r.top, r.bottom) = (r.top + top, r.bottom + top);
    if !field::place_field(hwnd, r) {
        move_window(hwnd, r);
    }
}

/// A button wide enough for its text (at least the standard width), in 96-DPI pixels.
pub fn button_width(button: HWND) -> i32 {
    let mut size = windows::Win32::Foundation::SIZE::default();
    send(button, BCM_GETIDEALSIZE, 0, &mut size as *mut _ as isize);
    (size.cx * 96 / px(96) + 24).max(BUTTON_WIDTH)
}

/// The window's inside, in 96-DPI pixels.
pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: plain query
    unsafe { let _ = GetClientRect(hwnd, &mut rect); }
    let edges = 2 * edge(hwnd); // (inside the line it draws around itself)
    ((rect.right - edges) * 96 / px(96), (rect.bottom - edges) * 96 / px(96))
}

/// Resize the window so its inside is `height` (96-DPI pixels) tall below its title bar, keeping
/// its width.
pub fn set_client_height(hwnd: HWND, height: i32) {
    let (mut outer, mut inner) = (RECT::default(), RECT::default());
    // SAFETY: plain calls on our window
    unsafe {
        let _ = GetWindowRect(hwnd, &mut outer);
        let _ = GetClientRect(hwnd, &mut inner);
        let frame = (outer.bottom - outer.top) - inner.bottom;
        let _ = SetWindowPos(hwnd, None, 0, 0, outer.right - outer.left, px(height) + content_top(hwnd) + 2 * edge(hwnd) + frame,
                             SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Put the window in the middle of the screen the mouse is on (its work area: no taskbar).
pub fn center(hwnd: HWND) {
    // SAFETY: plain queries and a move of our own window
    unsafe {
        if super::moves::offscreen() {
            let _ = SetWindowPos(hwnd, None, -30000, 0, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            return;
        }
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            return;
        }
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let (w, h, area) = (rect.right - rect.left, rect.bottom - rect.top, info.rcWork);
        let x = area.left + ((area.right - area.left - w) / 2).max(0);
        let y = area.top + ((area.bottom - area.top - h) / 2).max(0);
        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// A form: rows of a label and a field, top to bottom.
pub struct Rows {
    pub y: i32,
    left: i32,
    label_width: i32,
    width: i32,
}

impl Rows {
    /// Starting at (`left`, `top`), `width` wide, labels taking `label_width` of it.
    pub fn new(left: i32, top: i32, label_width: i32, width: i32) -> Self {
        Rows { y: top, left, label_width, width }
    }

    /// A labelled field `height` tall (the label beside its first line).
    pub fn field(&mut self, label: Option<HWND>, field: HWND, height: i32) {
        if let Some(label) = label {
            place(label, self.left, self.y + (height.min(FIELD) - LINE) / 2, self.label_width - 12, LINE);
        }
        place(field, self.left + self.label_width, self.y, self.width - self.label_width, height);
        self.y += height + 10;
    }

    /// Something across the whole width.
    pub fn full(&mut self, hwnd: HWND, height: i32) {
        place(hwnd, self.left, self.y, self.width, height);
        self.y += height + 8;
    }

    pub fn gap(&mut self, pixels: i32) {
        self.y += pixels;
    }

    /// Buttons on the right, on one row; returns the y below them.
    pub fn buttons(&mut self, buttons: &[HWND]) -> i32 {
        let mut x = self.left + self.width;
        for button in buttons.iter().rev() {
            let width = button_width(*button);
            x -= width;
            place(*button, x, self.y, width, FIELD + 2);
            x -= 8;
        }
        self.y += FIELD + 2;
        self.y
    }
}

/// Any of TurboDM's windows is on screen.
pub fn any_window_shown() -> bool {
    extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` points at the bool below, alive during the enumeration
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                *(found.0 as *mut bool) = true;
            }
        }
        true.into()
    }
    let mut found = false;
    // SAFETY: enumerates this thread's windows into `found`
    unsafe { let _ = EnumThreadWindows(GetCurrentThreadId(), Some(visit), LPARAM(&mut found as *mut bool as isize)); }
    found
}
