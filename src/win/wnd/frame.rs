//! A window with a title bar of its own (the main window's, as GTK's header bar on Linux): Windows
//! keeps the window's edges (resizing, snapping, its shadow), the window's inside reaches its
//! top, and the top of it acts as the caption (drag, double-click, the system menu).

use super::caption::{self, Button, Caption};
use super::{moves, px, Window};
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LRESULT, POINT, RECT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The window size that gives `width` × `height` of content.
pub fn outer_size(width: i32, height: i32, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE) -> (i32, i32) {
    let mut rect = RECT { left: 0, top: 0, right: width, bottom: height };
    // SAFETY: plain calculation
    unsafe { let _ = AdjustWindowRectEx(&mut rect, style, false, ex); }
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// How thick Windows' resizing edge is (screen pixels).
pub fn edge() -> i32 {
    // SAFETY: plain queries
    unsafe { GetSystemMetrics(SM_CYFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER) }
}

impl Window {
    /// Show it, with the keyboard in it (TURBODM_OFFSCREEN: shown without taking the keyboard).
    pub fn show(&self) {
        // SAFETY: plain calls on our own window
        unsafe {
            if moves::offscreen() {
                let _ = ShowWindow(self.hwnd(), SW_SHOWNOACTIVATE);
                return;
            }
            let _ = ShowWindow(self.hwnd(), SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd());
            if self.default_id.get() != 0 {
                // the keyboard starts on the main button, as in Windows' own dialogs
                if let Ok(button) = GetDlgItem(Some(self.hwnd()), self.default_id.get() as i32) {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(button));
                }
            }
        }
    }

    /// A dialog with TurboDM's own title bar (`minimize`: with a minimize button too); what's
    /// in it is laid out below the bar, as if the bar weren't there.
    pub fn dialog(title: &str, owner: Option<HWND>, size: (i32, i32), minimize: bool) -> Rc<Window> {
        let extra = if minimize { WS_MINIMIZEBOX } else { WINDOW_STYLE(0) };
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | extra;
        let window = Window::new(title, style, WINDOW_EX_STYLE(0), owner, (size.0, size.1 + caption::HEIGHT));
        moves::set_content_top(window.hwnd(), px(caption::HEIGHT));
        let kinds: &'static [Button] = if minimize { &[Button::Minimize, Button::Close] } else { &[Button::Close] };
        let bar = Rc::new(Caption::new(&window, kinds));
        window.caption.set(bar.title);
        window.hook(move |msg, _, lparam| {
            if msg == WM_SIZE {
                let width = (lparam.0 & 0xffff) as i32;
                bar.place(width, width - bar.buttons_width());
            }
            None
        });
        window.own_title_bar(caption::HEIGHT);
        window
    }

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
                let resizable = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_THICKFRAME.0 != 0;
                if resizable && !IsZoomed(hwnd).as_bool() && point.y < edge() {
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
