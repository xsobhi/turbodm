//! A slim scroll bar as Mint-Y draws it: no arrows, a thin rounded grey thumb on the content's
//! white, wider and darker under the mouse, Mint's green while dragged. It shows a control's
//! scroll position (mod.rs keeps it up to date) and scrolls it: dragged, clicked beside the thumb
//! (a page at a time, again and again while held), or with the wheel.

use super::thumb::Range;
use crate::win::theme::{self, Painter};
use crate::win::wnd::{instance, px, send};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Controls::{LVIR_BOUNDS, LVM_GETITEMRECT, LVM_GETTOPINDEX, LVM_SCROLL};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The thumb's width, idle and under the mouse; its gap from the edge; its shortest (96 DPI).
const SLIM: i32 = 6;
const WIDE: i32 = 10;
const GAP: i32 = 3;
const SHORTEST: i32 = 28;

pub struct Bar {
    pub hwnd: HWND,
    pub(super) control: HWND,
    vertical: bool,
    list: bool,          // a list view (scrolled with LVM_SCROLL)
    pub(super) start: Cell<i32>, // where its track starts (vertical: under the list's header)
    pub(super) range: Cell<Range>, // as drawn
    pub(super) hover: Cell<bool>,
    pub(super) drag: Cell<Option<i32>>, // dragged: where in the thumb it was taken
    pub(super) paging: Cell<i32>, // the track held: paging up (-1) or down (1)
    mouse: Cell<i32>,    // where the mouse was last, along it
}

thread_local! {
    pub(super) static BARS: RefCell<HashMap<isize, Rc<Bar>>> = RefCell::default();
}

impl Bar {
    /// A bar for `control` (hidden until placed), in the same window.
    pub fn new(control: HWND, vertical: bool, list: bool) -> Rc<Bar> {
        super::window::register();
        // SAFETY: a child of the control's window, destroyed with it (or by mod.rs)
        let hwnd = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), super::window::CLASS, PCWSTR::null(), WS_CHILD, 0, 0, 0, 0, GetParent(control).ok(), None,
                            Some(instance()), None).unwrap_or_default()
        };
        let bar = Rc::new(Bar { hwnd, control, vertical, list, start: Cell::new(0), range: Cell::default(),
                                hover: Cell::new(false), drag: Cell::new(None), paging: Cell::new(0),
                                mouse: Cell::new(0) });
        BARS.with(|b| b.borrow_mut().insert(hwnd.0 as isize, bar.clone()));
        bar
    }

    /// Put it at `r` (its window's pixels), the track starting `start` in; None: hidden.
    pub fn place(&self, r: Option<RECT>, start: i32) {
        self.start.set(start);
        let flags = SWP_NOACTIVATE | SWP_NOZORDER | if r.is_some() { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW };
        let r = r.unwrap_or_default();
        // SAFETY: plain calls on our own window
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, flags);
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// The control scrolled (or might have): drawn again if it did.
    pub fn update(&self) {
        let now = self.read();
        if self.range.replace(now) != now {
            // SAFETY: plain call on our own window
            unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); }
        }
    }

    fn read(&self) -> Range {
        let mut info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_ALL, ..Default::default() };
        // SAFETY: a plain query into our struct
        unsafe { let _ = GetScrollInfo(self.control, if self.vertical { SB_VERT } else { SB_HORZ }, &mut info); }
        Range { min: info.nMin, max: info.nMax, page: info.nPage, pos: info.nPos }
    }

    /// Its track along it: start and length (its pixels).
    pub(super) fn track(&self) -> (i32, i32) {
        let mut r = RECT::default();
        // SAFETY: plain query
        unsafe { let _ = GetClientRect(self.hwnd, &mut r); }
        let length = if self.vertical { r.bottom } else { r.right };
        let start = self.start.get() + px(GAP);
        (start, length - px(GAP) - start)
    }

    /// The thumb: start and length along the bar (its pixels).
    pub(super) fn thumb(&self) -> Option<(i32, i32)> {
        let (start, length) = self.track();
        self.range.get().thumb(length, px(SHORTEST)).map(|(at, size)| (start + at, size))
    }

    pub(super) fn paint(&self, p: &Painter) {
        p.fill(p.rect, theme::VIEW);
        let Some((at, size)) = self.thumb() else { return };
        let busy = self.hover.get() || self.drag.get().is_some();
        let width = px(if busy { WIDE } else { SLIM });
        let (across, edge) = (if self.vertical { p.rect.right } else { p.rect.bottom }, px(GAP));
        let rect = |from: i32, to: i32| if self.vertical {
            RECT { left: across - edge - width, top: from, right: across - edge, bottom: to }
        } else {
            RECT { left: from, top: across - edge - width, right: to, bottom: across - edge }
        };
        if busy {
            let (start, length) = self.track();
            p.round(rect(start, start + length), width / 2, Some(theme::mix(theme::VIEW, theme::TRACK, 0.6)), None);
        }
        let color = match (self.drag.get(), self.hover.get()) {
            (Some(_), _) => theme::ACCENT,
            (None, true) => theme::mix(theme::VIEW, theme::DIM, 0.8),
            _ => theme::mix(theme::VIEW, theme::DIM, 0.5),
        };
        p.round(rect(at, at + size), width / 2, Some(color), None);
    }

    /// Scroll the control so its top (or left) is at `pos`.
    pub(super) fn scroll_to(&self, pos: i32) {
        let by = pos - self.read().pos;
        if by == 0 {
            return;
        }
        if self.list {
            // (list views: their rows in pixels, as LVM_SCROLL wants them)
            let mut row = RECT { left: LVIR_BOUNDS as i32, ..Default::default() };
            let top = send(self.control, LVM_GETTOPINDEX, 0, 0) as usize;
            send(self.control, LVM_GETITEMRECT, top, &mut row as *mut RECT as isize);
            let (dx, dy) = if self.vertical { (0, by * (row.bottom - row.top).max(1)) } else { (by, 0) };
            send(self.control, LVM_SCROLL, dx as usize, dy as isize);
        } else {
            self.scroll(SB_THUMBPOSITION.0 as usize | ((pos as usize & 0xffff) << 16));
        }
        self.update();
    }

    /// Windows' scroll message (`what`: SB_…) to the control, then the end of it.
    fn scroll(&self, what: usize) {
        let msg = if self.vertical { WM_VSCROLL } else { WM_HSCROLL };
        send(self.control, msg, what, 0);
        send(self.control, msg, SB_ENDSCROLL.0 as usize, 0);
        self.update();
    }

    /// The mouse at `lparam` (a mouse message's): where it is along the bar.
    pub(super) fn mouse_at(&self, lparam: LPARAM) -> i32 {
        let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
        self.mouse.set(if self.vertical { y } else { x });
        self.mouse.get()
    }

    /// The track held: a page toward the mouse, if the thumb isn't there yet.
    pub(super) fn page(&self) {
        let (along, way) = (self.mouse.get(), self.paging.get());
        let Some((at, size)) = self.thumb() else { return };
        if (way < 0 && along < at) || (way > 0 && along >= at + size) {
            self.scroll(if way < 0 { SB_PAGEUP.0 } else { SB_PAGEDOWN.0 } as usize);
        }
    }

    pub(super) fn redraw(&self) {
        // SAFETY: plain call on our own window
        unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); }
    }
}
