//! The downloads list: Windows' list view in details mode, virtual (rows are drawn on demand,
//! so it stays quick), with Explorer's look, rows drawn like the Linux list's (draw.rs),
//! sortable columns and the selection kept by download, and Mint-Y's slim scroll bars.

use super::super::theme;
use super::super::wnd::{px, scrollbar, send, Window};
use super::cells::{Row, COLUMNS};
use super::draw::{self, ROW};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::engine::Snapshot;
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LRESULT, POINT, RECT};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct List {
    pub hwnd: HWND,
    rows: Rc<RefCell<Vec<Row>>>,
    sort: Rc<Cell<(usize, bool)>>, // column, ascending
    empty: Rc<RefCell<String>>,    // what it says when there's nothing to show
}

impl List {
    /// `on_open`: a double-click on a download; `on_menu`: a right-click, at screen `POINT`;
    /// `on_sort`: a column header was clicked.
    pub fn new(window: &Window, on_open: impl Fn(String) + 'static, on_menu: impl Fn(POINT) + 'static,
               on_sort: impl Fn() + 'static) -> Self {
        let style = WINDOW_STYLE(LVS_REPORT | LVS_OWNERDATA | LVS_SHOWSELALWAYS | LVS_SHAREIMAGELISTS) | WS_TABSTOP;
        let (hwnd, id) = window.control(WC_LISTVIEWW, "", style, WINDOW_EX_STYLE(0));
        let extended = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP;
        send(hwnd, LVM_SETEXTENDEDLISTVIEWSTYLE, extended as usize, extended as isize);
        // SAFETY: Explorer's look for the list, as on this Windows; an image list only as tall as
        // the rows should be (rows are as tall as their icons), with no images: we draw the icons
        unsafe {
            let _ = SetWindowTheme(hwnd, w!("Explorer"), PCWSTR::null());
            let height = ImageList_Create(1, px(ROW), ILC_COLOR32, 0, 1);
            send(hwnd, LVM_SETIMAGELIST, LVSIL_SMALL as usize, height.0);
        }
        send(hwnd, LVM_SETBKCOLOR, 0, theme::view().0 as isize);
        send(hwnd, LVM_SETTEXTBKCOLOR, 0, CLR_NONE as isize);
        send(hwnd, LVM_SETTEXTCOLOR, 0, theme::text().0 as isize);
        scrollbar::slim(hwnd); // Mint-Y's slim scroll bars, not Windows'
        super::header::own(hwnd); // its column header in the look's colours
        for (i, (title, width)) in COLUMNS.iter().enumerate() {
            let text = HSTRING::from(*title);
            let column = LVCOLUMNW { mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT, fmt: LVCFMT_LEFT, cx: px(*width),
                                     pszText: PWSTR(text.as_ptr() as *mut _), ..Default::default() };
            send(hwnd, LVM_INSERTCOLUMNW, i, &column as *const _ as isize);
        }
        let list = List { hwnd, rows: Rc::default(), sort: Rc::new(Cell::new((5, false))), empty: Rc::default() }; // newest first
        list.show_sort_arrow();
        let (rows, sort, empty) = (list.rows.clone(), list.sort.clone(), list.empty.clone());
        window.on_notify(id, move |header, lparam| {
            // SAFETY: each notification comes with the struct named for it
            unsafe {
                match header.code {
                    LVN_GETDISPINFOW => fill(&rows.borrow(), &mut (*(lparam.0 as *mut NMLVDISPINFOW)).item),
                    NM_CUSTOMDRAW => return Some(draw::custom_draw(hwnd, &rows.borrow(), lparam)),
                    LVN_GETEMPTYMARKUP => {
                        let markup = &mut *(lparam.0 as *mut NMLVEMPTYMARKUP);
                        markup.dwFlags = EMF_CENTERED;
                        let text: Vec<u16> = empty.borrow().encode_utf16().take(markup.szMarkup.len() - 1).collect();
                        markup.szMarkup[..text.len()].copy_from_slice(&text);
                        markup.szMarkup[text.len()] = 0;
                        return Some(LRESULT(1));
                    }
                    NM_DBLCLK => {
                        let item = (*(lparam.0 as *const NMITEMACTIVATE)).iItem;
                        let id = rows.borrow().get(item.max(0) as usize).filter(|_| item >= 0).map(|r| r.id.clone());
                        if let Some(id) = id {
                            on_open(id);
                        }
                    }
                    NM_RCLICK => {
                        let mut point = POINT::default();
                        let _ = GetCursorPos(&mut point);
                        on_menu(point);
                    }
                    // finished downloads dragged out, as their files
                    LVN_BEGINDRAG => super::drag_out(&super::super::app()),
                    LVN_COLUMNCLICK => {
                        let column = (*(lparam.0 as *const NMLISTVIEW)).iSubItem as usize;
                        let (current, ascending) = sort.get();
                        sort.set((column, if column == current { !ascending } else { column == 0 }));
                        on_sort();
                    }
                    _ => {}
                }
            }
            None
        });
        list
    }

