//! The sidebar's scrolling, for when its rows don't all fit (a short window, a screen scaled up):
//! the room they have above the free space, and Mint's slim scroll bar at the right.

use super::{rows, State, FOOTER};
use crate::win::wnd::canvas::{self, Mouse};
use crate::win::wnd::px;
use windows::Win32::Foundation::{HWND, RECT};

/// Where the rows end: above the free space, if it's shown (canvas pixels).
pub(super) fn rows_bottom(hwnd: HWND, state: &State) -> i32 {
    let mut r = RECT::default();
    // SAFETY: plain query
    unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut r); }
    r.bottom - if state.disk.is_some() { px(FOOTER) } else { 0 }
}

/// How tall the rows are, all of them (canvas pixels).
pub(super) fn content() -> i32 {
    rows(0).last().map_or(0, |(_, _, bottom)| bottom) + px(4)
}

/// How much taller the rows are than the room for them (canvas pixels; 0: they fit).
pub(super) fn overflow(hwnd: HWND, state: &State) -> i32 {
    (content() - rows_bottom(hwnd, state)).max(0)
}

/// Mint's slim scroll thumb at the right, when the rows don't all fit in `room` (canvas pixels):
/// wider while the mouse is on the bar.
pub(super) fn thumb(width: i32, room: i32, scroll: i32, wide: bool) -> Option<RECT> {
    let content = content();
    if content <= room {
        return None;
    }
    let length = (room * room / content).max(px(28));
    let top = (room - length) * scroll.min(content - room) / (content - room);
    let w = px(if wide { 8 } else { 4 });
    Some(RECT { left: width - w - px(3), top: top + px(3), right: width - px(3), bottom: top + length - px(3) })
}

/// The mouse on the scroll bar (the strip at the right, while the rows don't all fit): hover,
/// dragging the thumb, a click above or below it (a page). True if it was the bar's.
pub(super) fn scroll_bar(hwnd: HWND, state: &mut State, event: &Mouse) -> bool {
    let mut r = RECT::default();
    // SAFETY: plain query
    unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut r); }
    let (room, max) = (rows_bottom(hwnd, state), overflow(hwnd, state));
    let on_bar = |x: i32, y: i32| max > 0 && x >= r.right - px(14) && y < room;
    let hot = match *event {
        Mouse::Move(x, y) | Mouse::Down(x, y) | Mouse::Up(x, y) | Mouse::Double(x, y) => on_bar(x, y) || state.drag.is_some(),
        Mouse::Leave => state.drag.is_some(),
    };
    let mut changed = state.bar_hot != hot;
    state.bar_hot = hot;
    let scroll = state.scroll.min(max);
    let handled = match *event {
        Mouse::Down(x, y) | Mouse::Double(x, y) if on_bar(x, y) => {
            match thumb(r.right, room, scroll, true) {
                Some(t) if y >= t.top && y < t.bottom => state.drag = Some((y, scroll)),
                Some(t) => state.scroll = (scroll + if y < t.top { -room } else { room }).clamp(0, max),
                None => {}
            }
            true
        }
        Mouse::Move(_, y) if state.drag.is_some() => {
            let (from, start) = state.drag.unwrap_or_default();
            let length = thumb(r.right, room, scroll, true).map_or(room, |t| t.bottom - t.top + px(6));
            let moved = (y - from) * max / (room - length).max(1);
            state.scroll = (start + moved).clamp(0, max);
            true
        }
        Mouse::Up(..) if state.drag.is_some() => {
            state.drag = None;
            true
        }
        _ => false,
    };
    changed |= state.scroll != scroll;
    if changed || handled {
        canvas::redraw(hwnd);
    }
    handled
}
