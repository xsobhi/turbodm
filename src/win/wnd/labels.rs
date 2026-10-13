//! Labels TurboDM draws itself (Windows' static control, owner-drawn), so their text is drawn
//! like the rest: smooth (DirectWrite), in Mint's font, on the colour of the area they're on.

use super::{text, Window};
use crate::win::theme::{Font, Painter};
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::{SS_NOPREFIX, SS_OWNERDRAW};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, WC_STATICW};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy)]
pub enum Kind {
    Line,      // one line, cut with "…" (a path keeps its file name)
    Right,     // the same, against the right edge (a form's names)
    Paragraph, // wrapped onto as many lines as it takes
}

struct Label {
    parent: isize,
    font: Font,
    kind: Kind,
}

thread_local! {
    static LABELS: RefCell<HashMap<isize, Label>> = RefCell::default();
}

impl Window {
    pub fn text_label(&self, text: &str, font: Font, kind: Kind) -> HWND {
        let style = WINDOW_STYLE(SS_OWNERDRAW.0 | SS_NOPREFIX.0);
        let hwnd = self.control(WC_STATICW, text, style, WINDOW_EX_STYLE(0)).0;
        let label = Label { parent: self.hwnd().0 as isize, font, kind };
        LABELS.with(|l| l.borrow_mut().insert(hwnd.0 as isize, label));
        hwnd
    }
}

/// WM_DRAWITEM for one of our labels, on `bg`, in `color`; false when it isn't one.
pub fn draw(item: &DRAWITEMSTRUCT, bg: COLORREF, color: COLORREF) -> bool {
    let Some((font, kind)) = LABELS.with(|l| l.borrow().get(&(item.hwndItem.0 as isize)).map(|l| (l.font, l.kind))) else {
        return false;
    };
    let words = text(item.hwndItem);
    let r = item.rcItem;
    let cut = if words.contains('\\') { DT_PATH_ELLIPSIS } else { DT_END_ELLIPSIS };
    let flags = match kind {
        Kind::Line => DT_SINGLELINE | DT_VCENTER | DT_LEFT | cut,
        Kind::Right => DT_SINGLELINE | DT_VCENTER | DT_RIGHT | cut,
        Kind::Paragraph => DT_WORDBREAK | DT_LEFT,
    };
    Painter::over(item.hDC, r, |p| {
        p.fill(r, bg);
        p.draw_text(r, &words, font, color, flags | DT_NOPREFIX);
    });
    true
}

/// The window is gone: forget its labels.
pub fn forget(parent: HWND) {
    LABELS.with(|l| l.borrow_mut().retain(|_, label| label.parent != parent.0 as isize));
}
