//! "Show details": the map of every connection's part and the list of connections, as on Linux
//! and like IDM's. Both count what arrived, not only what's written, so they move live.

use super::super::theme::{self, Align, Font, Painter};
use super::super::wnd::{canvas, place, px, send, set_visible, Window};
use std::cell::RefCell;
use std::rc::Rc;
use turbodm::engine::segments::Segment;
use turbodm::engine::Snapshot;
use turbodm::text::{connection_info, part_info};
use turbodm::util::human_size;
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Connection rows shown without scrolling; the map's height and the list's (96-DPI pixels).
const VISIBLE_ROWS: i32 = 8;
pub const MAP: i32 = 38;
pub const LIST: i32 = 28 + VISIBLE_ROWS * 22;

#[derive(Default)]
struct Data {
    size: Option<u64>,
    segments: Vec<Segment>,
    video: bool, // a video: its parts (video, sound) instead of connections
}

pub struct Details {
    map: HWND,
    pub list: HWND,
    data: Rc<RefCell<Data>>,
    rows: Rc<RefCell<Vec<[String; 4]>>>,
}

impl Details {
    pub fn new(window: &Window) -> Self {
        let data: Rc<RefCell<Data>> = Rc::default();
        let d = data.clone();
        let map = window.canvas(move |p| {
            let data = d.borrow();
            p.fill(p.rect, theme::VIEW);
            let r = p.rect;
            let caption = if data.video { "The video and its sound, downloaded one after the other" }
                          else { "Start positions and progress of each connection" };
            p.text(RECT { bottom: px(18), ..r }, caption, Font::Body, theme::DIM, Align::Left);
            let bar = RECT { top: r.bottom - px(14), ..r };
            p.fill(bar, theme::TRACK);
            let Some(size) = data.size.filter(|&s| s > 0) else { return };
            let x = |bytes: u64| bar.left + (bytes as f64 / size as f64 * (bar.right - bar.left) as f64) as i32;
            for seg in &data.segments {
                let (start, end) = (x(seg.start), x(seg.start + seg.got()).max(x(seg.start) + (seg.got() > 0) as i32));
                p.fill(RECT { left: start, right: end, ..bar }, theme::ACCENT); // what arrived
                if seg.active && !seg.finished() {
                    p.fill(RECT { left: end - px(2), right: end + 1, ..bar }, theme::GREEN); // a connection at work
                }
                p.fill(RECT { left: start, right: start + 1, ..bar }, theme::VIEW); // where each part starts
            }
        });
        let style = WINDOW_STYLE(LVS_REPORT | LVS_NOSORTHEADER | LVS_SINGLESEL) | WS_TABSTOP;
        let (list, id) = window.control(WC_LISTVIEWW, "", style, WINDOW_EX_STYLE(0));
        send(list, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize);
        // SAFETY: Explorer's look for the list
        unsafe { let _ = SetWindowTheme(list, w!("Explorer"), PCWSTR::null()); }
        send(list, LVM_SETBKCOLOR, 0, theme::VIEW.0 as isize);
        send(list, LVM_SETTEXTBKCOLOR, 0, CLR_NONE as isize);
        for (i, (title, width, right)) in [("N.", 44, true), ("Starts at", 110, true), ("Downloaded", 110, true),
                                           ("Info", 300, false)].iter().enumerate() {
            let text = HSTRING::from(*title);
            let column = LVCOLUMNW { mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT, cx: px(*width),
                                     fmt: if *right { LVCFMT_RIGHT } else { LVCFMT_LEFT },
                                     pszText: PWSTR(text.as_ptr() as *mut _), ..Default::default() };
            send(list, LVM_INSERTCOLUMNW, i, &column as *const _ as isize);
        }
        let rows: Rc<RefCell<Vec<[String; 4]>>> = Rc::default();
        let r = rows.clone();
        window.on_notify(id, move |header, lparam| (header.code == NM_CUSTOMDRAW).then(|| draw(list, &r.borrow(), lparam)));
        Details { map, list, data, rows }
    }

