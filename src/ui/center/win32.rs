//! Windows: windows are moved and resized with the Win32 API (GTK ignores a new default size
//! once a window is shown here), and their title bars follow Windows' dark mode.

use gtk::glib::object::ObjectType;
use gtk::prelude::*;
use std::ffi::c_void;

type Hwnd = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect, // without the taskbar
    flags: u32,
}

#[link(name = "gtk-4")]
unsafe extern "C" {
    fn gdk_win32_surface_get_handle(surface: *mut c_void) -> Hwnd;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> *mut c_void;
    fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfo) -> i32;
}

const SWP_NOSIZE: u32 = 0x1;
const SWP_NOMOVE: u32 = 0x2;
const SWP_NOZORDER: u32 = 0x4;
const SWP_NOACTIVATE: u32 = 0x10;
const MONITOR_DEFAULTTONEAREST: u32 = 2;

/// The window's HWND and its outer rectangle (in physical pixels).
fn handle(window: &gtk::Window) -> Option<(Hwnd, Rect)> {
    let surface = window.native()?.surface()?;
    // SAFETY: a live GdkSurface of the Win32 backend; plain Win32 calls on its window
    unsafe {
        let hwnd = gdk_win32_surface_get_handle(surface.as_ptr().cast());
        let mut rect = Rect::default();
        (!hwnd.is_null() && GetWindowRect(hwnd, &mut rect) != 0).then_some((hwnd, rect))
    }
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *const c_void, size: u32) -> i32;
}

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_CAPTION_COLOR: u32 = 35; // Windows 11

/// Windows' title bar in its dark mode, and the colour of the window under it (Windows 11).
pub fn realized(window: &gtk::Window) {
    let Some(surface) = window.native().and_then(|n| n.surface()) else { return };
    let palette = crate::ui::style::palette();
    let (dark, caption): (i32, u32) = if palette.dark { (1, 0x202020) } else { (0, 0xf3f3f3) }; // 0x00BBGGRR
    // SAFETY: as in handle(); the values outlive the calls
    unsafe {
        let hwnd = gdk_win32_surface_get_handle(surface.as_ptr().cast());
        if hwnd.is_null() {
            return;
        }
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&dark as *const i32).cast(), 4);
        DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, (&caption as *const u32).cast(), 4);
    }
}

pub fn placed(_window: &gtk::Window) {}

/// Move the window to the middle of its monitor's work area, if its size isn't `placed` yet.
pub fn center(window: &gtk::Window, placed: (i32, i32)) -> (i32, i32) {
    let Some((hwnd, rect)) = handle(window) else { return placed };
    let size = (rect.right - rect.left, rect.bottom - rect.top);
    if size.0 <= 1 || size.1 <= 1 || size == placed {
        return placed;
    }
    // SAFETY: as above
    unsafe {
        let mut info = MonitorInfo { size: std::mem::size_of::<MonitorInfo>() as u32, ..Default::default() };
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) == 0 {
            return placed;
        }
        let area = info.work;
        let x = area.left + (area.right - area.left - size.0).max(0) / 2;
        let y = area.top + (area.bottom - area.top - size.1).max(0) / 2;
        SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    size
}

/// Make the window's content `height` tall, keeping its width and position.
pub fn set_height(window: &gtk::Window, height: i32) {
    let Some((hwnd, rect)) = handle(window) else { return };
    let scale = window.native().and_then(|n| n.surface()).map_or(1.0, |s| s.scale());
    let change = ((height - window.height()) as f64 * scale).round() as i32;
    if change == 0 {
        return;
    }
    // SAFETY: as above
    unsafe {
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, rect.right - rect.left, rect.bottom - rect.top + change,
                     SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}