    /// Show these downloads; the selection stays on the same downloads. `empty` is what it
    /// says when there are none.
    pub fn update(&self, snapshots: &[Snapshot], empty: &str) {
        if *self.empty.borrow() != empty {
            *self.empty.borrow_mut() = empty.to_string();
            self.redraw();
        }
        let (column, ascending) = self.sort.get();
        let mut rows: Vec<Row> = snapshots.iter().map(Row::new).collect();
        rows.sort_by(|a, b| if ascending { a.compare(b, column) } else { b.compare(a, column) });
        if *self.rows.borrow() == rows {
            return;
        }
        self.show_sort_arrow();
        let same_order = self.rows.borrow().iter().map(|r| &r.id).eq(rows.iter().map(|r| &r.id));
        if same_order {
            *self.rows.borrow_mut() = rows;
        } else {
            let selected = self.selected();
            *self.rows.borrow_mut() = rows;
            send(self.hwnd, LVM_SETITEMCOUNT, self.rows.borrow().len(), (LVSICF_NOSCROLL | LVSICF_NOINVALIDATEALL) as isize);
            self.set_selected(-1, false);
            for (i, row) in self.rows.borrow().iter().enumerate() {
                if selected.contains(&row.id) {
                    self.set_selected(i as i32, true);
                }
            }
            self.fit(); // a scroll bar may have come or gone
        }
        self.redraw(); // only the visible rows are asked for again
    }

    fn redraw(&self) {
        // SAFETY: plain call on our list
        unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); }
    }

    /// The name column takes the room the others leave, as on Linux.
    pub fn fit(&self) {
        let mut r = RECT::default();
        // SAFETY: plain query
        unsafe { let _ = GetClientRect(self.hwnd, &mut r); }
        let others: i32 = (1..COLUMNS.len()).map(|i| send(self.hwnd, LVM_GETCOLUMNWIDTH, i, 0) as i32).sum();
        let width = (r.right - others).max(px(160));
        if send(self.hwnd, LVM_GETCOLUMNWIDTH, 0, 0) as i32 != width {
            send(self.hwnd, LVM_SETCOLUMNWIDTH, 0, width as isize);
        }
    }

    /// The selected downloads' ids.
    pub fn selected(&self) -> Vec<String> {
        let rows = self.rows.borrow();
        let mut ids = Vec::new();
        let mut i = -1;
        loop {
            i = send(self.hwnd, LVM_GETNEXTITEM, i as usize, LVNI_SELECTED as isize) as i32;
            match rows.get(i.max(0) as usize).filter(|_| i >= 0) {
                Some(row) => ids.push(row.id.clone()),
                None => return ids,
            }
        }
    }

    pub fn select_all(&self) {
        self.set_selected(-1, true);
    }

    fn set_selected(&self, index: i32, on: bool) {
        let item = LVITEMW { stateMask: LVIS_SELECTED, state: if on { LVIS_SELECTED } else { LIST_VIEW_ITEM_STATE_FLAGS(0) },
                             ..Default::default() };
        send(self.hwnd, LVM_SETITEMSTATE, index as usize, &item as *const _ as isize);
    }

    /// The column it's sorted by now, and whether going up.
    pub fn sorting(&self) -> (usize, bool) {
        self.sort.get()
    }

    /// Sort by `column` (shown at the next refresh).
    pub fn sort_by(&self, column: usize, ascending: bool) {
        self.sort.set((column, ascending));
        self.show_sort_arrow();
    }

    /// The header's arrow on the sorted column.
    fn show_sort_arrow(&self) {
        let header = HWND(send(self.hwnd, LVM_GETHEADER, 0, 0) as *mut _);
        let (column, ascending) = self.sort.get();
        for i in 0..COLUMNS.len() {
            let mut item = HDITEMW { mask: HDI_FORMAT, ..Default::default() };
            send(header, HDM_GETITEMW, i, &mut item as *mut _ as isize);
            let mut format = item.fmt.0 & !(HDF_SORTUP.0 | HDF_SORTDOWN.0);
            if i == column {
                format |= if ascending { HDF_SORTUP.0 } else { HDF_SORTDOWN.0 };
            }
            let format = HEADER_CONTROL_FORMAT_FLAGS(format);
            if format != item.fmt {
                item.fmt = format;
                send(header, HDM_SETITEMW, i, &item as *const _ as isize);
            }
        }
    }
}

/// Fill in a row's text as the list asks for it (the name and status cells are drawn by us).
fn fill(rows: &[Row], item: &mut LVITEMW) {
    let Some(row) = rows.get(item.iItem.max(0) as usize).filter(|_| item.iItem >= 0) else { return };
    if item.mask.contains(LVIF_TEXT) && !item.pszText.is_null() && item.cchTextMax > 0 {
        let text: Vec<u16> = row.cells.get(item.iSubItem as usize).map_or("", |c| c.as_str()).encode_utf16()
            .take(item.cchTextMax as usize - 1).chain(Some(0)).collect();
        // SAFETY: the list's buffer holds cchTextMax characters; we write at most that many
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), item.pszText.0, text.len()) };
    }
    if item.mask.contains(LVIF_IMAGE) {
        item.iImage = I_IMAGENONE;
    }
}
