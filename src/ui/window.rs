//! The main window, as on Windows (and like IDM's): the title bar with the search, the menu bar,
//! the toolbar, the sidebar, the downloads list, a status line; closing it keeps TurboDM in the
//! notification area.

use super::sidebar::{Sidebar, View};
use super::toolbar::Toolbar;
use super::{list, menus, progress, style, toolbar, Ctx};
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
    pub search: gtk::Entry,
    pub toolbar: Toolbar,
    toolbar_rule: gtk::Separator,
    sidebar: Sidebar,
    filter: gtk::CustomFilter,
    pages: gtk::Stack, // the list, or a note when it's empty
    empty: gtk::Label,
}

impl MainWindow {
    pub fn new(app: &gtk::Application, settings: &turbodm::config::Settings) -> Self {
        let store = gio::ListStore::new::<super::item::DownloadItem>();
        let search = gtk::Entry::builder().placeholder_text("Search downloads").width_request(260)
            .valign(gtk::Align::Center).margin_end(8).build();
        let view_of = Rc::new(Cell::new(View::All));
        let (v, q) = (view_of.clone(), search.clone());
        let filter = gtk::CustomFilter::new(move |obj| {
            let Some(item) = obj.downcast_ref::<super::item::DownloadItem>() else { return false };
            let query = q.text().trim().to_lowercase();
            v.get().matches(&item.state(), &item.category())
                && (query.is_empty() || item.name().to_lowercase().contains(&query))
        });
        let f = filter.clone();
        search.connect_changed(move |_| f.changed(gtk::FilterChange::Different));
        let f = filter.clone();
        let sidebar = Sidebar::new(view_of, move || f.changed(gtk::FilterChange::Different));

        let menu = gtk::PopoverMenu::from_model(Some(&menus::context()));
        menu.set_has_arrow(false);
        menu.set_halign(gtk::Align::Start);
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

        // the status line: 30 pixels, its text 12 from the edge
        let status = gtk::Label::builder().xalign(0.0).margin_start(12).margin_end(12).hexpand(true).build();
        let status_line = gtk::Box::builder().css_classes(["statusbar"]).height_request(30).build();
        status_line.append(&status);
        let scroller = gtk::ScrolledWindow::builder().child(&view).vexpand(true).hexpand(true).build();
        let (empty_box, empty) = toolbar::empty_page();
        let pages = gtk::Stack::builder().css_classes(["list-area"]).build();
        pages.add_named(&scroller, Some("list"));
        pages.add_named(&empty_box, Some("empty"));
        sidebar.widget.set_visible(settings.show_categories);
        let paned = gtk::Paned::builder().orientation(gtk::Orientation::Horizontal)
            .start_child(&sidebar.widget).end_child(&pages).css_classes(["main-split"])
            .shrink_start_child(false).resize_start_child(false).position(210).vexpand(true).build();
        // the sidebar 160 to 500 wide, as on Windows
        paned.connect_position_notify(|paned| {
            let clamped = paned.position().clamp(160, 500);
            if paned.position() != clamped {
                paned.set_position(clamped);
            }
        });
        let menubar = gtk::PopoverMenuBar::from_model(Some(&menus::bar()));
        let menubar_row = gtk::Box::builder().css_classes(["menubar-row"]).height_request(28).build();
        menubar_row.append(&menubar);
        let toolbar = Toolbar::new(settings.toolbar_text);
        toolbar.widget.set_visible(settings.show_toolbar);
        let toolbar_rule = rule();
        let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).width_request(760)
            .height_request(360).build();
        content.append(&menubar_row);
        content.append(&toolbar.widget);
        content.append(&toolbar_rule);
        content.append(&paned);
        content.append(&rule());
        content.append(&status_line);
        let window = gtk::ApplicationWindow::builder()
            .application(app).title(turbodm::config::APP_NAME).default_width(1180).default_height(700)
            .icon_name("turbodm").child(&content).build();
        let (header, _) = style::title_bar(turbodm::config::APP_NAME, ":minimize,maximize,close");
        header.pack_end(&search);
        window.set_titlebar(Some(&header));
        MainWindow { window, store, view, selection, status, search, toolbar, toolbar_rule, sidebar, filter, pages, empty }
    }

    /// Show the categories and the toolbar, or not (View's options).
    pub fn show_parts(&self, categories: bool, toolbar: bool) {
        self.sidebar.widget.set_visible(categories);
        self.toolbar.widget.set_visible(toolbar);
        self.toolbar_rule.set_visible(true); // (the line under the menus, or under the toolbar)
    }
}

