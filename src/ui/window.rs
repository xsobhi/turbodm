//! Main window: toolbar, sidebar, downloads table, status bar, refresh loop, close-to-tray.

use super::sidebar::{Sidebar, View};
use super::{list, progress, toolbar, Ctx};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};
use turbodm::engine::Status;
use turbodm::util::human_speed;

pub struct MainWindow {
    pub window: gtk::ApplicationWindow,
    pub store: gio::ListStore,
    pub view: gtk::ColumnView,
    pub selection: gtk::MultiSelection,
    pub status: gtk::Label,
    pub search: gtk::SearchEntry,
    sidebar: Sidebar,
    filter: gtk::CustomFilter,
    pages: gtk::Stack, // the list, or a note when it's empty
    empty: gtk::Label,
}

impl MainWindow {
    pub fn new(app: &gtk::Application) -> Self {
        let store = gio::ListStore::new::<super::item::DownloadItem>();
        let search = gtk::SearchEntry::builder().placeholder_text("Search downloads").width_chars(24).build();
        let view_of = Rc::new(Cell::new(View::All));
        let (v, q) = (view_of.clone(), search.clone());
        let filter = gtk::CustomFilter::new(move |obj| {
            let Some(item) = obj.downcast_ref::<super::item::DownloadItem>() else { return false };
            let query = q.text().to_lowercase();
            v.get().matches(&item.state(), &item.category())
                && (query.is_empty() || item.name().to_lowercase().contains(query.trim()))
        });
        let f = filter.clone();
        search.connect_search_changed(move |_| f.changed(gtk::FilterChange::Different));
        let f = filter.clone();
        let sidebar = Sidebar::new(view_of, move || f.changed(gtk::FilterChange::Different));

        let menu = gtk::PopoverMenu::from_model(Some(&toolbar::context_menu()));
        menu.set_has_arrow(false);
        let menu_for_rows = menu.clone();
        let on_menu: list::MenuHandler = Rc::new(move |_pos, widget, x, y| {
            let Some(parent) = menu_for_rows.parent() else { return };
            let point = widget.compute_point(&parent, &gtk::graphene::Point::new(x as f32, y as f32));
            if let Some(p) = point {
                menu_for_rows.set_pointing_to(Some(&gtk::gdk::Rectangle::new(p.x() as i32, p.y() as i32, 1, 1)));
                menu_for_rows.popup();
            }
        });
        let (view, selection) = list::build(&store, &filter, on_menu);
        menu.set_parent(&view);

        let header = gtk::HeaderBar::new();
        header.pack_end(&gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&toolbar::app_menu()).build());
        header.pack_end(&search);

        let status = gtk::Label::builder().xalign(0.0).margin_start(10).margin_end(10)
            .margin_top(4).margin_bottom(4).build();
        let scroller = gtk::ScrolledWindow::builder().child(&view).vexpand(true).hexpand(true).build();
        let (empty_box, empty) = toolbar::empty_page();
        let pages = gtk::Stack::new();
        pages.add_named(&scroller, Some("list"));
        pages.add_named(&empty_box, Some("empty"));
        let paned = gtk::Paned::builder().orientation(gtk::Orientation::Horizontal)
            .start_child(&sidebar.widget).end_child(&pages)
            .shrink_start_child(false).resize_start_child(false).position(210).vexpand(true).build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&toolbar::toolbar());
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&paned);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&status);
        let window = gtk::ApplicationWindow::builder()
            .application(app).title("TurboDM").default_width(1180).default_height(700)
            .icon_name("turbodm").child(&content).build();
        window.set_titlebar(Some(&header));
        MainWindow { window, store, view, selection, status, search, sidebar, filter, pages, empty }
    }
}

/// How often running downloads update (their rows, progress windows, status bar): often enough
/// to look live, like IDM. The rest of the list only changes now and then.
const REFRESH: Duration = Duration::from_millis(50);
const FULL_EVERY: u32 = 10; // ticks between full list passes: new, removed, finished downloads

