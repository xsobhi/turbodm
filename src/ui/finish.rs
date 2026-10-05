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
use turbodm::util::human_size;

/// Chosen in the progress window's "Options on completion" tab.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnDone {
    pub open_file: bool,
    pub power: Power,
}

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

fn field(grid: &gtk::Grid, y: i32, name: &str, value: &str) {
    grid.attach(&gtk::Label::builder().label(name).xalign(1.0).css_classes(["dim-label"]).build(), 0, y, 1, 1);
    let value = gtk::Label::builder().label(value).xalign(0.0).hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle).build();
    grid.attach(&value, 1, y, 1, 1);
}

fn complete_dialog(ctx: &Rc<Ctx>, snap: &Snapshot) {
    let icon = gtk::Image::builder().icon_name(icon_for(&snap.filename)).pixel_size(48).build();
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
    heading.append(&gtk::Label::builder().label("Download complete").xalign(0.0).css_classes(["title-3"]).build());
    heading.append(&gtk::Label::builder().label(&snap.filename).xalign(0.0).wrap(true).build());
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    top.append(&icon);
    top.append(&heading);
    let grid = gtk::Grid::builder().row_spacing(6).column_spacing(14).build();
    field(&grid, 0, "Address", &snap.url);
    field(&grid, 1, "File size", &human_size(snap.size));
    field(&grid, 2, "Saved to", &snap.path.display().to_string());
    let again = gtk::CheckButton::with_label("Don't show this dialog again");
    let buttons = gtk::Box::builder().spacing(8).halign(gtk::Align::End).build();
    let open = gtk::Button::builder().label("Open").css_classes(["suggested-action"]).build();
    let with = gtk::Button::with_label("Open with…");
    let folder = gtk::Button::with_label("Open folder");
    let close = gtk::Button::with_label("Close");
    for b in [&open, &with, &folder, &close] {
        buttons.append(b);
    }
    let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(14)
        .margin_top(18).margin_bottom(18).margin_start(18).margin_end(18).build();
    body.append(&top);
    body.append(&grid);
    body.append(&again);
    body.append(&buttons);
    let window = gtk::Window::builder().title("Download complete").default_width(560).child(&body).build();
    window.set_default_widget(Some(&open));

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
    close.connect_clicked(then_close(&window, Box::new(|| {})));
    window.present();
    open.grab_focus(); // not the address field (it would show up selected)
}
