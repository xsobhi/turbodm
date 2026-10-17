//! The downloads list's column header, drawn by TurboDM in the look's colours (Windows draws it
//! light, whatever the look): each column's name, a thin line between columns and under them,
//! and a small arrow on the sorted one.

use super::super::theme::{self, Align, Font, Painter};
use super::super::wnd::{px, send};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, WM_NOTIFY};

const SUBCLASS: usize = 0x4844; // "HD"

/// The list's header is ours to draw (its drawing is told to the list, its parent).
pub fn own(list: HWND) {
    // SAFETY: a subclass of our own list control, for as long as it lives
    unsafe { let _ = SetWindowSubclass(list, Some(procedure), SUBCLASS, 0); }
}

unsafe extern "system" fn procedure(list: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    // SAFETY: WM_NOTIFY comes with an NMHDR; NM_CUSTOMDRAW with an NMCUSTOMDRAW
    unsafe {
        if msg == WM_NOTIFY {
            let header = &*(lparam.0 as *const windows::Win32::UI::Controls::NMHDR);
            if header.code == NM_CUSTOMDRAW && header.hwndFrom.0 as isize == send(list, LVM_GETHEADER, 0, 0) {
                let draw = &*(lparam.0 as *const NMCUSTOMDRAW);
                match draw.dwDrawStage {
                    CDDS_PREPAINT => {
                        let mut all = RECT::default();
                        let _ = GetClientRect(header.hwndFrom, &mut all);
                        Painter::over(draw.hdc, all, |p| {
                            p.fill(all, theme::view());
                            p.fill(RECT { top: all.bottom - 1, ..all }, theme::border());
                        });
                        return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
                    }
                    CDDS_ITEMPREPAINT => {
                        item(header.hwndFrom, draw.hdc, draw.rc, draw.dwItemSpec, draw.uItemState);
                        return LRESULT(CDRF_SKIPDEFAULT as isize);
                    }
                    _ => {}
                }
            }
        }
        DefSubclassProc(list, msg, wparam, lparam)
    }
}

/// A column's name in `r`, its line on the right, its arrow if the list is sorted by it.
fn item(header: HWND, dc: HDC, r: RECT, index: usize, state: NMCUSTOMDRAW_DRAW_STATE_FLAGS) {
    let mut name = [0u16; 64];
    let mut info = HDITEMW { mask: HDI_TEXT | HDI_FORMAT, pszText: windows::core::PWSTR(name.as_mut_ptr()),
                             cchTextMax: name.len() as i32, ..Default::default() };
    send(header, HDM_GETITEMW, index, &mut info as *mut _ as isize);
    let length = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let name = String::from_utf16_lossy(&name[..length]);
    let (up, down) = (info.fmt.0 & HDF_SORTUP.0 != 0, info.fmt.0 & HDF_SORTDOWN.0 != 0);
    Painter::over(dc, r, |p| {
        let bg = if state.contains(CDIS_HOT) || state.contains(CDIS_SELECTED) { theme::hover(theme::view()) } else { theme::view() };
        p.fill(RECT { bottom: r.bottom - 1, ..r }, bg);
        p.fill(RECT { top: r.bottom - 1, ..r }, theme::border());
        p.fill(RECT { left: r.right - 1, top: r.top + px(6), bottom: r.bottom - px(6), ..r }, theme::border());
        p.text(RECT { left: r.left + px(6), right: r.right - px(6), ..r }, &name, Font::Body, theme::text(), Align::Left);
        if up || down {
            // a small chevron over the middle of the name, as Explorer puts it
            let middle = (r.left + r.right) / 2;
            let arrow = RECT { left: middle - px(6), right: middle + px(6), top: r.top, bottom: r.top + px(9) };
            p.icon(arrow, if up { '\u{E70E}' } else { '\u{E70D}' }, 8, theme::dim());
        }
    });
}
