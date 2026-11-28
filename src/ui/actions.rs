//! What the menus, the toolbar, the shortcuts and the notifications do: the same commands as on
//! Windows (src/win/main/menu.rs).

use super::launch::{launch, Launch};
use super::{add_dialog, batch, dialogs, finish, list, progress, settings, Ctx};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::rc::Rc;
use turbodm::engine::{AddRequest, Status};

type IdAction = fn(&Rc<Ctx>, &str);

const GITHUB: &str = "https://github.com/xsobhi/turbodm";

fn selected(ctx: &Ctx) -> Vec<String> {
    list::selected_ids(&ctx.win.selection)
}

fn first(ctx: &Ctx) -> Option<String> {
    selected(ctx).into_iter().next()
}

fn add(ctx: &Rc<Ctx>, map: &impl IsA<gio::ActionMap>, name: &str, f: impl Fn(&Rc<Ctx>) + 'static) {
    let action = gio::SimpleAction::new(name, None);
    let weak = Rc::downgrade(ctx);
    action.connect_activate(move |_, _| {
        if let Some(ctx) = weak.upgrade() {
            f(&ctx);
        }
    });
    map.add_action(&action);
}

/// A setting with a tick in the menus (View's).
fn tick(ctx: &Rc<Ctx>, name: &str, get: fn(&turbodm::config::Settings) -> bool,
        set: fn(&mut turbodm::config::Settings, bool), after: fn(&Rc<Ctx>)) {
    let action = gio::SimpleAction::new_stateful(name, None, &get(&ctx.manager.settings()).to_variant());
    let weak = Rc::downgrade(ctx);
    action.connect_activate(move |action, _| {
        let Some(ctx) = weak.upgrade() else { return };
        let on = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
        action.set_state(&on.to_variant());
        let mut s = ctx.manager.settings();
        set(&mut s, on);
        ctx.manager.update_settings(s);
        after(&ctx);
    });
    ctx.win.window.add_action(&action);
}

