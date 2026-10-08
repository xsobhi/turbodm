//! "Show details": the bar of every connection's part and the list of connections, like IDM's.

use super::super::wnd::{place, px, send, Window};
use std::cell::RefCell;
use std::rc::Rc;
use turbodm::engine::segments::Segment;
use turbodm::engine::Snapshot;
use turbodm::text::connection_info;
use turbodm::util::human_size;
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LRESULT, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::SystemServices::SS_OWNERDRAW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Connection rows shown without scrolling.
pub const VISIBLE_ROWS: i32 = 8;
pub const ROW: i32 = 19;

#[derive(Default)]
struct Data {
    size: Option<u64>,
    segments: Vec<Segment>,
}

pub struct Details {
    pub caption: HWND,
    pub bar: HWND,
    pub list: HWND,
    data: Rc<RefCell<Data>>,
    rows: RefCell<Vec<[String; 4]>>,
}

impl Details {
    pub fn new(window: &Window) -> Self {
        let caption = window.label("Start positions and progress of each connection");
        let (bar, bar_id) = window.control(WC_STATICW, "", WINDOW_STYLE(SS_OWNERDRAW.0), WINDOW_EX_STYLE(0));
        let style = WINDOW_STYLE(LVS_REPORT | LVS_NOSORTHEADER | LVS_SINGLESEL) | WS_TABSTOP;
        let (list, _) = window.control(WC_LISTVIEWW, "", style, WS_EX_CLIENTEDGE);
        send(list, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize);
        // SAFETY: Explorer's look for the list
        unsafe { let _ = SetWindowTheme(list, w!("Explorer"), PCWSTR::null()); }
        for (i, (title, width, right)) in [("N.", 40, true), ("Starts at", 100, true), ("Downloaded", 100, true),
                                           ("Info", 210, false)].iter().enumerate() {
            let text = HSTRING::from(*title);
            let column = LVCOLUMNW { mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT, cx: px(*width),
                                     fmt: if *right { LVCFMT_RIGHT } else { LVCFMT_LEFT },
                                     pszText: PWSTR(text.as_ptr() as *mut _), ..Default::default() };
            send(list, LVM_INSERTCOLUMNW, i, &column as *const _ as isize);
        }
        let data: Rc<RefCell<Data>> = Rc::default();
        let d = data.clone();
        window.hook(move |msg, _, lparam| {
            if msg != WM_DRAWITEM {
                return None;
            }
            // SAFETY: WM_DRAWITEM comes with a DRAWITEMSTRUCT for the control to draw
            let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            (item.CtlID == bar_id as u32).then(|| {
                draw(item.hDC, item.rcItem, &d.borrow());
                LRESULT(1)
            })
        });
        Details { caption, bar, list, data, rows: RefCell::default() }
    }

    /// Lay out from `y` (96-DPI pixels), `width` wide; returns the y below.
    pub fn place(&self, x: i32, y: i32, width: i32) -> i32 {
        place(self.caption, x, y, width, 16);
        place(self.bar, x, y + 20, width, 10);
        let height = (VISIBLE_ROWS + 1) * ROW + 8;
        place(self.list, x, y + 38, width, height);
        y + 38 + height
    }

    pub fn show(&self, on: bool) {
        for hwnd in [self.caption, self.bar, self.list] {
            super::super::wnd::set_visible(hwnd, on);
        }
    }

    pub fn update(&self, s: &Snapshot) {
        {
            let mut data = self.data.borrow_mut();
            if data.size != s.size || data.segments != s.segments {
                *data = Data { size: s.size, segments: s.segments.clone() };
                // SAFETY: redraw our bar
                unsafe { let _ = InvalidateRect(Some(self.bar), None, false); }
            }
        }
        let wanted: Vec<[String; 4]> = s.segments.iter().enumerate().map(|(i, seg)| {
            [(i + 1).to_string(), human_size(Some(seg.start)), human_size(Some(seg.done)),
             connection_info(seg, s.status).to_string()]
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

/// The bar: each part's downloaded share in the selection colour, live connections' heads
/// in green, a line where each part starts.
fn draw(dc: HDC, rect: RECT, data: &Data) {
    let fill = |left: i32, right: i32, color: COLORREF| {
        let area = RECT { left, right, ..rect };
        // SAFETY: drawing into the DC Windows gave us for this control
        unsafe {
            let brush = CreateSolidBrush(color);
            FillRect(dc, &area, brush);
            let _ = DeleteObject(brush.into());
        }
    };
    let width = (rect.right - rect.left) as f64;
    fill(rect.left, rect.right, COLORREF(0x00DCDCDC));
    let Some(size) = data.size.filter(|&s| s > 0) else { return };
    // SAFETY: plain query
    let done_color = COLORREF(unsafe { GetSysColor(COLOR_HIGHLIGHT) });
    let x = |bytes: u64| rect.left + (bytes as f64 / size as f64 * width) as i32;
    for seg in &data.segments {
        let (start, end) = (x(seg.start), x(seg.start + seg.done).max(x(seg.start) + (seg.done > 0) as i32));
        fill(start, end, done_color);
        if seg.active && !seg.finished() {
            fill(end - 2, end + 1, COLORREF(0x0048C832)); // green (0x00BBGGRR)
        }
        fill(start, start + 1, COLORREF(0x00FFFFFF));
    }
}
