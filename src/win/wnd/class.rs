//! The window class all of TurboDM's windows share, and its procedure: it finds the window's
//! `Window` (kept in GWLP_USERDATA) and lets it handle the message.

use super::{instance, Window, APP_ICON};
use std::rc::Rc;
use std::sync::Once;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{HBRUSH, COLOR_BTNFACE};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const CLASS: PCWSTR = w!("TurboDMWindow");

pub fn register_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: plain class registration with a static name and procedure
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance(),
            hIcon: LoadIconW(Some(instance()), PCWSTR(APP_ICON as usize as *const u16)).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as usize as *mut _),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
    });
}

extern "system" fn procedure(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds the Rc<Window> given to CreateWindowExW, until WM_NCDESTROY
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            (*(create.lpCreateParams as *const Window)).hwnd.set(hwnd);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Window;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let win = Rc::from_raw(ptr);
            win.forget();
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        Rc::increment_strong_count(ptr); // alive while handling, even if the window goes
        let win = Rc::from_raw(ptr);
        win.handle(msg, wparam, lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
    }
}
