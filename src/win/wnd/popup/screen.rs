//! What menus ask of the screen and of Windows: where the mouse is, the screen's room, the
//! user's settings; and that tests' menus (TURBODM_OFFSCREEN) never open on a screen.

use crate::win::wnd::moves;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetCapture;
use windows::Win32::UI::WindowsAndMessaging::*;

/// TurboDM still has the keyboard and the menu (`root`) the mouse: not lost to another program.
pub fn in_front(root: HWND) -> bool {
    // SAFETY: plain queries
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), None) == GetCurrentThreadId() && GetCapture() == root }
}

/// A menu `width` wide at `p`, moved off every screen when testing (TURBODM_OFFSCREEN), even
/// where the mouse is (the notification area's menus, the list's).
pub fn off_screen_in_tests(mut p: POINT, width: i32) -> POINT {
    if moves::offscreen() {
        // SAFETY: plain query
        let screens = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        if p.x + width > screens {
            p.x = screens - 30_000 + p.x.rem_euclid(10_000);
        }
    }
    p
}

/// Where a mouse message happened, on the screen.
pub fn screen_point(msg: &MSG) -> POINT {
    let mut p = POINT { x: (msg.lParam.0 & 0xffff) as i16 as i32, y: ((msg.lParam.0 >> 16) & 0xffff) as i16 as i32 };
    let client = (WM_MOUSEFIRST..=WM_MOUSELAST).contains(&msg.message) && !matches!(msg.message, WM_MOUSEWHEEL | WM_MOUSEHWHEEL);
    if client {
        // SAFETY: plain conversion
        unsafe { let _ = ClientToScreen(msg.hwnd, &mut p); }
    }
    p
}

pub fn cursor() -> POINT {
    let mut p = POINT::default();
    // SAFETY: plain query
    unsafe { let _ = GetCursorPos(&mut p); }
    p
}

/// The screen's area without the taskbar, where `p` is (tests: anywhere, off the screen).
pub fn work_area(p: POINT) -> RECT {
    let far = 1 << 24;
    if moves::offscreen() {
        return RECT { left: -far, top: -far, right: far, bottom: far };
    }
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: plain queries
    unsafe {
        if !GetMonitorInfoW(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            return RECT { left: -far, top: -far, right: far, bottom: far };
        }
    }
    info.rcWork
}

/// The user asked Windows to always underline the letters to type (else: once the keyboard's used).
pub fn system_cues() -> bool {
    let mut on = BOOL(0);
    // SAFETY: a plain query into our variable
    unsafe { let _ = SystemParametersInfoW(SPI_GETKEYBOARDCUES, 0, Some((&raw mut on).cast()), Default::default()); }
    on.as_bool()
}