    /// Lay out from `y` (96-DPI pixels), `width` wide; returns the y below.
    pub fn place(&self, x: i32, y: i32, width: i32) -> i32 {
        place(self.map, x, y, width, MAP);
        place(self.list, x, y + MAP + 10, width, LIST);
        // Info takes the rest, leaving room for the scroll bar many connections bring
        let others: i32 = (0..3).map(|i| send(self.list, LVM_GETCOLUMNWIDTH, i, 0) as i32).sum();
        // SAFETY: plain query
        let scroll_bar = unsafe { GetSystemMetrics(SM_CXVSCROLL) };
        send(self.list, LVM_SETCOLUMNWIDTH, 3, (px(width) - others - scroll_bar).max(px(80)) as isize);
        y + MAP + 10 + LIST
    }

    pub fn show(&self, on: bool) {
        set_visible(self.map, on);
        set_visible(self.list, on);
    }

    pub fn update(&self, s: &Snapshot) {
        {
            let mut data = self.data.borrow_mut();
            if data.size != s.size || data.segments != s.segments || data.video != s.video.is_some() {
                *data = Data { size: s.size, segments: s.segments.clone(), video: s.video.is_some() };
                canvas::redraw(self.map);
            }
        }
        let wanted: Vec<[String; 4]> = s.segments.iter().enumerate().map(|(i, seg)| {
            [(i + 1).to_string(), human_size(Some(seg.start)), human_size(Some(seg.got())),
             if s.video.is_some() { part_info(i, s) } else { connection_info(seg, s.status).to_string() }]
        }).collect();
        let mut rows = self.rows.borrow_mut();
        while rows.len() < wanted.len() {
            let item = LVITEMW { mask: LVIF_TEXT, iItem: rows.len() as i32, pszText: PWSTR(w!("").as_ptr() as *mut _),
                                 ..Default::default() };
            send(self.list, LVM_INSERTITEMW, 0, &item as *const _ as isize);
            rows.push(Default::default());
        }
        for (i, (row, values)) in rows.iter_mut().zip(wanted).enumerate() {
            for (column, (shown, value)) in row.iter_mut().zip(values).enumerate() {
                if *shown != value {
                    let text = HSTRING::from(value.as_str());
                    let item = LVITEMW { iSubItem: column as i32, pszText: PWSTR(text.as_ptr() as *mut _), ..Default::default() };
                    send(self.list, LVM_SETITEMTEXTW, i, &item as *const _ as isize);
                    *shown = value;
                }
            }
        }
    }
}

/// The connections' cells, as GTK's plain grid on Linux: their text, nothing else.
fn draw(list: HWND, rows: &[[String; 4]], lparam: LPARAM) -> LRESULT {
    // SAFETY: NM_CUSTOMDRAW from a list view comes with an NMLVCUSTOMDRAW
    let draw = unsafe { &*(lparam.0 as *const NMLVCUSTOMDRAW) };
    let stage = draw.nmcd.dwDrawStage.0;
    if stage == CDDS_PREPAINT.0 {
        return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
    }
    if stage == CDDS_ITEMPREPAINT.0 {
        return LRESULT(CDRF_NOTIFYSUBITEMDRAW as isize);
    }
    let (row, column) = (draw.nmcd.dwItemSpec, draw.iSubItem);
    let Some(text) = rows.get(row).and_then(|r| r.get(column as usize)) else { return LRESULT(CDRF_DODEFAULT as isize) };
    let mut r = RECT { top: column, left: LVIR_BOUNDS as i32, ..Default::default() };
    send(list, LVM_GETSUBITEMRECT, row, &mut r as *mut _ as isize);
    if column == 0 {
        r.right = r.left + send(list, LVM_GETCOLUMNWIDTH, 0, 0) as i32;
    }
    Painter::over(draw.nmcd.hdc, r, |p| {
        p.fill(r, theme::VIEW);
        let inner = RECT { left: r.left + px(6), right: r.right - px(6), ..r };
        p.text(inner, text, Font::Body, theme::TEXT, if column == 3 { Align::Left } else { Align::Right });
    });
    LRESULT(CDRF_SKIPDEFAULT as isize)
}
