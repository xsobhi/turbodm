//! Moving controls: all of a window's at once while it's laid out (one repaint: resizing stays
//! smooth, nothing smears), and the room a window's own title bar takes at the top of it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    static BATCH: Cell<isize> = const { Cell::new(0) }; // the moves being collected (HDWP)
    static TOPS: RefCell<HashMap<isize, i32>> = RefCell::default(); // window → where its content starts
}

/// `TURBODM_OFFSCREEN=1`: windows open off the screen, without taking the focus (automated
/// screenshots and tests while someone uses the computer).
pub fn offscreen() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("TURBODM_OFFSCREEN").is_some_and(|v| v == "1"))
}

/// Where a window's content starts (screen pixels): below its own title bar, if it has one.
pub fn content_top(window: HWND) -> i32 {
    TOPS.with(|t| t.borrow().get(&(window.0 as isize)).copied().unwrap_or(0))
}

pub fn set_content_top(window: HWND, top: i32) {
    TOPS.with(|t| t.borrow_mut().insert(window.0 as isize, top));
}

pub fn forget(window: HWND) {
    TOPS.with(|t| t.borrow_mut().remove(&(window.0 as isize)));
}

/// Move a control to `r` (screen pixels in its window): with the others, during `batch`.
/// Without Windows copying its old picture to the new place first (that copy is what showed
/// as ghosted text and icons while resizing): it's drawn again whole (see `paint_now`).
pub fn move_window(hwnd: HWND, r: RECT) {
    let (width, height) = (r.right - r.left, r.bottom - r.top);
    let flags = SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS;
    let batch = BATCH.with(Cell::get);
    // SAFETY: plain moves of our own controls
    unsafe {
        if batch != 0 {
            match DeferWindowPos(HDWP(batch as *mut _), hwnd, None, r.left, r.top, width, height, flags) {
                Ok(next) => return BATCH.with(|b| b.set(next.0 as isize)),
                Err(_) => BATCH.with(|b| b.set(0)), // (Windows dropped the batch: move them one by one)
            }
        }
        let _ = SetWindowPos(hwnd, None, r.left, r.top, width, height, flags);
    }
}

/// Draw what moving left to draw, now: not after the next mouse moves (while resizing or
/// dragging a splitter, the old picture would show meanwhile).
pub fn paint_now(window: HWND) {
    use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_ALLCHILDREN, RDW_UPDATENOW};
    // SAFETY: plain repaint of our own window
    unsafe { let _ = RedrawWindow(Some(window), None, None, RDW_UPDATENOW | RDW_ALLCHILDREN); }
}

/// Lay out at once: the moves `f` makes happen together when it returns.
pub fn batch(f: impl FnOnce()) {
    if BATCH.with(Cell::get) != 0 {
        return f(); // already in one
    }
    // SAFETY: Windows' deferred moves, ended below
    if let Ok(start) = unsafe { BeginDeferWindowPos(24) } {
        BATCH.with(|b| b.set(start.0 as isize));
    }
    f();
    let collected = BATCH.with(|b| b.replace(0));
    if collected != 0 {
        // SAFETY: the batch begun above
        unsafe { let _ = EndDeferWindowPos(HDWP(collected as *mut _)); }
    }
}