/// Refresh loop, double-click, and close-to-tray behaviour.
pub fn setup(ctx: &Rc<Ctx>) {
    let weak = Rc::downgrade(ctx);
    let started = Instant::now();
    let (mut ticks, mut live) = (0u32, HashSet::new());
    glib::timeout_add_local(REFRESH, move || {
        let Some(ctx) = weak.upgrade() else { return glib::ControlFlow::Break };
        ticks = ticks.wrapping_add(1);
        if ticks % FULL_EVERY == 0 {
            refresh(&ctx);
        } else {
            refresh_running(&ctx, &mut live);
        }
        // started hidden by the browser, or closed while downloading: quit when idle. Any open
        // window (add dialog, progress, preferences…) counts, and the browser gets a few
        // seconds to deliver the download it started us for.
        let any_window = gtk::Window::list_toplevels().iter().any(|w| w.is_visible());
        let idle = !ctx.manager.busy() && ctx.launching.get() == 0;
        if !any_window && idle && started.elapsed() > Duration::from_secs(5) {
            ctx.app.quit();
        }
        glib::ControlFlow::Continue
    });
    let weak = Rc::downgrade(ctx);
    ctx.win.view.connect_activate(move |_, pos| {
        let Some(ctx) = weak.upgrade() else { return };
        let Some(item) = ctx.win.selection.item(pos).and_downcast::<super::item::DownloadItem>() else { return };
        match ctx.manager.get(&item.id()).map(|s| s.status) {
            Some(Status::Completed) => super::actions::open_file(&ctx, &item.id()),
            Some(_) => progress::open(&ctx, &item.id()),
            None => {}
        }
    });
    let weak = Rc::downgrade(ctx);
    ctx.win.window.connect_close_request(move |window| {
        let Some(ctx) = weak.upgrade() else { return glib::Propagation::Proceed };
        match (ctx.manager.busy(), ctx.tray.get()) {
            (true, true) => window.set_visible(false), // keep downloading; the tray brings it back
            (true, false) => window.minimize(),        // no tray on this panel: stay reachable
            (false, _) => ctx.app.quit(),
        }
        glib::Propagation::Stop
    });
}

/// Everything: the whole list and the progress windows.
pub fn refresh(ctx: &Ctx) {
    if ctx.win.window.is_visible() { // hidden in the tray: nothing to draw
        let snapshots = ctx.manager.snapshot();
        let moved = list::sync(&ctx.win.store, &snapshots);
        ctx.win.sidebar.update(&snapshots, &ctx.manager.settings().download_dir);
        lists_changed(ctx, moved);
        status_bar(ctx);
    }
    refresh_progress(ctx);
}

/// Just what's moving: rows of running downloads (and of ones that just stopped).
fn refresh_running(ctx: &Ctx, live: &mut HashSet<String>) {
    if ctx.win.window.is_visible() {
        let snapshots = ctx.manager.snapshot_where(|id, s| s.is_active() || live.contains(id));
        // a stopped download's speed reads 0 one engine tick later: keep its row until then
        *live = snapshots.iter().filter(|s| s.status.is_active() || s.speed > 0.0)
            .map(|s| s.id.clone()).collect();
        match list::update(&ctx.win.store, &snapshots) {
            (true, moved) => lists_changed(ctx, moved),
            (false, _) => refresh(ctx), // a new download: add its row
        }
        status_bar(ctx);
    }
    refresh_progress(ctx);
}

fn refresh_progress(ctx: &Ctx) {
    // close windows of removed downloads after the loop (closing edits ctx.progress)
    let gone: Vec<gtk::Window> = ctx.progress.borrow().values()
        .filter(|p| !p.refresh(&ctx.manager)).map(|p| p.window.clone()).collect();
    gone.iter().for_each(|w| w.close());
}

/// Rows moved between sidebar views: filter again; and say so when the list is empty.
fn lists_changed(ctx: &Ctx, moved: bool) {
    let win = &ctx.win;
    if moved {
        win.filter.changed(gtk::FilterChange::Different);
    }
    let empty = win.selection.n_items() == 0;
    let text = match (win.store.n_items(), win.search.text().is_empty()) {
        (0, _) => "No downloads yet\n\nClick Add URL (Ctrl+N), copy a link, or download from your browser",
        (_, false) => "No downloads match your search",
        _ => "Nothing here",
    };
    if win.empty.label() != text {
        win.empty.set_label(text);
    }
    win.pages.set_visible_child_name(if empty { "empty" } else { "list" });
}

fn status_bar(ctx: &Ctx) {
    let (all, active, queued) = ctx.manager.counts();
    let speed = human_speed(ctx.manager.total_speed());
    ctx.win.status.set_text(&format!(
        "{all} download{} · {active} active{}{}",
        if all == 1 { "" } else { "s" },
        if queued > 0 { format!(" · {queued} queued") } else { String::new() },
        if speed.is_empty() { String::new() } else { format!(" · {speed}") }
    ));
}
