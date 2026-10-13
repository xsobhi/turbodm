//! A window with a title bar of its own (the main window's, as GTK's header bar on Linux): Windows
//! keeps the window's edges (resizing, snapping, its shadow), the window's inside reaches its
//! top, and the top of it acts as the caption (drag, double-click, the system menu).

use super::{px, Window};
use windows::Win32::Foundation::{LRESULT, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::*;

/// How thick Windows' resizing edge is (screen pixels).
pub fn edge() -> i32 {
    // SAFETY: plain queries
    unsafe { GetSystemMetrics(SM_CYFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER) }
}

impl Window {
    /// Take over the title bar: the top `height` (96-DPI pixels) of the window's inside is the
    /// caption wherever no control is (controls on it that should let the caption have the mouse
    /// say so: `canvas::on_hit`).
    pub fn own_title_bar(&self, height: i32) {
        let hwnd = self.hwnd();
        self.hook(move |msg, wparam, lparam| match msg {
            // Windows' frame on the sides and bottom, none at the top (its caption is ours)
            WM_NCCALCSIZE if wparam.0 != 0 => unsafe {
                // SAFETY: with wparam set, lparam points at an NCCALCSIZE_PARAMS
                let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                let top = params.rgrc[0].top;
                let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                // maximized, the window reaches past the screen by its edge: keep the bar on it
                params.rgrc[0].top = top + if IsZoomed(hwnd).as_bool() { edge() } else { 0 };
                Some(result)
            },
            WM_NCHITTEST => unsafe {
                // SAFETY: plain calls on our window
                let hit = DefWindowProcW(hwnd, msg, wparam, lparam);
                if hit.0 != HTCLIENT as isize {
                    return Some(hit);
                }
                let mut point = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut point);
                if !IsZoomed(hwnd).as_bool() && point.y < edge() {
                    let (width, _) = super::client_size(hwnd);
                    let corner = edge() * 2;
                    let at = if point.x < corner { HTTOPLEFT } else if point.x >= px(width) - corner { HTTOPRIGHT } else { HTTOP };
                    return Some(LRESULT(at as isize));
                }
                (point.y < px(height)).then_some(LRESULT(HTCAPTION as isize))
            },
            _ => None,
        });
        // SAFETY: plain call on our window: lay out its frame again, with the hooks above
        unsafe {
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}
