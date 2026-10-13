//! Windows' own controls (buttons, text fields, labels, check boxes) in the look of the rest,
//! as on Linux Mint: every control on the colour of the area it sits on, buttons and check
//! boxes drawn as Mint-Y draws them (buttons.rs), fields in its frames (field.rs).

use super::{buttons, Window};
use crate::win::theme::{self, Painter};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Marks on buttons (window properties) for how they're drawn.
pub const PRIMARY: PCWSTR = w!("TurboDM.Primary"); // the main button: in the accent colour
pub const FLAT: PCWSTR = w!("TurboDM.Flat"); // no fill until the mouse is on it
pub const ICON: PCWSTR = w!("TurboDM.Icon"); // its text is an icon (Windows' icon font)
pub const SWITCH: PCWSTR = w!("TurboDM.Switch"); // a check box drawn as a switch
pub const DROPDOWN: PCWSTR = w!("TurboDM.Dropdown"); // a button that opens a list: its words, an arrow

pub fn mark(hwnd: HWND, what: PCWSTR) {
    // SAFETY: a property on our own control
    unsafe { let _ = SetPropW(hwnd, what, Some(HANDLE(1 as *mut _))); }
}

pub(super) fn marked(hwnd: HWND, what: PCWSTR) -> bool {
    // SAFETY: plain query
    unsafe { !GetPropW(hwnd, what).is_invalid() }
}

impl Window {
    /// Areas (window pixels) painted in other colours than the window's, later ones on top,
    /// such as the page of tabs; the controls on them take their colour.
    pub fn set_surfaces(&self, surfaces: Vec<(RECT, COLORREF)>) {
        if *self.surfaces.borrow() != surfaces {
            *self.surfaces.borrow_mut() = surfaces;
            // SAFETY: plain call on our window
            unsafe { let _ = RedrawWindow(Some(self.hwnd()), None, None, RDW_INVALIDATE | RDW_ALLCHILDREN); }
        }
    }

    /// A label's own text colour.
    pub fn set_text_color(&self, control: HWND, color: COLORREF) {
        if self.colors.borrow_mut().insert(control.0 as isize, color) != Some(color) {
            // SAFETY: plain call on our control
            unsafe { let _ = InvalidateRect(Some(control), None, true); }
        }
    }
}

/// The colour of the window's area under `r` (window pixels).
fn surface_at(win: &Window, r: RECT) -> COLORREF {
    let (x, y) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    if super::field::inside_field(win.hwnd(), x, y) {
        return theme::VIEW; // a field's buttons are on its white
    }
    win.surfaces.borrow().iter().rev().find(|(s, _)| x >= s.left && x < s.right && y >= s.top && y < s.bottom)
        .map_or(theme::CHROME, |(_, color)| *color)
}

fn rect_in(control: HWND, parent: HWND) -> RECT {
    let mut r = RECT::default();
    // SAFETY: plain queries
    unsafe {
        let _ = GetWindowRect(control, &mut r);
        let mut points = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
        MapWindowPoints(None, Some(parent), &mut points);
        RECT { left: points[0].x, top: points[0].y, right: points[1].x, bottom: points[1].y }
    }
}

fn class_is(hwnd: HWND, name: &str) -> bool {
    let mut class = [0u16; 32];
    // SAFETY: plain query into our buffer
    let n = unsafe { GetClassNameW(hwnd, &mut class) }.max(0) as usize;
    String::from_utf16_lossy(&class[..n]).eq_ignore_ascii_case(name)
}

/// The window's background, colours of its controls, its buttons: what Windows would draw
/// otherwise. `None`: not handled here.
pub(super) fn handle(win: &Window, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_ERASEBKGND => Some(LRESULT(1)), // all painted in WM_PAINT
        WM_PAINT => {
            paint(win);
            Some(LRESULT(0))
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let (dc, control) = (HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _));
            let (bg, fg) = if class_is(control, "Edit") {
                (theme::VIEW, theme::TEXT)
            } else {
                let fg = win.colors.borrow().get(&(control.0 as isize)).copied().unwrap_or(theme::TEXT);
                (surface_at(win, rect_in(control, win.hwnd())), fg)
            };
            // SAFETY: the DC Windows passes for drawing the control
            unsafe {
                SetBkColor(dc, bg);
                SetTextColor(dc, fg);
            }
            Some(LRESULT(theme::brush(bg).0 as isize))
        }
        WM_DRAWITEM => {
            // SAFETY: WM_DRAWITEM comes with a DRAWITEMSTRUCT for the control to draw
            let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            let bg = surface_at(win, rect_in(item.hwndItem, win.hwnd()));
            let color = if item.itemState.0 & ODS_DISABLED.0 != 0 { theme::DISABLED }
                        else { win.colors.borrow().get(&(item.hwndItem.0 as isize)).copied().unwrap_or(theme::TEXT) };
            super::labels::draw(item, bg, color).then_some(LRESULT(1))
        }
        WM_NOTIFY => {
            // SAFETY: WM_NOTIFY's lparam points at an NMHDR; for NM_CUSTOMDRAW, an NMCUSTOMDRAW
            let header = unsafe { &*(lparam.0 as *const NMHDR) };
            if header.code != NM_CUSTOMDRAW || !class_is(header.hwndFrom, "Button") {
                return None;
            }
            let kind = buttons::kind(header.hwndFrom)?;
            let draw = unsafe { &*(lparam.0 as *const NMCUSTOMDRAW) };
            (draw.dwDrawStage == CDDS_PREPAINT).then(|| {
                buttons::draw(kind, draw, surface_at(win, rect_in(header.hwndFrom, win.hwnd())));
                LRESULT(CDRF_SKIPDEFAULT as isize)
            })
        }
        _ => None,
    }
}

fn paint(win: &Window) {
    let hwnd = win.hwnd();
    let surfaces = win.surfaces.borrow().clone();
    // SAFETY: plain query
    let focus = unsafe { GetFocus() };
    Painter::paint(hwnd, |p| {
        p.fill(p.rect, theme::CHROME);
        for (r, color) in &surfaces {
            p.fill(*r, *color);
        }
        super::field::paint_frames(p, hwnd, focus);
    });
}
