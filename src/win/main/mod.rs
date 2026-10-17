//! The main window, as on Linux Mint (and like IDM's): the title bar with search, IDM's menu bar,
//! the toolbar, the sidebar, the downloads list, a status line; closing it keeps TurboDM in the
//! notification area.

mod cells;
mod clipboard;
mod draw;
mod events;
mod list;
pub mod menu;
mod menubar;
mod sidebar;
mod toolbar;

use super::theme::{self, Align, Font};
use super::wnd::caption::{self, Button, Caption};
use super::wnd::{self, canvas, field, px, send, Window};
pub use events::{message_loop, setup};
use super::{progress, shell, App};
use menu::Cmd;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::engine::Status;
use turbodm::util::human_speed;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Controls::EM_SETCUEBANNER;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct MainWindow {
    pub window: Rc<Window>,
    pub list: list::List,
    caption: Caption,
    sidebar: sidebar::Sidebar,
    toolbar: toolbar::Toolbar,
    menubar: menubar::MenuBar,
    pub search: HWND,
    status: HWND,
    status_text: Rc<RefCell<String>>,
    split: Rc<Cell<i32>>, // width of the sidebar, 96-DPI pixels
    shown: Cell<(bool, bool)>, // (categories, toolbar): View's options
}

/// The splitter between the sidebar and the list, and the status line (96-DPI pixels).
const GAP: i32 = 5;
const STATUS: i32 = 30;

impl MainWindow {
    pub fn new() -> Self {
        let window = Window::new("TurboDM", WS_OVERLAPPEDWINDOW, WINDOW_EX_STYLE(0), None, (1180, 700));
        let caption = Caption::new(&window, &[Button::Minimize, Button::Maximize, Button::Close]);
        window.caption.set(caption.title);
        window.own_title_bar(caption::HEIGHT);
        let toolbar = toolbar::Toolbar::new(&window);
        let refresh_soon = || super::post(refresh);
        let search = window.edit("", refresh_soon);
        send(search, EM_SETCUEBANNER, 1, HSTRING::from("Search downloads").as_ptr() as isize);
        let menubar = menubar::MenuBar::new(&window);
        let sidebar = sidebar::Sidebar::new(&window, refresh_soon);
        let list = list::List::new(&window, |id| super::post(move |app| open_row(app, &id)),
                                   |point| menu::context(super::app().main.window.hwnd(), point), refresh_soon);
        let status_text: Rc<RefCell<String>> = Rc::default();
        let text = status_text.clone();
        let status = window.canvas(move |p| {
            p.fill(p.rect, theme::CHROME);
            let r = RECT { left: px(12), right: p.rect.right - px(12), ..p.rect };
            p.text(r, &text.borrow(), Font::Body, theme::TEXT, Align::Left);
        });
        let main = MainWindow { window, list, caption, sidebar, toolbar, menubar, search, status, status_text,
                                split: Rc::new(Cell::new(210)), shown: Cell::new((true, true)) };
        wnd::let_edges_resize(main.window.hwnd()); // (Windows 10: no frame outside to grab)
        wnd::center(main.window.hwnd());
        main
    }

    pub fn show(&self) {
        // SAFETY: plain calls on our window
        unsafe {
            if IsIconic(self.window.hwnd()).as_bool() {
                let _ = ShowWindow(self.window.hwnd(), SW_RESTORE);
            }
        }
        self.window.show();
        super::post(refresh);
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: plain query
        unsafe { IsWindowVisible(self.window.hwnd()).as_bool() }
    }

    /// Where the splitter is (screen pixels, in the window): the strip between sidebar and list
    /// (none while the categories are hidden).
    pub(super) fn grip(&self, height: i32) -> RECT {
        let (split, top) = (px(self.sidebar_width()), self.body_top());
        let right = if split > 0 { split + px(GAP) } else { 0 };
        RECT { left: split, top, right, bottom: height - px(STATUS) }
    }

    /// The sidebar's width now (96-DPI pixels): 0 while View → Categories is off.
    fn sidebar_width(&self) -> i32 {
        if self.shown.get().0 { self.split.get() } else { 0 }
    }

    /// Where the sidebar and the list start (screen pixels): under the title bar, the menu bar
    /// and the toolbar (if it's shown).
    fn body_top(&self) -> i32 {
        let toolbar = if self.shown.get().1 { toolbar::height() } else { 0 };
        px(caption::HEIGHT + menubar::HEIGHT + toolbar)
    }

    /// The toolbar's names under its icons, or not (View → Toolbar text), and lay out again.
    pub fn toolbar_text(&self, on: bool) {
        self.toolbar.show_text(on);
        self.relayout();
    }

    /// Show the categories and the toolbar, or not (View's options), and lay out again.
    pub fn show_parts(&self, categories: bool, toolbar: bool) {
        self.shown.set((categories, toolbar));
        self.relayout();
    }

    fn relayout(&self) {
        let (w, h) = wnd::client_size(self.window.hwnd());
        self.layout(px(w), px(h));
        wnd::moves::paint_now(self.window.hwnd());
    }

