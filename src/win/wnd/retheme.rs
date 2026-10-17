//! A new look (Options → Look) on windows already open: the colours they keep for parts of
//! themselves (surfaces, labels' own colours) become the new look's, their controls get its
//! font, Windows 11 rounds their corners or not, and they're laid out and drawn again.

use super::class::CLASS;
use super::Window;
use crate::win::theme::{self, themes::Palette};
use std::rc::Rc;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, RECT, WPARAM};
use windows::core::BOOL;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The look's corners and border, which Windows 11 draws around windows: square for a look
/// without round corners (Windows 10's), its edge colour for a dark look. (Windows 10 has no
/// such settings: they're ignored.)
pub fn dress(hwnd: HWND) {
    let look = theme::themes::current();
    let corners: i32 = if look.radius == 0 { 1 } else { 0 }; // DWMWCP_DONOTROUND, DWMWCP_DEFAULT
    let border: u32 = if look.dark() { look.palette.edge.0 } else { 0xFFFF_FFFF }; // DWMWA_COLOR_DEFAULT
    let dark = BOOL::from(look.dark());
    let set = |attribute: i32, value: *const std::ffi::c_void| {
        // SAFETY: a 4-byte value for an attribute of our own window
        unsafe { let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(attribute), value, 4); }
    };
    set(33, (&corners as *const i32).cast()); // DWMWA_WINDOW_CORNER_PREFERENCE
    set(34, (&border as *const u32).cast()); // DWMWA_BORDER_COLOR
    set(20, (&dark as *const BOOL).cast()); // DWMWA_USE_IMMERSIVE_DARK_MODE
}

/// The look changed from `before`'s colours: every window of TurboDM's takes the new one.
pub fn retheme(before: &Palette) {
    let after = theme::themes::current().palette;
    for hwnd in windows() {
        if let Some(win) = window_of(hwnd) {
            for (_, color) in win.surfaces.borrow_mut().iter_mut() {
                *color = translate(*color, before, &after);
            }
            for color in win.colors.borrow_mut().values_mut() {
                *color = translate(*color, before, &after);
            }
            dress(hwnd);
            relayout(hwnd);
        }
        refont(hwnd);
        // SAFETY: plain redraw of our window and all inside it
        unsafe { let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME); }
    }
}

/// The new look's colour for what was `before`'s `color` (the content's, a line's…); others stay.
fn translate(color: COLORREF, before: &Palette, after: &Palette) -> COLORREF {
    let roles = |p: &Palette| [p.view, p.chrome, p.border, p.edge, p.separator, p.text, p.dim, p.disabled, p.accent,
                                p.track, p.danger];
    roles(before).iter().position(|c| *c == color).map_or(color, |i| roles(after)[i])
}

/// The top-level windows of this thread (TurboDM's windows all live on its UI thread).
fn windows() -> Vec<HWND> {
    extern "system" fn add(hwnd: HWND, list: LPARAM) -> BOOL {
        // SAFETY: `list` points at the Vec below, alive during the enumeration
        unsafe { (*(list.0 as *mut Vec<HWND>)).push(hwnd) };
        true.into()
    }
    let mut list: Vec<HWND> = vec![];
    // SAFETY: enumerates into `list`
    unsafe { let _ = EnumThreadWindows(GetCurrentThreadId(), Some(add), LPARAM(&mut list as *mut _ as isize)); }
    list
}

/// The `Window` of one of TurboDM's windows (of its window class).
fn window_of(hwnd: HWND) -> Option<Rc<Window>> {
    let mut class = [0u16; 32];
    // SAFETY: plain queries; the window's Rc is shared, one more reference taken for us
    unsafe {
        let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
        if class[..n] != *CLASS.as_wide() {
            return None;
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Window;
        if ptr.is_null() {
            return None;
        }
        Rc::increment_strong_count(ptr);
        Some(Rc::from_raw(ptr))
    }
}

/// The controls in `hwnd` get the new look's font, as they had the old one's.
fn refont(hwnd: HWND) {
    extern "system" fn each(child: HWND, _: LPARAM) -> BOOL {
        // SAFETY: plain messages to the control
        unsafe {
            let old = HFONT_OF(SendMessageW(child, WM_GETFONT, None, None).0);
            if let Some(new) = theme::refont(old) {
                SendMessageW(child, WM_SETFONT, Some(WPARAM(new.0 as usize)), Some(LPARAM(0)));
            }
        }
        true.into()
    }
    // SAFETY: enumerates our window's controls
    unsafe { let _ = EnumChildWindows(Some(hwnd), Some(each), LPARAM(0)); }
}

#[allow(non_snake_case)]
fn HFONT_OF(handle: isize) -> windows::Win32::Graphics::Gdi::HFONT {
    windows::Win32::Graphics::Gdi::HFONT(handle as *mut _)
}

/// Its parts placed again (a Windows look's title bar buttons are wider…), as when it's resized.
fn relayout(hwnd: HWND) {
    let mut r = RECT::default();
    // SAFETY: plain query and a WM_SIZE to our own window
    unsafe {
        if GetClientRect(hwnd, &mut r).is_ok() && !IsIconic(hwnd).as_bool() {
            let size = ((r.bottom as u32 & 0xffff) << 16) | (r.right as u32 & 0xffff);
            SendMessageW(hwnd, WM_SIZE, Some(WPARAM(SIZE_RESTORED as usize)), Some(LPARAM(size as isize)));
        }
    }
}
