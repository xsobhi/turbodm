//! Text fields as Mint-Y draws them: Windows' edit control without its border, inside a frame
//! its window draws (a thin border, Mint's green while focused), with any buttons of the field
//! (a number's − and +) inside the frame too.

use super::px;
use crate::win::theme::{self, Font, Painter};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// A text field: its frame (in its window's pixels) and the buttons inside it (a number's − +).
struct Field {
    parent: isize,
    frame: RECT,
    buttons: Vec<HWND>,
}

thread_local! {
    static FIELDS: RefCell<HashMap<isize, Field>> = RefCell::default();
}

/// Draw `edit` as a Fluent text field, with `buttons` inside on the right.
pub fn make_field(edit: HWND, buttons: Vec<HWND>) {
    // SAFETY: plain query
    let parent = unsafe { GetParent(edit) }.unwrap_or_default();
    FIELDS.with(|f| f.borrow_mut().insert(edit.0 as isize, Field { parent: parent.0 as isize, frame: RECT::default(), buttons }));
}

/// The buttons inside a field (they show and hide with it).
pub fn buttons_of(edit: HWND) -> Vec<HWND> {
    FIELDS.with(|f| f.borrow().get(&(edit.0 as isize)).map(|f| f.buttons.clone()).unwrap_or_default())
}

/// Put a field in `frame` (window pixels): its text centred inside, its buttons on the right.
/// False when `edit` isn't a field.
pub fn place_field(edit: HWND, frame: RECT) -> bool {
    let buttons = FIELDS.with(|f| {
        let mut fields = f.borrow_mut();
        let field = fields.get_mut(&(edit.0 as isize))?;
        field.frame = frame;
        Some(field.buttons.clone())
    });
    let Some(buttons) = buttons else { return false };
    let side = frame.bottom - frame.top - px(8);
    let mut right = frame.right - px(4);
    for button in buttons.iter().rev() {
        right -= side;
        move_to(*button, RECT { left: right, top: frame.top + px(4), right: right + side, bottom: frame.top + px(4) + side });
        right -= px(2);
    }
    let right = if buttons.is_empty() { frame.right - px(10) } else { right - px(4) };
    let height = text_height() + px(2);
    let top = frame.top + (frame.bottom - frame.top - height) / 2;
    move_to(edit, RECT { left: frame.left + px(10), top, right, bottom: top + height });
    redraw_frame(edit);
    true
}

fn move_to(hwnd: HWND, r: RECT) {
    // SAFETY: plain move of our control
    unsafe { let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE); }
}

fn text_height() -> i32 {
    static HEIGHT: OnceLock<i32> = OnceLock::new();
    *HEIGHT.get_or_init(|| unsafe {
        // SAFETY: a measurement on the screen's DC, released after
        let dc = GetDC(None);
        let old = SelectObject(dc, theme::font(Font::Body).into());
        let mut metrics = TEXTMETRICW::default();
        let _ = GetTextMetricsW(dc, &mut metrics);
        SelectObject(dc, old);
        ReleaseDC(None, dc);
        metrics.tmHeight
    })
}

/// The field's frame needs drawing again (focus moved, it was shown or hidden).
pub fn redraw_frame(edit: HWND) {
    let field = FIELDS.with(|f| f.borrow().get(&(edit.0 as isize)).map(|f| (f.parent, f.frame)));
    if let Some((parent, frame)) = field {
        // SAFETY: plain call on our window
        unsafe { let _ = InvalidateRect(Some(HWND(parent as *mut _)), Some(&frame), false); }
    }
}

/// (x, y) in `parent` is inside one of its fields' frames.
pub fn inside_field(parent: HWND, x: i32, y: i32) -> bool {
    FIELDS.with(|f| f.borrow().values().any(|field| field.parent == parent.0 as isize
        && x >= field.frame.left && x < field.frame.right && y >= field.frame.top && y < field.frame.bottom))
}

/// The window is gone: forget its fields.
pub fn forget(parent: HWND) {
    FIELDS.with(|f| f.borrow_mut().retain(|_, field| field.parent != parent.0 as isize));
}

/// The frames of `parent`'s visible fields; `focus` has the keyboard.
pub fn paint_frames(p: &Painter, parent: HWND, focus: HWND) {
    FIELDS.with(|fields| {
        for (edit, field) in fields.borrow().iter() {
            let edit = HWND(*edit as *mut _);
            // SAFETY: plain query
            if field.parent != parent.0 as isize || !unsafe { IsWindowVisible(edit) }.as_bool() {
                continue;
            }
            frame(p, field.frame, focus == edit || field.buttons.contains(&focus));
        }
    });
}

/// A field's frame, as Mint-Y's: white, a thin border, Mint's green around it when focused.
fn frame(p: &Painter, r: RECT, focused: bool) {
    p.round(r, px(theme::RADIUS), Some(theme::VIEW), Some(if focused { theme::ACCENT } else { theme::ENTRY_BORDER }));
    if focused {
        let inner = RECT { left: r.left + 1, top: r.top + 1, right: r.right - 1, bottom: r.bottom - 1 };
        p.round(inner, px(theme::RADIUS) - 1, None, Some(theme::mix(theme::ACCENT, theme::VIEW, 0.6)));
    }
}
