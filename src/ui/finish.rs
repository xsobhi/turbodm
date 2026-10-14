//! When a download finishes or fails: IDM's "Download complete" dialog, the per-download
//! "on completion" options, or a desktop notification for downloads nobody was watching.

use super::launch::{launch, Launch};
use super::power::{self, Power};
use super::{actions, progress, Ctx};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;
use turbodm::categories::icon_for;
use turbodm::engine::{Snapshot, Status};
use turbodm::text::finished_line;

pub use turbodm::power::OnDone;

pub fn on_status(ctx: &Rc<Ctx>, snap: Snapshot) {
    match snap.status {
        Status::Completed => completed(ctx, snap),
        Status::Error if ctx.manager.settings().notify_complete => {
            let body = format!("{}\n{}", snap.filename, snap.error.clone().unwrap_or_default());
            notify(ctx, &snap, "Download failed", &body, false);
        }
        _ => {}
    }
}

fn completed(ctx: &Rc<Ctx>, snap: Snapshot) {
    progress::close(ctx, &snap.id);
    // downloads with a progress window (now or earlier) get the dialog, like IDM
    let watched = ctx.on_done.borrow_mut().remove(&snap.id);
    let settings = ctx.manager.settings();
    if let Some(todo) = watched {
        if todo.open_file {
            actions::open_file(ctx, &snap.id);
        }
        if settings.show_complete_dialog && todo.power == Power::Nothing {
            complete_dialog(ctx, &snap);
        }
        power::countdown(ctx, todo.power, &snap.filename);
        if settings.show_complete_dialog {
            return;
        }
    }
    if settings.notify_complete {
        notify(ctx, &snap, "Download complete", &snap.filename, true);
    }
}

fn notify(ctx: &Ctx, snap: &Snapshot, title: &str, body: &str, buttons: bool) {
    let n = gio::Notification::new(title);
    n.set_body(Some(body));
    if buttons {
        n.add_button_with_target_value("Open", "app.open-file", Some(&snap.id.to_variant()));
        n.add_button_with_target_value("Show in folder", "app.open-folder", Some(&snap.id.to_variant()));
    }
    n.set_icon(&gio::ThemedIcon::new("folder-download"));
    ctx.app.send_notification(Some(&format!("tdm-{}", snap.id)), &n);
}

/// A name and its value, in the dialog's card ("Saved to", "From").
fn field(grid: &gtk::Grid, y: i32, name: &str, value: &str, cut: gtk::pango::EllipsizeMode) {
    grid.attach(&gtk::Label::builder().label(name).xalign(0.0).css_classes(["dim-label"]).build(), 0, y, 1, 1);
    let value = gtk::Label::builder().label(value).xalign(0.0).hexpand(true).ellipsize(cut).selectable(true).build();
    grid.attach(&value, 1, y, 1, 1);
}

/// What was downloaded and where it is, and what to do next: the file as a file manager shows
/// it (its icon, its name, its size and kind), where it is in a card, the choices at the bottom,
/// Open first among them. The same as on Windows.
fn complete_dialog(ctx: &Rc<Ctx>, snap: &Snapshot) {
    let icon = super::style::icon(icon_for(&snap.filename), 48);
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 4);
    heading.append(&gtk::Label::builder().label(&snap.filename).xalign(0.0).wrap(true).css_classes(["title-3"]).build());
    heading.append(&gtk::Label::builder().label(finished_line(snap)).xalign(0.0).css_classes(["dim-label"]).build());
    heading.set_valign(gtk::Align::Center);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    top.append(&icon);
    top.append(&heading);
    let grid = gtk::Grid::builder().row_spacing(8).column_spacing(18)
        .margin_top(12).margin_bottom(12).margin_start(14).margin_end(14).build();
    field(&grid, 0, "Saved to", &snap.path.display().to_string(), gtk::pango::EllipsizeMode::Middle); // keeps the name
    field(&grid, 1, "From", &snap.url, gtk::pango::EllipsizeMode::End);
    let card = gtk::Frame::builder().child(&grid).build();
    let file = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(20)
        .margin_top(24).margin_bottom(20).margin_start(24).margin_end(24).build();
    file.append(&top);
    file.append(&card);

    let again = gtk::CheckButton::with_label("Don't show this again");
    again.set_hexpand(true);
    let open = gtk::Button::builder().label("Open").css_classes(["suggested-action"]).build();
    let with = gtk::Button::with_label("Open with…");
    let folder = gtk::Button::with_label("Open folder");
    let bar = gtk::Box::builder().spacing(8).margin_top(14).margin_bottom(14).margin_start(20).margin_end(20).build();
    for w in [again.upcast_ref::<gtk::Widget>(), with.upcast_ref(), folder.upcast_ref(), open.upcast_ref()] {
        bar.append(w);
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&file);
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    body.append(&bar);
    let window = gtk::Window::builder().title("Download complete").default_width(560).resizable(false).child(&body).build();
    window.set_default_widget(Some(&open));
    let keys = gtk::EventControllerKey::new(); // Escape closes it, as in dialogs
    let w = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            w.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);

    let c = ctx.clone();
    window.connect_close_request(move |_| {
        if again.is_active() {
            let mut s = c.manager.settings();
            s.show_complete_dialog = false;
            c.manager.update_settings(s);
        }
        glib::Propagation::Proceed
    });
    let then_close = |w: &gtk::Window, f: Box<dyn Fn()>| {
        let w = w.clone();
        move |_: &gtk::Button| {
            f();
            w.close();
        }
    };
    let (c, id) = (ctx.clone(), snap.id.clone());
    open.connect_clicked(then_close(&window, Box::new(move || actions::open_file(&c, &id))));
    let (c, path) = (ctx.clone(), snap.path.clone());
    with.connect_clicked(then_close(&window, Box::new(move || launch(&c, &path, Launch::OpenWith))));
    let (c, id) = (ctx.clone(), snap.id.clone());
    folder.connect_clicked(then_close(&window, Box::new(move || actions::open_folder(&c, &id))));
    window.present();
    open.grab_focus();
}
