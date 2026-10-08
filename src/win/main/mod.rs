//! The main window, like IDM's: menu bar, toolbar, categories on the left, downloads on the
//! right, a status bar; closing it keeps TurboDM running in the notification area.

mod cells;
mod clipboard;
mod events;
mod list;
pub mod menu;
mod toolbar;
mod tree;

use super::wnd::{self, px, Window};
pub use events::{message_loop, setup};
use super::{progress, shell, App};
use menu::Cmd;
use std::cell::Cell;
use std::rc::Rc;
use turbodm::engine::Status;
use turbodm::util::human_speed;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct MainWindow {
    pub window: Rc<Window>,
    pub list: list::List,
    tree: tree::Tree,
    toolbar: HWND,
    status: HWND,
    split: Rc<Cell<i32>>, // width of the categories, 96-DPI pixels
}


impl MainWindow {
    pub fn new() -> Self {
        let window = Window::new("TurboDM", WS_OVERLAPPEDWINDOW, WINDOW_EX_STYLE(0), None, (1000, 600));
        // SAFETY: our window takes the menu bar
        unsafe { let _ = SetMenu(window.hwnd(), Some(menu::bar())); }
        let toolbar = toolbar::create(&window);
        let refresh_soon = || super::post(refresh);
        let tree = tree::Tree::new(&window, refresh_soon);
        let list = list::List::new(&window, |id| super::post(move |app| open_row(app, &id)),
                                   |point| menu::context(super::app().main.window.hwnd(), point), refresh_soon);
        let status = window.control(STATUSCLASSNAMEW, "", WINDOW_STYLE(SBARS_SIZEGRIP), WINDOW_EX_STYLE(0)).0;
        let main = MainWindow { window, list, tree, toolbar, status, split: Rc::new(Cell::new(210)) };
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

    /// Place everything in the window's inside (screen pixels).
    pub(super) fn layout(&self, width: i32, height: i32) {
        let bar = toolbar::height(self.toolbar);
        let status = {
            wnd::send(self.status, WM_SIZE, 0, 0);
            let mut rect = Default::default();
            // SAFETY: plain query
            unsafe { let _ = GetWindowRect(self.status, &mut rect); }
            rect.bottom - rect.top
        };
        let split = px(self.split.get());
        let gap = px(4);
        // SAFETY: plain moves of our controls
        unsafe {
            let _ = SetWindowPos(self.toolbar, None, 0, 0, width, bar, SWP_NOZORDER);
            let body = (height - bar - status).max(0);
            let _ = SetWindowPos(self.tree.hwnd, None, 0, bar, split, body, SWP_NOZORDER);
            let _ = SetWindowPos(self.list.hwnd, None, split + gap, bar, (width - split - gap).max(0), body, SWP_NOZORDER);
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

/// The list, the counts, the status bar, the toolbar and the progress windows.
pub fn refresh(app: &Rc<App>) {
    let main = &app.main;
    if main.is_visible() {
        let snapshots = app.manager.snapshot();
        main.tree.update(&snapshots);
        let view = main.tree.view.get();
        let shown: Vec<_> = snapshots.into_iter().filter(|s| view.matches(s)).collect();
        main.list.update(&shown);
        let (all, active, queued) = app.manager.counts();
        let speed = human_speed(app.manager.total_speed());
        wnd::set_text(main.status, &format!(
            "  {all} download{} · {active} active{}{}", if all == 1 { "" } else { "s" },
            if queued > 0 { format!(" · {queued} queued") } else { String::new() },
            if speed.is_empty() { String::new() } else { format!(" · {speed}") }));
        let any = !main.list.selected().is_empty();
        for cmd in [Cmd::Resume, Cmd::Pause, Cmd::Remove] {
            toolbar::enable(main.toolbar, cmd, any);
        }
    }
    progress::refresh_all(app);
}