/// A 1-pixel line between the parts.
fn rule() -> gtk::Separator {
    gtk::Separator::builder().orientation(gtk::Orientation::Horizontal).css_classes(["rule"]).build()
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
        // started hidden by the browser: quit when idle and nothing is shown (any open window
        // counts), giving the browser a few seconds to deliver the download it started us for
        let any_window = gtk::Window::list_toplevels().iter().any(|w| w.is_visible());
        let idle = !ctx.manager.busy() && ctx.launching.get() == 0;
        if ctx.background && !any_window && idle && started.elapsed() > Duration::from_secs(5) {
            ctx.app.quit();
        }
        glib::ControlFlow::Continue
    });
    let weak = Rc::downgrade(ctx);
    ctx.win.view.connect_activate(move |_, pos| {
        let Some(ctx) = weak.upgrade() else { return };
        let Some(item) = ctx.win.selection.item(pos).and_downcast::<super::item::DownloadItem>() else { return };
        open_row(&ctx, &item.id());
    });
    let weak = Rc::downgrade(ctx);
    ctx.win.selection.connect_selection_changed(move |_, _, _| {
        if let Some(ctx) = weak.upgrade() {
            super::actions::update_enabled(&ctx);
        }
    });
    let weak = Rc::downgrade(ctx);
    ctx.win.window.connect_close_request(move |window| {
        let Some(ctx) = weak.upgrade() else { return glib::Propagation::Proceed };
        match (ctx.tray.get(), ctx.manager.busy()) {
            (true, _) => window.set_visible(false), // in the notification area, as on Windows
            (false, true) => window.minimize(),     // no tray on this panel: stay reachable
            (false, false) => ctx.app.quit(),
        }
        glib::Propagation::Stop
    });
    // the files of downloads moved or deleted meanwhile, when the window comes back
    let weak = Rc::downgrade(ctx);
    ctx.win.window.connect_is_active_notify(move |window| {
        if let (Some(ctx), true) = (weak.upgrade(), window.is_active()) {
            let manager = ctx.manager.clone();
            ctx.rt.spawn_blocking(move || manager.check_files());
        }
    });
}

/// Double-click or Enter on a download: open it when it's done, else its progress window.
pub fn open_row(ctx: &Rc<Ctx>, id: &str) {
    match ctx.manager.get(id).map(|s| s.status) {
        Some(Status::Completed) => super::actions::open_file(ctx, id),
        Some(_) => progress::open(ctx, id),
        None => {}
    }
}

/// Everything: the whole list and the progress windows.
pub fn refresh(ctx: &Ctx) {
    if ctx.win.window.is_visible() { // hidden in the tray: nothing to draw
        let snapshots = ctx.manager.snapshot();
        let moved = list::sync(&ctx.win.store, &snapshots);
        ctx.win.sidebar.update(&snapshots, &ctx.manager.settings().download_dir);
        lists_changed(ctx, moved);
        status_bar(ctx);
        super::actions::update_enabled(ctx);
    }
    refresh_progress(ctx);
    super::queue::update(ctx); // (how far the hidden ones are)
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
    let text = match (win.store.n_items(), win.search.text().trim().is_empty()) {
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
    let text = format!(
        "{all} download{} · {active} active{}{}",
        if all == 1 { "" } else { "s" },
        if queued > 0 { format!(" · {queued} queued") } else { String::new() },
        if speed.is_empty() { String::new() } else { format!(" · {speed}") }
    );
    if ctx.win.status.text() != text {
        ctx.win.status.set_text(&text);
    }
}
