//! The main window, as on Linux Mint (and like IDM's): the toolbar with search and the menu,
//! the sidebar, the downloads list, a status line; closing it keeps TurboDM in the notification
//! area.

mod caption;
mod cells;
mod clipboard;
mod draw;
mod events;
mod list;
pub mod menu;
mod sidebar;
mod toolbar;

use super::theme::{self, icon, Align, Font};
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
    caption: caption::Caption,
    sidebar: sidebar::Sidebar,
    toolbar: toolbar::Toolbar,
    pub search: HWND,
    menu: HWND,
    status: HWND,
    status_text: Rc<RefCell<String>>,
    split: Rc<Cell<i32>>, // width of the sidebar, 96-DPI pixels
}

/// The splitter between the sidebar and the list, and the status line (96-DPI pixels).
const GAP: i32 = 5;
const STATUS: i32 = 30;

impl MainWindow {
    pub fn new() -> Self {
        let window = Window::new("TurboDM", WS_OVERLAPPEDWINDOW, WINDOW_EX_STYLE(0), None, (1180, 700));
        window.own_title_bar(caption::HEIGHT);
        let caption = caption::Caption::new(&window);
        let toolbar = toolbar::Toolbar::new(&window);
        let refresh_soon = || super::post(refresh);
        let search = window.edit("", refresh_soon);
        send(search, EM_SETCUEBANNER, 1, HSTRING::from("Search downloads").as_ptr() as isize);
        let menu = window.icon_button(icon::MENU, || super::post(|app| menu::app_menu(app)));
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
        let main = MainWindow { window, list, caption, sidebar, toolbar, search, menu, status, status_text,
                                split: Rc::new(Cell::new(210)) };
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

    /// Where the splitter is (screen pixels, in the window): the strip between sidebar and list.
    pub(super) fn grip(&self, height: i32) -> RECT {
        let split = px(self.split.get());
        RECT { left: split, top: px(caption::HEIGHT + toolbar::HEIGHT), right: split + px(GAP), bottom: height - px(STATUS) }
    }

    /// Place everything in the window's inside (screen pixels).
    pub(super) fn layout(&self, width: i32, height: i32) {
        // the title bar (with the search and the menu, as GTK's header bar), the toolbar, the rest
        let bar = px(caption::HEIGHT);
        let (top, bottom) = (bar + px(toolbar::HEIGHT), (height - px(STATUS)).max(bar + px(toolbar::HEIGHT)));
        let split = px(self.split.get());
        let buttons = width - caption::buttons_width();
        let menu = RECT { left: buttons - px(42), top: (bar - px(34)) / 2, right: buttons - px(8), bottom: (bar + px(34)) / 2 };
        let search = RECT { left: menu.left - px(8) - px(260), top: (bar - px(32)) / 2, right: menu.left - px(8),
                            bottom: (bar + px(32)) / 2 };
        let place = |hwnd: HWND, r: RECT| {
            // SAFETY: plain moves of our controls
            unsafe { let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER); }
        };
        self.caption.place(width, search.left - px(8));
        field::place_field(self.search, search);
        place(self.menu, menu);
        place(self.toolbar.hwnd, RECT { left: 0, top: bar, right: width, bottom: top });
        // as on Mint: white content between grey bars, thin lines between the parts (the one
        // between sidebar and list in a white strip that moves it)
        let (body, line) = (top + 1, split + px(GAP) / 2);
        self.window.set_surfaces(vec![
            (RECT { left: 0, top, right: width, bottom: body }, theme::BORDER),
            (RECT { left: split, top: body, right: split + px(GAP), bottom }, theme::VIEW),
            (RECT { left: line, top: body, right: line + 1, bottom }, theme::BORDER),
            (RECT { left: 0, top: bottom, right: width, bottom: bottom + 1 }, theme::BORDER),
        ]);
        place(self.sidebar.hwnd, RECT { left: 0, top: body, right: split, bottom });
        place(self.list.hwnd, RECT { left: split + px(GAP), top: body, right: width, bottom });
        self.list.fit();
        place(self.status, RECT { left: 0, top: bottom + 1, right: width, bottom: height });
    }

    pub fn menu_button(&self) -> HWND {
        self.menu
    }

    fn set_status(&self, text: String) {
        if *self.status_text.borrow() != text {
            *self.status_text.borrow_mut() = text;
            canvas::redraw(self.status);
        }
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