pub fn install(ctx: &Rc<Ctx>) {
    let win = &ctx.win.window;
    // Tasks
    add(ctx, win, "add", |ctx| add_dialog::open_from_menu(ctx, AddRequest::default()));
    add(ctx, win, "add-video", add_dialog::add_video);
    add(ctx, win, "add-torrent", add_dialog::add_torrent_file);
    add(ctx, win, "batch-add", batch::pattern);
    add(ctx, win, "clipboard-links", batch::from_clipboard);
    add(ctx, win, "import", batch::import);
    add(ctx, win, "export", batch::export);
    add(ctx, &ctx.app, "quit", |ctx| ctx.app.quit());
    // File
    add(ctx, win, "open", |ctx| if let Some(id) = first(ctx) { open_file(ctx, &id) });
    add(ctx, win, "open-with", |ctx| {
        if let Some(snap) = first(ctx).and_then(|id| ctx.manager.get(&id)) {
            launch(ctx, &snap.path, Launch::OpenWith);
        }
    });
    add(ctx, win, "open-folder", |ctx| if let Some(id) = first(ctx) { open_folder(ctx, &id) });
    add(ctx, win, "complete-window", |ctx| {
        // (its window again, for one closed by mistake)
        if let Some(s) = first(ctx).and_then(|id| ctx.manager.get(&id)).filter(|s| s.status == Status::Completed) {
            finish::complete_dialog(ctx, &s);
        }
    });
    add(ctx, win, "redownload", |ctx| dialogs::confirm_redownload(ctx, selected(ctx)));
    add(ctx, win, "refresh", |ctx| {
        if let Some(s) = first(ctx).and_then(|id| ctx.manager.get(&id)).filter(|s| s.torrent.is_none()) {
            super::refresh::start(ctx, &s.id);
        }
    });
    add(ctx, win, "copy-url", |ctx| {
        if let Some(snap) = first(ctx).and_then(|id| ctx.manager.get(&id)) {
            ctx.win.window.clipboard().set_text(&snap.url);
        }
    });
    add(ctx, win, "details", |ctx| if let Some(id) = first(ctx) { progress::open(ctx, &id) });
    add(ctx, win, "remove", |ctx| dialogs::confirm_delete(ctx, selected(ctx), false)); // (asks, with "Also delete the files")
    add(ctx, win, "delete-file", |ctx| dialogs::confirm_delete(ctx, selected(ctx), true));
    // Downloads
    add(ctx, win, "resume", |ctx| selected(ctx).iter().for_each(|id| resume(ctx, id)));
    add(ctx, win, "pause", |ctx| selected(ctx).iter().for_each(|id| ctx.manager.pause(id)));
    add(ctx, win, "pause-resume", pause_or_resume);
    add(ctx, win, "resume-all", |ctx| ctx.manager.resume_all());
    add(ctx, win, "pause-all", |ctx| ctx.manager.pause_all());
    add(ctx, win, "delete-completed", |ctx| {
        // from the list only: the files stay
        ctx.manager.snapshot().iter().filter(|s| s.status == Status::Completed)
            .for_each(|s| ctx.manager.remove(&s.id, false));
    });
    add(ctx, win, "find", |ctx| {
        ctx.win.search.grab_focus();
        ctx.win.search.select_region(0, -1);
    });
    let speed = gio::SimpleAction::new_stateful("speed-limit", Some(glib::VariantTy::INT64),
                                                &(ctx.manager.settings().speed_limit_kib as i64).to_variant());
    let weak = Rc::downgrade(ctx);
    speed.connect_activate(move |action, kib| {
        let (Some(ctx), Some(kib)) = (weak.upgrade(), kib.and_then(|k| k.get::<i64>())) else { return };
        action.set_state(&kib.to_variant());
        let mut s = ctx.manager.settings();
        s.speed_limit_kib = kib.max(0) as u64;
        ctx.manager.update_settings(s);
    });
    win.add_action(&speed);
    add(ctx, win, "open-download-folder", |ctx| {
        let dir = ctx.manager.settings().download_dir;
        let _ = std::fs::create_dir_all(&dir);
        launch(ctx, &dir, Launch::Open);
    });
    add(ctx, win, "settings", settings::open);
    // View
    tick(ctx, "show-categories", |s| s.show_categories, |s, on| s.show_categories = on, show_parts);
    tick(ctx, "show-toolbar", |s| s.show_toolbar, |s, on| s.show_toolbar = on, show_parts);
    tick(ctx, "toolbar-text", |s| s.toolbar_text, |s, on| s.toolbar_text = on,
         |ctx| ctx.win.toolbar.show_text(ctx.manager.settings().toolbar_text));
    tick(ctx, "show-progress", |s| s.show_progress_window, |s, on| s.show_progress_window = on, |_| {});
    tick(ctx, "show-complete", |s| s.show_complete_dialog, |s, on| s.show_complete_dialog = on, |_| {});
    let sort = gio::SimpleAction::new_stateful("sort", Some(glib::VariantTy::INT32), &5i32.to_variant());
    let weak = Rc::downgrade(ctx);
    sort.connect_activate(move |action, column| {
        let (Some(ctx), Some(column)) = (weak.upgrade(), column.and_then(|c| c.get::<i32>())) else { return };
        // the same again turns it around, as clicking a column's header does
        let (now, up) = list::sorting(&ctx.win.view);
        let column = column as usize;
        let ascending = if now == column { !up } else { column == 0 || column == 2 || column == 4 };
        list::sort_by(&ctx.win.view, column, ascending);
        action.set_state(&(column as i32).to_variant());
    });
    win.add_action(&sort);
    // Help
    add(ctx, win, "github", |ctx| open_uri(ctx, GITHUB));
    add(ctx, win, "report", |ctx| open_uri(ctx, &format!("{GITHUB}/issues/new")));
    add(ctx, win, "browsers", super::browsers::open);
    add(ctx, win, "youtube", super::youtube::open);
    add(ctx, win, "check-updates", |ctx| super::update::check(ctx, true));
    add(ctx, win, "about", dialogs::about);
    add(ctx, win, "select-all", |ctx| list::select_all(&ctx.win.selection));
    add(ctx, &ctx.app, "show", |ctx| ctx.show_all());
    // targeted, for notifications' buttons: app.open-file('<id>')
    let targeted: [(&str, IdAction); 2] = [("open-file", open_file), ("open-folder", open_folder)];
    for (name, f) in targeted {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        let weak = Rc::downgrade(ctx);
        action.connect_activate(move |_, param| {
            if let (Some(ctx), Some(id)) = (weak.upgrade(), param.and_then(|p| p.str().map(String::from))) {
                f(&ctx, &id);
            }
        });
        ctx.app.add_action(&action);
    }
    shortcuts(ctx);
    update_enabled(ctx);
}

