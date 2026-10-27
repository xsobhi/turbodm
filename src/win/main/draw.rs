//! Drawing the downloads list's rows as GTK does on Linux Mint: the file's icon and name, the
//! status as a progress bar with its words over it, every cell on one line, lines between rows
//! and the selected row in Mint's green.

use super::super::theme::{self, Align, Font, Painter};
use super::super::wnd::{px, send};
use super::cells::{system_icons, Row, COLUMNS, STATUS};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT};
use windows::Win32::UI::Controls::*;

/// Rows this tall (96-DPI pixels), as on Linux.
pub const ROW: i32 = 38;

/// NM_CUSTOMDRAW from the list: the cells are ours to draw.
pub fn custom_draw(list: HWND, rows: &[Row], lparam: LPARAM) -> LRESULT {
    // SAFETY: NM_CUSTOMDRAW from a list view comes with an NMLVCUSTOMDRAW
    let draw = unsafe { &mut *(lparam.0 as *mut NMLVCUSTOMDRAW) };
    let stage = draw.nmcd.dwDrawStage.0;
    if stage == CDDS_PREPAINT.0 {
        // (the look may have changed: Options → Look)
        if send(list, LVM_GETBKCOLOR, 0, 0) as u32 != theme::view().0 {
            send(list, LVM_SETBKCOLOR, 0, theme::view().0 as isize);
            send(list, LVM_SETTEXTCOLOR, 0, theme::text().0 as isize);
        }
        return LRESULT((CDRF_NOTIFYITEMDRAW | CDRF_NOTIFYPOSTPAINT) as isize);
    }
    if stage == CDDS_POSTPAINT.0 {
        below_rows(list, rows.len(), draw.nmcd.hdc);
        return LRESULT(CDRF_DODEFAULT as isize);
    }
    if stage != CDDS_ITEMPREPAINT.0 {
        return LRESULT(CDRF_DODEFAULT as isize);
    }
    let index = draw.nmcd.dwItemSpec;
    let Some(row) = rows.get(index) else { return LRESULT(CDRF_DODEFAULT as isize) };
    let mut whole = RECT { left: LVIR_BOUNDS as i32, ..Default::default() };
    send(list, LVM_GETITEMRECT, index, &mut whole as *mut _ as isize);
    let selected = send(list, LVM_GETITEMSTATE, index, LVIS_SELECTED.0 as isize) & LVIS_SELECTED.0 as isize != 0;
    // as GTK draws lists on Mint: lines between rows, the selected one in Mint's green (or the
    // look's: tinted, on Windows')
    let look = if selected {
        let (bg, text) = (theme::selected(), theme::on_selected(false));
        let bar = if bg == theme::accent() { text } else { theme::accent() };
        Look { bg, text, dim: theme::on_selected(true), track: theme::mix(bg, text, 0.25), bar }
    } else {
        let hot = draw.nmcd.uItemState.contains(CDIS_HOT);
        let bg = if hot { theme::hover(theme::view()) } else { theme::view() };
        Look { bg, text: theme::text(), dim: theme::dim(), track: theme::track(), bar: theme::accent() }
    };
    // the whole row at once: its background and file icon first, then its words and bar in one
    // smooth pass
    Painter::over(draw.nmcd.hdc, whole, |p| {
        p.fill(whole, look.bg);
        if !selected {
            p.fill(RECT { top: whole.bottom - 1, ..whole }, theme::separator());
        }
        let name = cell(list, index, 0);
        let text_left = icon(p, name, row);
        for column in 0..COLUMNS.len() as i32 {
            let r = cell(list, index, column);
            match column {
                0 => p.text(RECT { left: text_left, right: r.right - px(6), ..r }, &row.cells[0], Font::Body, look.text, Align::Left),
                STATUS => status(p, r, row, &look),
                _ => {
                    let text = RECT { left: r.left + px(10), right: r.right - px(6), ..r };
                    p.text(text, &row.cells[column as usize], Font::Body, look.text, Align::Left); // one line, cut with "…"
                }
            }
        }
    });
    LRESULT(CDRF_SKIPDEFAULT as isize)
}

/// Plain white under the last row (Windows' look draws lines there, between the columns).
fn below_rows(list: HWND, count: usize, dc: windows::Win32::Graphics::Gdi::HDC) {
    if count == 0 {
        return; // "No downloads yet" is there
    }
    let mut last = RECT { left: LVIR_BOUNDS as i32, ..Default::default() };
    send(list, LVM_GETITEMRECT, count - 1, &mut last as *mut _ as isize);
    let mut client = RECT::default();
    // SAFETY: plain query
    unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(list, &mut client); }
    if last.bottom < client.bottom {
        Painter::over(dc, client, |p| p.fill(RECT { top: last.bottom, ..client }, theme::view()));
    }
}

/// A row's colours.
struct Look {
    bg: COLORREF,
    text: COLORREF,
    dim: COLORREF,
    track: COLORREF,
    bar: COLORREF,
}

/// A cell's area in the list.
fn cell(list: HWND, item: usize, column: i32) -> RECT {
    let mut r = RECT { top: column, left: LVIR_BOUNDS as i32, ..Default::default() };
    send(list, LVM_GETSUBITEMRECT, item, &mut r as *mut _ as isize);
    if column == 0 {
        r.right = r.left + send(list, LVM_GETCOLUMNWIDTH, 0, 0) as i32; // (its bounds are the whole row)
    }
    r
}

/// The file's icon at the start of the name cell; returns where its name goes.
fn icon(p: &Painter, r: RECT, row: &Row) -> i32 {
    let (mut width, mut height) = (0, 0);
    let x = r.left + px(10);
    // SAFETY: Windows' file-type icons, drawn into the list's DC
    unsafe {
        let _ = ImageList_GetIconSize(system_icons(), Some(&mut width), Some(&mut height));
        if row.icon >= 0 {
            let _ = ImageList_Draw(system_icons(), row.icon, p.gdi(), x, (r.top + r.bottom - height) / 2, ILD_TRANSPARENT);
        }
    }
    x + width + px(8)
}

/// The status: its words, and a thin bar under them.
fn status(p: &Painter, r: RECT, row: &Row, look: &Look) {
    let inner = RECT { left: r.left + px(10), right: r.right - px(10), ..r };
    let top = (r.top + r.bottom - px(23)) / 2;
    p.text(RECT { top, bottom: top + px(16), ..inner }, &row.cells[STATUS as usize], Font::Caption, look.dim, Align::Center);
    let bar = RECT { top: top + px(19), bottom: top + px(23), ..inner };
    p.round(bar, px(2), Some(look.track), None);
    let done = ((bar.right - bar.left) as f64 * row.progress.clamp(0.0, 1.0)).round() as i32;
    if done > 0 {
        // (faint when the file is gone: "File missing")
        let color = if row.missing { theme::mix(look.bar, look.track, 0.7) } else { look.bar };
        p.round(RECT { right: bar.left + done, ..bar }, px(2), Some(color), None);
    }
}
