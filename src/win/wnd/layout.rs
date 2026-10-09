//! Placing controls (in 96-DPI pixels, scaled to the screen) and windows on the screen.

use super::{px, send};
use windows::Win32::UI::Controls::{BCM_GETIDEALSIZE, UDM_GETBUDDY, UDM_SETBUDDY};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::core::BOOL;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Standard heights: a text field, button or drop-down; a label line; a check box.
pub const FIELD: i32 = 23;
pub const LINE: i32 = 16;
pub const BUTTON_WIDTH: i32 = 88;

pub fn place(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
    // SAFETY: plain calls on our controls
    unsafe {
        let _ = SetWindowPos(hwnd, None, px(x), px(y), px(width), px(height), SWP_NOZORDER | SWP_NOACTIVATE);
        if let Some(arrows) = arrows_of(hwnd) {
            send(arrows, UDM_SETBUDDY, hwnd.0 as usize, 0); // they move along with it
        }
    }
}

/// A number field's up/down arrows (created right after it).
pub fn arrows_of(hwnd: HWND) -> Option<HWND> {
    // SAFETY: plain queries
    unsafe {
        let next = GetWindow(hwnd, GW_HWNDNEXT).ok()?;
        let mut class = [0u16; 32];
        let n = GetClassNameW(next, &mut class).max(0) as usize;
        (String::from_utf16_lossy(&class[..n]) == "msctls_updown32" && send(next, UDM_GETBUDDY, 0, 0) == hwnd.0 as isize)
            .then_some(next)
    }
}

/// A button wide enough for its text (at least the standard width), in 96-DPI pixels.
fn button_width(button: HWND) -> i32 {
    let mut size = windows::Win32::Foundation::SIZE::default();
    send(button, BCM_GETIDEALSIZE, 0, &mut size as *mut _ as isize);
    (size.cx * 96 / px(96) + 16).max(BUTTON_WIDTH)
}

/// The window's inside, in 96-DPI pixels.
pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: plain query
    unsafe { let _ = GetClientRect(hwnd, &mut rect); }
    (rect.right * 96 / px(96), rect.bottom * 96 / px(96))
}

/// Resize the window so its inside is `height` (96-DPI pixels) tall, keeping its width.
pub fn set_client_height(hwnd: HWND, height: i32) {
    let (mut outer, mut inner) = (RECT::default(), RECT::default());
    // SAFETY: plain calls on our window
    unsafe {
        let _ = GetWindowRect(hwnd, &mut outer);
        let _ = GetClientRect(hwnd, &mut inner);
        let frame = (outer.bottom - outer.top) - inner.bottom;
        let _ = SetWindowPos(hwnd, None, 0, 0, outer.right - outer.left, px(height) + frame,
                             SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Put the window in the middle of the screen the mouse is on (its work area: no taskbar).
pub fn center(hwnd: HWND) {
    // SAFETY: plain queries and a move of our own window
    unsafe {
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

    /// A labelled field `height` tall (the label sits on its first line).
    pub fn field(&mut self, label: Option<HWND>, field: HWND, height: i32) {
        if let Some(label) = label {
            place(label, self.left, self.y + 4, self.label_width - 8, LINE);
        }
        place(field, self.left + self.label_width, self.y, self.width - self.label_width, height);
        self.y += height + 8;
    }

    /// A field only as wide as `width`, after the labels.
    pub fn short(&mut self, label: Option<HWND>, field: HWND, width: i32) {
        if let Some(label) = label {
            place(label, self.left, self.y + 4, self.label_width - 8, LINE);
        }
        place(field, self.left + self.label_width, self.y, width, FIELD);
        self.y += FIELD + 8;
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
