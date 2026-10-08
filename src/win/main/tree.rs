//! The category tree on the left, like IDM's: all downloads by file type, and by state.

use super::super::wnd::{instance, px, send, Window};
use crate::win::icons;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::engine::{Snapshot, Status};
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// What the downloads list shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    All,
    Active,     // downloading or waiting in the queue
    Unfinished, // everything not complete
    Completed,
    Category(&'static str),
}

impl View {
    pub fn matches(self, s: &Snapshot) -> bool {
        match self {
            View::All => true,
            View::Active => s.status.is_active() || s.status == Status::Queued,
            View::Unfinished => s.status != Status::Completed,
            View::Completed => s.status == Status::Completed,
            View::Category(c) => s.category == c,
        }
    }
}

/// (view, name, icon, under "All downloads")
const ITEMS: [(View, &str, u16, bool); 11] = [
    (View::All, "All downloads", icons::ALL, false),
    (View::Category("Compressed"), "Compressed", icons::COMPRESSED, true),
    (View::Category("Documents"), "Documents", icons::DOCUMENTS, true),
    (View::Category("Music"), "Music", icons::MUSIC, true),
    (View::Category("Programs"), "Programs", icons::PROGRAMS, true),
    (View::Category("Video"), "Video", icons::VIDEO, true),
    (View::Category("Images"), "Images", icons::IMAGES, true),
    (View::Category("General"), "Other", icons::OTHER, true),
    (View::Active, "Downloading", icons::ACTIVE, false),
    (View::Unfinished, "Unfinished", icons::UNFINISHED, false),
    (View::Completed, "Completed", icons::COMPLETED, false),
];

pub struct Tree {
    pub hwnd: HWND,
    items: Vec<HTREEITEM>,
    names: RefCell<Vec<String>>, // shown now, with counts
    pub view: Rc<Cell<View>>, // the one picked
}

impl Tree {
    pub fn new(window: &Window, on_change: impl Fn() + 'static) -> Self {
        let style = WINDOW_STYLE(TVS_HASBUTTONS | TVS_LINESATROOT | TVS_SHOWSELALWAYS | TVS_FULLROWSELECT | TVS_TRACKSELECT)
            | WS_TABSTOP;
        let (hwnd, id) = window.control(WC_TREEVIEWW, "", style, WS_EX_CLIENTEDGE);
        let size = px(16);
        // SAFETY: Explorer's look for the tree (as in Explorer on this Windows), and our icons
        unsafe {
            let _ = SetWindowTheme(hwnd, w!("Explorer"), PCWSTR::null());
            let images = ImageList_Create(size, size, ILC_COLOR32 | ILC_MASK, ITEMS.len() as i32, 0);
            for (_, _, icon, _) in ITEMS {
                let handle = LoadImageW(Some(instance()), PCWSTR(icon as usize as *const u16), IMAGE_ICON, size, size,
                                        LR_DEFAULTCOLOR).unwrap_or_default();
                ImageList_ReplaceIcon(images, -1, HICON(handle.0));
            }
            send(hwnd, TVM_SETIMAGELIST, TVSIL_NORMAL as usize, images.0 as isize);
        }
        send(hwnd, TVM_SETITEMHEIGHT, px(24) as usize, 0);
        let mut items = Vec::new();
        let mut parent = TVI_ROOT;
        for (i, (_, name, _, child)) in ITEMS.iter().enumerate() {
            let text = HSTRING::from(*name);
            let insert = TVINSERTSTRUCTW {
                hParent: if *child { parent } else { TVI_ROOT },
                hInsertAfter: TVI_LAST,
                Anonymous: TVINSERTSTRUCTW_0 { item: TVITEMW {
                    mask: TVIF_TEXT | TVIF_IMAGE | TVIF_SELECTEDIMAGE | TVIF_PARAM,
                    pszText: PWSTR(text.as_ptr() as *mut _),
                    iImage: i as i32,
                    iSelectedImage: i as i32,
                    lParam: windows::Win32::Foundation::LPARAM(i as isize),
                    ..Default::default()
                } },
            };
            let item = HTREEITEM(send(hwnd, TVM_INSERTITEMW, 0, &insert as *const _ as isize));
            if !child {
                parent = item;
            }
            items.push(item);
        }
        send(hwnd, TVM_EXPAND, TVE_EXPAND.0 as usize, items[0].0);
        send(hwnd, TVM_SELECTITEM, TVGN_CARET as usize, items[0].0);
        let view = Rc::new(Cell::new(View::All));
        let picked = view.clone();
        window.on_notify(id, move |header, lparam| {
            if header.code == TVN_SELCHANGEDW {
                // SAFETY: TVN_SELCHANGED comes with an NMTREEVIEW
                let changed = unsafe { &*(lparam.0 as *const NMTREEVIEWW) };
                let index = changed.itemNew.lParam.0 as usize;
                if let Some((view, ..)) = ITEMS.get(index) {
                    picked.set(*view);
                    on_change();
                }
            }
            None
        });
        Tree { hwnd, items, names: RefCell::new(ITEMS.iter().map(|i| i.1.to_string()).collect()), view }
    }

    /// Counts next to the names, like "Video (3)".
    pub fn update(&self, snapshots: &[Snapshot]) {
        let mut names = self.names.borrow_mut();
        for (i, (view, name, ..)) in ITEMS.iter().enumerate() {
            let n = snapshots.iter().filter(|s| view.matches(s)).count();
            let text = if n == 0 { name.to_string() } else { format!("{name} ({n})") };
            if names[i] != text {
                let wide = HSTRING::from(text.as_str());
                let item = TVITEMW { mask: TVIF_TEXT, hItem: self.items[i], pszText: PWSTR(wide.as_ptr() as *mut _),
                                     ..Default::default() };
                send(self.hwnd, TVM_SETITEMW, 0, &item as *const _ as isize);
                names[i] = text;
            }
        }
    }
}