/// The main window's keyboard shortcuts, as on Windows: not while typing in the search (Alt and
/// a menu's letter open the menus, as GTK's menu bar does).
fn shortcuts(ctx: &Rc<Ctx>) {
    use gdk::{Key, ModifierType as M};
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(ctx);
    keys.connect_key_pressed(move |_, key, _, state| {
        let Some(ctx) = weak.upgrade() else { return glib::Propagation::Proceed };
        let typing = gtk::prelude::GtkWindowExt::focus(&ctx.win.window).is_some_and(|w| w.is::<gtk::Text>());
        if typing {
            return glib::Propagation::Proceed;
        }
        let ctrl = state.contains(M::CONTROL_MASK);
        let action = match (ctrl, key.to_lower()) {
            (true, Key::n) => "add",
            (true, Key::r) => "resume",
            (true, Key::p) => "pause",
            (true, Key::i) => "details",
            (true, Key::a) => "select-all",
            (true, Key::comma) => "settings",
            (true, Key::f) => "find",
            (true, Key::q) => return activate_app(&ctx, "quit"),
            (false, Key::Delete | Key::KP_Delete) => "remove",
            (false, Key::F5) => "resume-all",
            (false, Key::space) if !state.intersects(M::SHIFT_MASK | M::ALT_MASK) => "pause-resume",
            _ => return glib::Propagation::Proceed,
        };
        let _ = WidgetExt::activate_action(&ctx.win.window, &format!("win.{action}"), None);
        glib::Propagation::Stop
    });
    ctx.win.window.add_controller(keys);
}

fn activate_app(ctx: &Ctx, name: &str) -> glib::Propagation {
    ctx.app.activate_action(name, None);
    glib::Propagation::Stop
}

/// The commands greyed for what's selected now, as on Windows.
pub fn update_enabled(ctx: &Ctx) {
    let selected: Vec<_> = selected(ctx).iter().filter_map(|id| ctx.manager.get(id)).collect();
    let any = !selected.is_empty();
    let done = selected.first().is_some_and(|s| s.status == Status::Completed);
    let torrent = selected.first().is_some_and(|s| s.torrent.is_some()); // (no address to refresh)
    let states = [("open", done), ("open-with", done), ("complete-window", done), ("open-folder", any),
                  ("redownload", any), ("refresh", any && !torrent), ("copy-url", any), ("details", any),
                  ("remove", any), ("delete-file", any), ("resume", any), ("pause", any), ("pause-resume", any)];
    for (name, on) in states {
        if let Some(action) = ctx.win.window.lookup_action(name).and_downcast::<gio::SimpleAction>()
            && action.is_enabled() != on {
                action.set_enabled(on);
            }
    }
    let column = list::sorting(&ctx.win.view).0 as i32;
    if let Some(action) = ctx.win.window.lookup_action("sort").and_downcast::<gio::SimpleAction>()
        && action.state().and_then(|s| s.get::<i32>()) != Some(column) {
            action.set_state(&column.to_variant());
        }
    let speed = ctx.manager.settings().speed_limit_kib as i64;
    if let Some(action) = ctx.win.window.lookup_action("speed-limit").and_downcast::<gio::SimpleAction>()
        && action.state().and_then(|s| s.get::<i64>()) != Some(speed) {
            action.set_state(&speed.to_variant());
        }
}

/// View's options changed: show the sidebar and the toolbar, or not.
fn show_parts(ctx: &Rc<Ctx>) {
    let s = ctx.manager.settings();
    ctx.win.show_parts(s.show_categories, s.show_toolbar);
}

/// Space: pause the selected downloads if any is under way (or waiting its turn), else resume them.
fn pause_or_resume(ctx: &Rc<Ctx>) {
    let ids = selected(ctx);
    let busy = ids.iter().filter_map(|id| ctx.manager.get(id))
        .any(|s| s.status.is_active() || s.status == Status::Queued);
    for id in &ids {
        if busy { ctx.manager.pause(id) } else { resume(ctx, id) }
    }
}

/// Resume and, like IDM, show the download's progress window.
fn resume(ctx: &Rc<Ctx>, id: &str) {
    ctx.manager.resume(id);
    if ctx.manager.settings().show_progress_window {
        progress::open(ctx, id);
    }
}

/// Open a finished download's file (or say it isn't there any more).
pub fn open_file(ctx: &Rc<Ctx>, id: &str) {
    let Some(snap) = ctx.manager.get(id) else { return };
    if snap.status == Status::Completed && !snap.path.exists() {
        dialogs::missing_file(ctx, id);
        return;
    }
    launch(ctx, &snap.path, Launch::Open);
}

pub fn open_folder(ctx: &Rc<Ctx>, id: &str) {
    let Some(snap) = ctx.manager.get(id) else { return };
    if snap.path.exists() {
        launch(ctx, &snap.path, Launch::ShowInFolder);
    } else {
        launch(ctx, &snap.directory, Launch::Open); // file gone: just open the folder
    }
}

/// A web page, in the default browser.
pub fn open_uri(ctx: &Ctx, uri: &str) {
    let context = gdk::Display::default().map(|d| d.app_launch_context());
    if let Err(err) = gio::AppInfo::launch_default_for_uri(uri, context.as_ref()) {
        eprintln!("turbodm: open {uri}: {err}");
    }
    let _ = ctx;
}
