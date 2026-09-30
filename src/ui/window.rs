//! Main window: toolbar, downloads table, status bar, refresh loop, close-to-tray.

use super::{list, progress, Ctx};
use gtk::prelude::*;
use gtk::{gio, glib};
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
}

fn tool_button(icon: &str, tooltip: &str, action: &str) -> gtk::Button {
    gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).action_name(action).build()
}

impl MainWindow {
    pub fn new(app: &gtk::Application) -> Self {
        let store = gio::ListStore::new::<super::item::DownloadItem>();
        let menu = gtk::PopoverMenu::from_model(Some(&context_menu()));
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
        let (view, selection) = list::build(&store, on_menu);
        menu.set_parent(&view);

        let header = gtk::HeaderBar::new();
        for (icon, tip, action) in [
            ("list-add-symbolic", "Add URL (Ctrl+N)", "win.add"),
            ("media-playback-start-symbolic", "Resume (Ctrl+R)", "win.resume"),
            ("media-playback-pause-symbolic", "Pause (Ctrl+P)", "win.pause"),
            ("user-trash-symbolic", "Remove (Delete)", "win.remove"),
        ] {
            header.pack_start(&tool_button(icon, tip, action));
        }
        let app_menu = gio::Menu::new();
        app_menu.append(Some("Resume all"), Some("win.resume-all"));
        app_menu.append(Some("Pause all"), Some("win.pause-all"));
        app_menu.append(Some("Preferences"), Some("win.settings"));
        app_menu.append(Some("About TurboDM"), Some("win.about"));
        app_menu.append(Some("Quit"), Some("app.quit"));
        header.pack_end(&gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&app_menu).build());

        let status = gtk::Label::builder().xalign(0.0).margin_start(10).margin_end(10)
            .margin_top(4).margin_bottom(4).build();
        let scroller = gtk::ScrolledWindow::builder().child(&view).vexpand(true).build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&scroller);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&status);
        let window = gtk::ApplicationWindow::builder()
            .application(app).title("TurboDM").default_width(1000).default_height(560)
            .icon_name("turbodm").child(&content).build();
        window.set_titlebar(Some(&header));
        MainWindow { window, store, view, selection, status }
    }
}

fn context_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let open = gio::Menu::new();
    open.append(Some("Open"), Some("win.open"));
    open.append(Some("Open folder"), Some("win.open-folder"));
    open.append(Some("Progress details"), Some("win.details"));
    menu.append_section(None, &open);
    let control = gio::Menu::new();
    control.append(Some("Resume"), Some("win.resume"));
    control.append(Some("Pause"), Some("win.pause"));
    control.append(Some("Refresh download address…"), Some("win.refresh"));
    control.append(Some("Copy address"), Some("win.copy-url"));
    menu.append_section(None, &control);
    let remove = gio::Menu::new();
    remove.append(Some("Remove from list"), Some("win.remove"));
    remove.append(Some("Delete with file"), Some("win.delete-file"));
    menu.append_section(None, &remove);
    menu
}

/// Refresh loop, double-click, and close-to-tray behaviour.
pub fn setup(ctx: &Rc<Ctx>) {
    refresh(ctx);
    let weak = Rc::downgrade(ctx);
    let started = Instant::now();
    glib::timeout_add_local(Duration::from_millis(500), move || {
        let Some(ctx) = weak.upgrade() else { return glib::ControlFlow::Break };
        refresh(&ctx);
        // started hidden by the browser, or closed while downloading: quit when idle. Any open
        // window (add dialog, progress, preferences…) counts, and the browser gets a few
        // seconds to deliver the download it started us for.
        let any_window = gtk::Window::list_toplevels().iter().any(|w| w.is_visible());
        if !any_window && !ctx.manager.busy() && started.elapsed() > Duration::from_secs(5) {
            ctx.app.quit();
        }
        glib::ControlFlow::Continue
    });
    let weak = Rc::downgrade(ctx);
    ctx.win.view.connect_activate(move |_, pos| {
        let Some(ctx) = weak.upgrade() else { return };
        let Some(item) = ctx.win.store.item(pos).and_downcast::<super::item::DownloadItem>() else { return };
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

fn refresh(ctx: &Ctx) {
    let snapshots = ctx.manager.snapshot();
    list::sync(&ctx.win.store, &snapshots);
    let active = snapshots.iter().filter(|s| s.status.is_active()).count();
    let speed = human_speed(ctx.manager.total_speed());
    let queued = snapshots.iter().filter(|s| s.status == Status::Queued).count();
    ctx.win.status.set_text(&format!(
        "{} download{} · {active} active{}{}",
        snapshots.len(),
        if snapshots.len() == 1 { "" } else { "s" },
        if queued > 0 { format!(" · {queued} queued") } else { String::new() },
        if speed.is_empty() { String::new() } else { format!(" · {speed}") }
    ));
    for window in ctx.progress.borrow().values() {
        window.refresh(&ctx.manager);
    }
}
