//! Windows' buttons and check boxes drawn the way GTK draws them on Linux Mint (Mint-Y): flat,
//! a 1-pixel border, small corners; the main button, ticked boxes and switches in Mint's green.

use super::look::{marked, DROPDOWN, FLAT, ICON, PRIMARY, SWITCH};
use super::{px, send, text};
use crate::win::theme::{self, Font, Painter, WHITE};
use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub enum Kind {
    Push,
    Check,
}

/// A push button or a check box (the others are drawn by Windows).
pub fn kind(hwnd: HWND) -> Option<Kind> {
    // SAFETY: plain query
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as i32 & BS_TYPEMASK;
    match style {
        BS_PUSHBUTTON | BS_DEFPUSHBUTTON => Some(Kind::Push),
        BS_CHECKBOX | BS_AUTOCHECKBOX => Some(Kind::Check),
        _ => None,
    }
}

/// Draw it (NM_CUSTOMDRAW's first stage), on `behind`, the colour of the area it's on.
pub fn draw(kind: Kind, d: &NMCUSTOMDRAW, behind: COLORREF) {
    let r = d.rc;
    Painter::over(d.hdc, r, |p| {
        p.fill(r, behind);
        match kind {
            Kind::Push => push(p, d, behind),
            Kind::Check if marked(d.hdr.hwndFrom, SWITCH) => switch(p, d),
            Kind::Check => check(p, d),
        }
    });
}

fn push(p: &Painter, d: &NMCUSTOMDRAW, behind: COLORREF) {
    let (hwnd, state, r) = (d.hdr.hwndFrom, d.uItemState, d.rc);
    let (disabled, down, hot) = (state.contains(CDIS_DISABLED), state.contains(CDIS_SELECTED), state.contains(CDIS_HOT));
    let darker = |c: COLORREF, by: f64| theme::mix(c, COLORREF(0), by);
    let (fill, border, color) = if marked(hwnd, FLAT) {
        let fill = if down { Some(theme::pressed(behind)) } else if hot { Some(theme::hover(behind)) } else { None };
        (fill, None, if disabled { theme::disabled() } else { theme::text() })
    } else if marked(hwnd, PRIMARY) && !disabled {
        let fill = if down { darker(theme::accent(), 0.12) } else if hot { theme::mix(theme::accent(), WHITE, 0.12) } else { theme::accent() };
        (Some(fill), Some(darker(theme::accent(), 0.12)), theme::on_accent())
    } else if disabled {
        (Some(theme::mix(theme::button(), behind, 0.5)), Some(theme::mix(theme::button_border(), behind, 0.5)), theme::disabled())
    } else {
        let fill = if down { theme::button_pressed() } else if hot { theme::button_hover() } else { theme::button() };
        (Some(fill), Some(if hot || down { darker(theme::button_border(), 0.1) } else { theme::button_border() }), theme::text())
    };
    p.round(r, px(theme::radius()), fill, border);
    let cues = state.contains(CDIS_SHOWKEYBOARDCUES);
    if state.contains(CDIS_FOCUS) && cues {
        let ring = if marked(hwnd, PRIMARY) { theme::mix(WHITE, theme::accent(), 0.3) } else { theme::accent() };
        p.round(RECT { left: r.left + px(2), top: r.top + px(2), right: r.right - px(2), bottom: r.bottom - px(2) },
                px(theme::radius() - 1), None, Some(ring));
    }
    let label = text(hwnd);
    if marked(hwnd, ICON) {
        p.icon(r, label.chars().next().unwrap_or(' '), 12, color); // the whole button: no room to spare
        return;
    }
    let prefix = if cues { DRAW_TEXT_FORMAT(0) } else { DT_HIDEPREFIX };
    if marked(hwnd, DROPDOWN) {
        let arrow = RECT { left: r.right - px(28), right: r.right - px(6), ..r };
        p.icon(arrow, theme::icon::CHEVRON_DOWN, 10, color);
        let words = RECT { left: r.left + px(10), right: arrow.left, ..r };
        p.draw_text(words, &label, Font::Body, color, prefix | DT_END_ELLIPSIS | DT_SINGLELINE | DT_VCENTER | DT_LEFT);
        return;
    }
    let cut = if label.contains('\\') { DT_PATH_ELLIPSIS } else { DT_END_ELLIPSIS };
    let inner = RECT { left: r.left + px(8), right: r.right - px(8), ..r };
    p.draw_text(inner, &label, Font::Body, color, prefix | cut | DT_SINGLELINE | DT_VCENTER | DT_CENTER);
}

