//! When a download finishes or fails, as on Windows: IDM's "Download complete" dialog, the per-download
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
        Status::Error => {
            if ctx.manager.settings().notify_complete {
                let body = format!("{}\n{}", snap.filename, snap.error.clone().unwrap_or_default());
                notify(ctx, &snap, "Download failed", &body, false);
            }
            super::refresh::offer(ctx, &snap); // (an expired link: get a fresh one)
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
        power::countdown(ctx, todo.power, &format!("“{}” has finished downloading.", snap.filename));
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
        // (a click on it opens the file, as on Windows)
        n.set_default_action_and_target_value("app.open-file", Some(&snap.id.to_variant()));
        n.add_button_with_target_value("Open", "app.open-file", Some(&snap.id.to_variant()));
        n.add_button_with_target_value("Show in folder", "app.open-folder", Some(&snap.id.to_variant()));
    }
    n.set_icon(&gio::ThemedIcon::new("folder-download"));
    ctx.app.send_notification(Some(&format!("tdm-{}", snap.id)), &n);
}

/// A name and its value, in the dialog's card ("Saved to", "From").
fn field(grid: &gtk::Grid, y: i32, name: &str, value: &str, cut: gtk::pango::EllipsizeMode) {
    grid.attach(&gtk::Label::builder().label(name).xalign(0.0).css_classes(["dim-label"]).width_request(70)
        .height_request(26).build(), 0, y, 1, 1);
    let value = gtk::Label::builder().label(value).xalign(0.0).hexpand(true).ellipsize(cut).max_width_chars(10).build();
    grid.attach(&value, 1, y, 1, 1);
}

/// "Download complete", as on Windows (src/win/complete.rs): the file on the content's colour,
/// as a file manager shows files (its big icon, its name, its size and kind), where it is in a
/// card; the choices in a bar at the bottom, on two rows as IDM's: Open, Open with… and Open
/// folder on the left with Close on the right; under them, on a row half as tall, "Don't show this
/// dialog again", and at the right a small square that drags the file out: into a folder, a chat,
/// anywhere files go.
pub fn complete_dialog(ctx: &Rc<Ctx>, snap: &Snapshot) {
    let icon = gtk::Image::builder().icon_name(icon_for(&snap.filename)).pixel_size(48).valign(gtk::Align::Start).build();
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 0);
    heading.append(&gtk::Label::builder().label(&snap.filename).xalign(0.0).css_classes(["title-3"])
        .ellipsize(gtk::pango::EllipsizeMode::End).max_width_chars(10).hexpand(true).height_request(28).build());
    heading.append(&gtk::Label::builder().label(finished_line(snap)).xalign(0.0).css_classes(["dim-label"])
        .height_request(22).build());
    let top = gtk::Box::builder().spacing(16).height_request(52).build();
    top.append(&icon);
    top.append(&heading);
    let grid = gtk::Grid::builder().column_spacing(6).margin_top(9).margin_bottom(9).margin_start(14).margin_end(14).build();
    field(&grid, 0, "Saved to", &snap.path.display().to_string(), gtk::pango::EllipsizeMode::Middle); // keeps the name
    field(&grid, 1, "From", &snap.url, gtk::pango::EllipsizeMode::End);
    let card = gtk::Box::builder().css_classes(["card"]).margin_top(20).build();
    card.append(&grid);
    let file = gtk::Box::builder().orientation(gtk::Orientation::Vertical).css_classes(["page"]).build();
    let inner = gtk::Box::builder().orientation(gtk::Orientation::Vertical).margin_top(24).margin_bottom(20)
        .margin_start(24).margin_end(24).width_request(512).build();
    inner.append(&top);
    inner.append(&card);
    file.append(&inner);

    let d = super::frame::dialog_with("Download complete", None, 560, ":close", (0, 0));
    d.body.set_margin_bottom(0);
    let window = d.window.clone();
    let open = super::frame::default_button(&window, "Open");
    let with = super::frame::button("Open with…");
    let folder = super::frame::button("Open folder");
    let close = super::frame::button("Close");
    let first = gtk::Box::builder().spacing(8).build();
    first.append(&open);
    first.append(&with);
    first.append(&folder);
    first.append(&gtk::Box::builder().hexpand(true).build());
    first.append(&close);
    let again = gtk::CheckButton::builder().label("Don't show this dialog again").hexpand(true).build();
    let second = gtk::Box::builder().margin_top(12).build();
    second.append(&again);
    second.append(&drag_square(&snap.path));
    let footer = gtk::Box::builder().orientation(gtk::Orientation::Vertical).margin_top(16).margin_bottom(16)
        .margin_start(20).margin_end(20).build();
    footer.append(&first);
    footer.append(&second);
    d.body.append(&file);
    d.body.append(&gtk::Separator::builder().css_classes(["rule"]).build());
    d.body.append(&footer);

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
    let w = window.clone();
    close.connect_clicked(move |_| w.close());
    window.present();
}

/// The drag square: a small picture of a file with the "move" arrows over it (as IDM's has);
/// dragged, the file goes along with the mouse, to drop anywhere.
fn drag_square(path: &std::path::Path) -> gtk::Widget {
    let square = gtk::Box::builder().css_classes(["drag-square"]).width_request(24).height_request(16)
        .valign(gtk::Align::Center).halign(gtk::Align::End).hexpand(false).tooltip_text("Drag the file to a folder, a chat\u{2026} anywhere files go")
        .cursor(&gtk::gdk::Cursor::from_name("move", None).unwrap_or_else(|| gtk::gdk::Cursor::from_name("default", None).unwrap()))
        .build();
    let picture = super::style::picture("drag", 14);
    picture.set_halign(gtk::Align::Center);
    square.append(&picture);
    let source = gtk::DragSource::builder().actions(gtk::gdk::DragAction::COPY).build();
    let file = gio::File::for_path(path);
    source.connect_prepare(move |_, _, _| {
        let list = gtk::gdk::FileList::from_array(std::slice::from_ref(&file));
        Some(gtk::gdk::ContentProvider::for_value(&list.to_value()))
    });
    square.add_controller(source);
    square.upcast()
}