    /// Place everything in the window's inside (screen pixels).
    pub(super) fn layout(&self, width: i32, height: i32) {
        // the title bar (with the search, as GTK's header bar), the menu bar, the toolbar, the rest
        let bar = px(caption::HEIGHT);
        let menus = bar + px(menubar::HEIGHT);
        let top = self.body_top();
        let bottom = (height - px(STATUS)).max(top);
        let split = px(self.sidebar_width());
        let buttons = width - self.caption.buttons_width();
        let search = RECT { left: buttons - px(8) - px(260), top: (bar - px(32)) / 2, right: buttons - px(8),
                            bottom: (bar + px(32)) / 2 };
        let place = wnd::moves::move_window;
        let show = |hwnd: HWND, on: bool| {
            // SAFETY: plain calls on our own controls
            unsafe { let _ = ShowWindow(hwnd, if on { SW_SHOWNA } else { SW_HIDE }); }
        };
        wnd::batch(|| {
            self.caption.place(width, search.left - px(8));
            field::place_field(self.search, search);
            place(self.menubar.hwnd, RECT { left: 0, top: bar, right: width, bottom: menus });
            show(self.toolbar.hwnd, top > menus);
            place(self.toolbar.hwnd, RECT { left: 0, top: menus, right: width, bottom: top.max(menus) });
            // as on Mint: white content between grey bars, thin lines between the parts (the one
            // between sidebar and list in a white strip that moves it)
            let body = top + 1;
            let gap = if split > 0 { px(GAP) } else { 0 };
            let line = split + gap / 2;
            let mut surfaces = vec![(RECT { left: 0, top, right: width, bottom: body }, theme::BORDER),
                                    (RECT { left: 0, top: bottom, right: width, bottom: bottom + 1 }, theme::BORDER)];
            if split > 0 {
                surfaces.push((RECT { left: split, top: body, right: split + gap, bottom }, theme::VIEW));
                surfaces.push((RECT { left: line, top: body, right: line + 1, bottom }, theme::BORDER));
            }
            self.window.set_surfaces(surfaces);
            show(self.sidebar.hwnd, split > 0);
            place(self.sidebar.hwnd, RECT { left: 0, top: body, right: split, bottom });
            place(self.list.hwnd, RECT { left: split + gap, top: body, right: width, bottom });
            place(self.status, RECT { left: 0, top: bottom + 1, right: width, bottom: height });
        });
        self.list.fit(); // (by its new size)
    }

    fn set_status(&self, text: String) {
        if *self.status_text.borrow() != text {
            *self.status_text.borrow_mut() = text;
            canvas::redraw(self.status);
        }
    }
}

/// Finished downloads dragged out of the list: their files go along, to drop anywhere.
pub(super) fn drag_out(app: &Rc<App>) {
    let paths: Vec<std::path::PathBuf> = app.main.list.selected().iter().filter_map(|id| app.manager.get(id))
        .filter(|s| s.status == Status::Completed).map(|s| s.path).collect();
    if !paths.is_empty() {
        shell::drag_files(app.main.window.hwnd(), &paths);
    }
}

/// Double-click or Enter on a download: open it when it's done, else its progress window.
pub(super) fn open_row(app: &Rc<App>, id: &str) {
    match app.manager.get(id).map(|s| s.status) {
        Some(Status::Completed) => shell::open_download(app, id),
        Some(_) => progress::open(app, id),
        None => {}
    }
}

/// The list (the sidebar's view, searched), the counts, the status line, the toolbar and the
/// progress windows.
pub fn refresh(app: &Rc<App>) {
    let main = &app.main;
    if main.is_visible() {
        let snapshots = app.manager.snapshot();
        main.sidebar.update(&snapshots, &app.manager.settings().download_dir);
        let (view, query) = (main.sidebar.view.get(), wnd::text(main.search).trim().to_lowercase());
        let shown: Vec<_> = snapshots.iter().filter(|s| view.matches(s))
            .filter(|s| query.is_empty() || s.filename.to_lowercase().contains(&query)).cloned().collect();
        let empty = match (snapshots.is_empty(), query.is_empty()) {
            (true, _) => "No downloads yet\n\nClick Add URL (Ctrl+N), copy a link, or download from your browser",
            (_, false) => "No downloads match your search",
            _ => "Nothing here",
        };
        main.list.update(&shown, empty);
        let (all, active, queued) = app.manager.counts();
        let speed = human_speed(app.manager.total_speed());
        main.set_status(format!(
            "{all} download{} · {active} active{}{}", if all == 1 { "" } else { "s" },
            if queued > 0 { format!(" · {queued} queued") } else { String::new() },
            if speed.is_empty() { String::new() } else { format!(" · {speed}") }));
        let any = !main.list.selected().is_empty();
        for cmd in [Cmd::Resume, Cmd::Pause, Cmd::Remove] {
            main.toolbar.enable(cmd, any);
        }
    }
    progress::refresh_all(app);
}