/// A box with Mint's green and a white tick when ticked, its words after it.
fn check(p: &Painter, d: &NMCUSTOMDRAW) {
    let (hwnd, state, r) = (d.hdr.hwndFrom, d.uItemState, d.rc);
    let (disabled, hot) = (state.contains(CDIS_DISABLED), state.contains(CDIS_HOT));
    let ticked = send(hwnd, BM_GETCHECK, 0, 0) == 1;
    let middle = (r.top + r.bottom) / 2;
    let size = px(16);
    let check = RECT { left: r.left + 1, top: middle - size / 2, right: r.left + 1 + size, bottom: middle - size / 2 + size };
    if ticked {
        let fill = if disabled { theme::disabled() } else if hot { theme::mix(theme::accent(), WHITE, 0.12) } else { theme::accent() };
        p.round(check, px(theme::radius()), Some(fill), None);
        p.icon(check, theme::icon::CHECK, 12, theme::on_accent());
    } else {
        let line = if disabled { theme::border() } else if hot { theme::accent() } else { theme::mix(theme::button_border(), COLORREF(0), 0.12) };
        p.round(check, px(theme::radius()), Some(theme::view()), Some(line));
    }
    let cues = state.contains(CDIS_SHOWKEYBOARDCUES);
    let label = RECT { left: check.right + px(8), ..r };
    let prefix = if cues { DRAW_TEXT_FORMAT(0) } else { DT_HIDEPREFIX };
    let color = if disabled { theme::disabled() } else { theme::text() };
    p.draw_text(label, &text(hwnd), Font::Body, color, prefix | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    if state.contains(CDIS_FOCUS) && cues {
        p.round(RECT { left: check.left - 1, top: check.top - 1, right: check.right + 1, bottom: check.bottom + 1 },
                px(theme::radius()), None, Some(theme::accent()));
    }
}

/// A switch, as GTK's on Mint: a pill, Mint's green when on, with a white knob.
fn switch(p: &Painter, d: &NMCUSTOMDRAW) {
    let (hwnd, state, r) = (d.hdr.hwndFrom, d.uItemState, d.rc);
    let (disabled, hot) = (state.contains(CDIS_DISABLED), state.contains(CDIS_HOT));
    let on = send(hwnd, BM_GETCHECK, 0, 0) == 1;
    let (width, height) = (px(40), px(22));
    let top = (r.top + r.bottom - height) / 2;
    let track = RECT { left: r.left + 1, top, right: r.left + 1 + width, bottom: top + height };
    let fill = match (on, disabled, hot) {
        (true, true, _) => theme::disabled(),
        (true, _, true) => theme::mix(theme::accent(), WHITE, 0.12),
        (true, ..) => theme::accent(),
        (false, _, true) => theme::mix(theme::track(), COLORREF(0), 0.06),
        (false, ..) => theme::track(),
    };
    p.round(track, height / 2, Some(fill), None);
    let knob = height - px(4);
    let left = if on { track.right - px(2) - knob } else { track.left + px(2) };
    p.round(RECT { left, top: top + px(2), right: left + knob, bottom: top + px(2) + knob }, knob / 2, Some(WHITE), None);
    if state.contains(CDIS_FOCUS) && state.contains(CDIS_SHOWKEYBOARDCUES) {
        let ring = RECT { left: track.left - 1, top: track.top - 1, right: track.right + 1, bottom: track.bottom + 1 };
        p.round(ring, height / 2 + 1, None, Some(theme::accent()));
    }
}
