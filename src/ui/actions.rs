//! Actions behind the toolbar, context menu, shortcuts and notification buttons.

use super::{add_dialog, list, progress, settings, Ctx};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;
use turbodm::engine::AddRequest;

type IdAction = fn(&Rc<Ctx>, &str);

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

pub fn install(ctx: &Rc<Ctx>) {
    let win = &ctx.win.window;
    add(ctx, win, "add", |ctx| add_dialog::open(ctx, AddRequest::default()));
    add(ctx, win, "resume", |ctx| selected(ctx).iter().for_each(|id| ctx.manager.resume(id)));
    add(ctx, win, "pause", |ctx| selected(ctx).iter().for_each(|id| ctx.manager.pause(id)));
    add(ctx, win, "resume-all", |ctx| ctx.manager.resume_all());
    add(ctx, win, "pause-all", |ctx| ctx.manager.pause_all());
    add(ctx, win, "remove", |ctx| selected(ctx).iter().for_each(|id| ctx.manager.remove(id, false)));
    add(ctx, win, "delete-file", confirm_delete);
    add(ctx, win, "open", |ctx| if let Some(id) = first(ctx) { open_file(ctx, &id) });
    add(ctx, win, "open-folder", |ctx| if let Some(id) = first(ctx) { open_folder(ctx, &id) });
    add(ctx, win, "details", |ctx| if let Some(id) = first(ctx) { progress::open(ctx, &id) });
    add(ctx, win, "copy-url", copy_url);
    add(ctx, win, "refresh", refresh_address);
    add(ctx, win, "settings", settings::open);
    add(ctx, win, "about", about);
    add(ctx, &ctx.app, "quit", |ctx| ctx.app.quit());
    add(ctx, &ctx.app, "show", |ctx| ctx.show());
    // Targeted actions used by notification buttons: app.open-file('<id>')
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
    for (action, accel) in [("win.add", "<Ctrl>n"), ("win.resume", "<Ctrl>r"), ("win.pause", "<Ctrl>p"),
                            ("win.remove", "Delete"), ("win.settings", "<Ctrl>comma"),
                            ("win.details", "<Ctrl>i"), ("app.quit", "<Ctrl>q")] {
        ctx.app.set_accels_for_action(action, &[accel]);
    }
}

pub fn open_file(ctx: &Rc<Ctx>, id: &str) {
    let Some(snap) = ctx.manager.get(id) else { return };
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&snap.path)));
    launcher.launch(Some(&ctx.win.window), gio::Cancellable::NONE, |_| {});
}

pub fn open_folder(ctx: &Rc<Ctx>, id: &str) {
    let Some(snap) = ctx.manager.get(id) else { return };
    let target = if snap.path.exists() { snap.path } else { snap.directory };
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(target)));
    launcher.open_containing_folder(Some(&ctx.win.window), gio::Cancellable::NONE, |_| {});
}

fn copy_url(ctx: &Rc<Ctx>) {
    if let Some(snap) = first(ctx).and_then(|id| ctx.manager.get(&id)) {
        ctx.win.window.clipboard().set_text(&snap.url);
    }
}

fn confirm_delete(ctx: &Rc<Ctx>) {
    let ids = selected(ctx);
    if ids.is_empty() {
        return;
    }
    let dialog = gtk::AlertDialog::builder()
        .message(format!("Delete {} download(s) and their files?", ids.len()))
        .detail("Downloaded files will be removed from disk.")
        .buttons(["Cancel", "Delete"])
        .cancel_button(0)
        .default_button(0)
        .build();
    let ctx2 = ctx.clone();
    dialog.choose(Some(&ctx.win.window), gio::Cancellable::NONE, move |choice| {
        if choice == Ok(1) {
            ids.iter().for_each(|id| ctx2.manager.remove(id, true));
        }
    });
}

/// IDM's "Refresh download address": paste a fresh link for an expired one.
fn refresh_address(ctx: &Rc<Ctx>) {
    let Some(snap) = first(ctx).and_then(|id| ctx.manager.get(&id)) else { return };
    let entry = gtk::Entry::builder().text(&snap.url).hexpand(true).build();
    let hint = gtk::Label::builder().wrap(true).xalign(0.0)
        .label(format!("New link for “{}”. Progress is kept if the server returns the same file.", snap.filename))
        .build();
    let ok = gtk::Button::builder().label("Refresh").css_classes(["suggested-action"]).build();
    let cancel = gtk::Button::with_label("Cancel");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&ok);
    let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12)
        .margin_top(16).margin_bottom(16).margin_start(16).margin_end(16).build();
    body.append(&hint);
    body.append(&entry);
    body.append(&buttons);
    let dialog = gtk::Window::builder().title("Refresh download address").modal(true)
        .transient_for(&ctx.win.window).default_width(560).child(&body).build();
    let (d, c, id) = (dialog.clone(), ctx.clone(), snap.id.clone());
    let apply = move || {
        let url = entry.text().trim().to_string();
        if url.starts_with("http") {
            c.manager.refresh_address(&id, url);
        }
        d.close();
    };
    let apply = Rc::new(apply);
    let a = apply.clone();
    ok.connect_clicked(move |_| a());
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    dialog.present();
}

fn about(ctx: &Rc<Ctx>) {
    gtk::AboutDialog::builder()
        .transient_for(&ctx.win.window)
        .modal(true)
        .program_name("TurboDM")
        .logo_icon_name("turbodm")
        .version(turbodm::config::VERSION)
        .comments("Fast multi-connection download manager for Linux, with browser integration.")
        .website("https://github.com/xsobhi/turbodm")
        .license_type(gtk::License::MitX11)
        .build()
        .present();
}
