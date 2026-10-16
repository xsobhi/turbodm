//! Scroll bars as Mint-Y draws them, on Windows' own controls (the downloads list, a download's
//! connections, a long text field): Windows' bar is cut out of the control (which scrolls as
//! before: keys, the wheel, the selection, rows coming and going) and a slim bar of TurboDM's
//! (bar.rs) shows in its place, following the control however it scrolls.

mod bar;
#[cfg(test)]
mod tests;
mod thumb;
mod window;

use bar::Bar;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, CreateRectRgn, MapWindowPoints, SetWindowRgn};
use windows::Win32::UI::Controls::{LVM_GETHEADER, WM_MOUSEHOVER, WM_MOUSELEAVE};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

const SUBCLASS: usize = 0x5C01;

/// What's laid out: which of Windows' bars the control has, its inside, where it is.
#[derive(Clone, Copy, PartialEq)]
struct Shape {
    bars: (bool, bool), // vertical, horizontal
    inside: RECT,       // its client area, in its window's pixels (Windows' bars are outside it)
    at: RECT,           // the control, in its parent's pixels
    shown: bool,
}

struct Slim {
    bars: [Rc<Bar>; 2], // vertical, horizontal
    list: bool,         // a list view (with column headers)
    shape: Cell<Option<Shape>>,
    busy: Cell<bool>,
}

thread_local! {
    static SLIM: RefCell<HashMap<isize, Rc<Slim>>> = RefCell::default();
}

/// Give `control` (a list view, a multi-line text field) slim scroll bars.
pub fn slim(control: HWND) {
    let mut class = [0u16; 32];
    // SAFETY: plain query into our buffer
    let n = unsafe { GetClassNameW(control, &mut class) }.max(0) as usize;
    let list = String::from_utf16_lossy(&class[..n]) == "SysListView32";
    let bars = [Bar::new(control, true, list), Bar::new(control, false, list)];
    let slim = Slim { bars, list, shape: Cell::new(None), busy: Cell::new(false) };
    SLIM.with(|s| s.borrow_mut().insert(control.0 as isize, Rc::new(slim)));
    // SAFETY: a subclass of our own control, removed when it goes
    unsafe { let _ = SetWindowSubclass(control, Some(subclass), SUBCLASS, 0); }
    sync(control);
}

extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    // SAFETY: the control's own handling first
    let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    match msg {
        WM_NCDESTROY => {
            if let Some(slim) = SLIM.with(|s| s.borrow_mut().remove(&(hwnd.0 as isize))) {
                // SAFETY: ours, made in `slim`
                unsafe {
                    let _ = RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS);
                    slim.bars.iter().for_each(|bar| { let _ = DestroyWindow(bar.hwnd); });
                }
            }
        }
        // (often, and never a scroll)
        WM_MOUSEMOVE | WM_SETCURSOR | WM_NCHITTEST | WM_GETOBJECT | WM_MOUSEHOVER | WM_MOUSELEAVE => {}
        _ => sync(hwnd),
    }
    result
}

/// Windows' bars cut out and ours in their place, if the control's shape changed; ours show
/// where it's scrolled to.
fn sync(control: HWND) {
    let Some(slim) = SLIM.with(|s| s.borrow().get(&(control.0 as isize)).cloned()) else { return };
    if slim.busy.replace(true) {
        return; // (cutting it out sends it messages)
    }
    let shape = shape(control);
    if slim.shape.replace(Some(shape)) != Some(shape) {
        let Shape { bars: (v, h), inside, at, shown } = shape;
        // SAFETY: the region is the control's from now on (Windows frees it)
        unsafe {
            let region = (v || h).then(|| CreateRectRgn(inside.left, inside.top, inside.right, inside.bottom));
            let _ = SetWindowRgn(control, region, true);
        }
        let (right, bottom) = (at.left + inside.right, at.top + inside.bottom);
        let vertical = RECT { left: right, top: at.top, right: at.right, bottom: at.bottom };
        let horizontal = RECT { left: at.left, top: bottom, right, bottom: at.bottom };
        let header = if slim.list { header(control) } else { 0 };
        slim.bars[0].place((v && shown).then_some(vertical), header + inside.top);
        slim.bars[1].place((h && shown).then_some(horizontal), 0);
    }
    slim.bars.iter().for_each(|bar| bar.update());
    slim.busy.set(false);
}

fn shape(control: HWND) -> Shape {
    // SAFETY: plain queries
    unsafe {
        let style = GetWindowLongPtrW(control, GWL_STYLE) as u32;
        let (mut inside, mut at) = (RECT::default(), RECT::default());
        let _ = GetClientRect(control, &mut inside);
        let _ = GetWindowRect(control, &mut at);
        let mut origin = POINT::default();
        let _ = ClientToScreen(control, &mut origin);
        let (dx, dy) = (origin.x - at.left, origin.y - at.top);
        let inside = RECT { left: dx, top: dy, right: dx + inside.right, bottom: dy + inside.bottom };
        let mut corners = [POINT { x: at.left, y: at.top }, POINT { x: at.right, y: at.bottom }];
        MapWindowPoints(None, GetParent(control).ok(), &mut corners);
        let at = RECT { left: corners[0].x, top: corners[0].y, right: corners[1].x, bottom: corners[1].y };
        Shape { bars: (style & WS_VSCROLL.0 != 0, style & WS_HSCROLL.0 != 0), inside, at,
                shown: IsWindowVisible(control).as_bool() }
    }
}

/// How tall a list view's column headers are (its vertical bar starts below them).
fn header(control: HWND) -> i32 {
    let header = HWND(super::send(control, LVM_GETHEADER, 0, 0) as *mut _);
    let mut r = RECT::default();
    // SAFETY: plain queries
    unsafe {
        if header.is_invalid() || !IsWindowVisible(header).as_bool() || GetWindowRect(header, &mut r).is_err() {
            return 0;
        }
    }
    r.bottom - r.top
}
