//! A window with a title bar of its own (the main window's, as GTK's header bar on Linux): Windows
//! keeps the window's edges (resizing, snapping, its shadow), the window's inside reaches its
//! top, and the top of it acts as the caption (drag, double-click, the system menu).
//!
//! Windows 10 draws a dark line just outside a window's inside, wherever it has a frame, which on
//! TurboDM's light windows showed as a black outline on three sides (none on top, where the title
//! bar is ours). There the whole window is its inside: no frame, so no line. It draws a light one
//! of its own, the same all round (moves.rs keeps its controls inside it); Windows still gives it
//! its shadow, and its edges, a few pixels in, resize it. Windows 11 draws a light edge itself.

use super::caption::{self, Button, Caption};
use super::{moves, px, Window};
use std::rc::Rc;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The line a frameless window draws around itself (Windows 10), and how far in from it its
/// edges resize it (screen pixels; the second in 96-DPI ones).
const LINE: i32 = 1;
const GRAB: i32 = 5;

/// The window size that gives `width` × `height` of content.
pub fn outer_size(width: i32, height: i32, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE) -> (i32, i32) {
    let mut rect = RECT { left: 0, top: 0, right: width, bottom: height };
    // SAFETY: plain calculation
    unsafe { let _ = AdjustWindowRectEx(&mut rect, style, false, ex); }
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// Windows 10, not 11 (whose frame has a light edge of its own).
fn windows_10() -> bool {
    static TEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TEN.get_or_init(|| {
        let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
        let build: Option<String> = key.and_then(|k| k.get_value("CurrentBuildNumber")).ok();
        build.and_then(|b| b.parse::<u32>().ok()).is_some_and(|b| b < 22000)
    })
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

    /// Show it in front of every other program's windows: something TurboDM did by itself (a
    /// download caught from the browser, one that finished) while another program is in use.
    /// Windows may not let it take the keyboard then, so it's lifted above the others anyway.
    pub fn show_in_front(&self) {
        self.show();
        if moves::offscreen() {
            return;
        }
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        // SAFETY: plain calls on our own window: on top of all, then back among the others (first)
        unsafe {
            let _ = SetWindowPos(self.hwnd(), Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
            let _ = SetWindowPos(self.hwnd(), Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
            let _ = SetForegroundWindow(self.hwnd());
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
        let me = window.hwnd();
        window.hook(move |msg, _, lparam| {
            if msg == WM_SIZE {
                let width = (lparam.0 & 0xffff) as i32 - 2 * moves::edge(me); // (inside its edge)
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
        let frameless = windows_10();
        self.hook(move |msg, wparam, lparam| match msg {
            WM_NCCALCSIZE if wparam.0 != 0 => unsafe {
                // SAFETY: with wparam set, lparam points at an NCCALCSIZE_PARAMS
                let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                let zoomed = IsZoomed(hwnd).as_bool();
                if frameless && !zoomed {
                    // no frame: the inside is the whole window, with its own edge (see above)
                    moves::set_edge(hwnd, LINE);
                    return Some(LRESULT(0));
                }
                // Windows' frame on the sides and bottom, none at the top (its caption is ours)
                moves::set_edge(hwnd, 0);
                let top = params.rgrc[0].top;
                let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                // maximized, the window reaches past the screen by its edge: keep the bar on it
                params.rgrc[0].top = top + if zoomed { edge() } else { 0 };
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
                if let Some(at) = resize_at(hwnd, point) {
                    return Some(LRESULT(at as isize));
                }
                (point.y < px(height) + moves::edge(hwnd)).then_some(LRESULT(HTCAPTION as isize))
            },
            _ => None,
        });
        // SAFETY: plain calls on our window: lay out its frame again, with the hooks above
        unsafe {
            let mut asked = RECT::default(); // (the inside it was made with)
            let _ = GetClientRect(hwnd, &mut asked);
            if frameless {
                // its shadow without a frame: Windows draws one for a frame reaching in (by a pixel,
                // which the window paints over)
                let margins = MARGINS { cxLeftWidth: 1, cxRightWidth: 1, cyTopHeight: 1, cyBottomHeight: 1 };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            }
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            if moves::edge(hwnd) > 0 {
                // the frame gone, the same inside as asked for, with the edge around it
                let around = 2 * moves::edge(hwnd);
                let _ = SetWindowPos(hwnd, None, 0, 0, asked.right + around, asked.bottom + around,
                                     SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }
}

/// Where `point` (the window's own pixels) resizes the window, if it does: along its top (the bar
/// is ours), and without a frame (Windows 10) along every edge, a few pixels in.
fn resize_at(hwnd: HWND, point: POINT) -> Option<u32> {
    // SAFETY: plain queries on our window
    let (resizable, zoomed, size) = unsafe {
        let mut r = RECT::default();
        let _ = GetClientRect(hwnd, &mut r);
        (GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_THICKFRAME.0 != 0, IsZoomed(hwnd).as_bool(), r)
    };
    if !resizable || zoomed {
        return None;
    }
    let sides = moves::edge(hwnd) > 0; // (else Windows' frame outside resizes it)
    let grab = if sides { px(GRAB) } else { edge() };
    let corner = 2 * grab.max(edge());
    let (left, right) = (sides && point.x < grab, sides && point.x >= size.right - grab);
    let (top, bottom) = (point.y < grab, sides && point.y >= size.bottom - grab);
    let near = (point.x < corner, point.x >= size.right - corner, point.y < corner, point.y >= size.bottom - corner);
    let (near_left, near_right, near_top, near_bottom) = near;
    Some(match () {
        _ if (top && near_left) || (left && near_top) => HTTOPLEFT,
        _ if (top && near_right) || (right && near_top) => HTTOPRIGHT,
        _ if (bottom && near_left) || (left && near_bottom) => HTBOTTOMLEFT,
        _ if (bottom && near_right) || (right && near_bottom) => HTBOTTOMRIGHT,
        _ if top => HTTOP,
        _ if bottom => HTBOTTOM,
        _ if left => HTLEFT,
        _ if right => HTRIGHT,
        _ => return None,
    })
}

/// A frameless window's controls reach its edges: near one, they let the mouse through to the
/// window, which resizes from there (`resize_at`). For a resizable window's controls, once made.
pub fn let_edges_resize(window: HWND) {
    if !windows_10() {
        return;
    }
    extern "system" fn each(child: HWND, window: LPARAM) -> BOOL {
        // SAFETY: plain query, and a subclass of our own control (only the window's own children)
        unsafe {
            if GetParent(child).ok().is_some_and(|p| p.0 as isize == window.0) {
                let _ = SetWindowSubclass(child, Some(pass_edges), 0x7EDE, 0);
            }
        }
        BOOL(1)
    }
    // SAFETY: our window's children, visited now
    unsafe { let _ = EnumChildWindows(Some(window), Some(each), LPARAM(window.0 as isize)); }
}

extern "system" fn pass_edges(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    // SAFETY: plain calls; the rest goes on to the control's own procedure
    unsafe {
        if msg == WM_NCHITTEST && let Ok(window) = GetParent(hwnd) {
            let mut point = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
            let _ = ScreenToClient(window, &mut point);
            if resize_at(window, point).is_some() {
                return LRESULT(HTTRANSPARENT as isize);
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}
