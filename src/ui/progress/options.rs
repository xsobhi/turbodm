//! The "Speed limiter" and "Options on completion" tabs of the progress window.

use super::super::finish::OnDone;
use super::super::power::Power;
use super::Ctx;
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::engine::Snapshot;

fn note(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).wrap(true).css_classes(["dim-label"]).build()
}

pub fn speed_limiter(ctx: &Rc<Ctx>, snap: &Snapshot) -> gtk::Box {
    let limited = snap.speed_limit_kib > 0;
    let enable = gtk::CheckButton::builder().label("Use speed limiter").active(limited).build();
    let speed = gtk::SpinButton::with_range(1.0, 10_000_000.0, 64.0);
    speed.set_value(if limited { snap.speed_limit_kib as f64 } else { 1024.0 });
    speed.set_sensitive(limited);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&gtk::Label::new(Some("Maximum download speed")));
    row.append(&speed);
    row.append(&gtk::Label::new(Some("KiB/s")));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.append(&enable);
    body.append(&row);
    body.append(&note("Applies to this download only, right away. The limit for all downloads \
                       together is in Preferences."));
    let apply = {
        let (c, id, enable, speed) = (ctx.clone(), snap.id.clone(), enable.clone(), speed.clone());
        move || {
            speed.set_sensitive(enable.is_active());
            let kib = if enable.is_active() { speed.value() as u64 } else { 0 };
            c.manager.set_speed_limit(&id, kib);
        }
    };
    let a = apply.clone();
    enable.connect_toggled(move |_| a());
    speed.connect_value_changed(move |_| apply());
    body
}

pub fn on_completion(ctx: &Rc<Ctx>, id: &str) -> gtk::Box {
    let todo = ctx.on_done.borrow().get(id).copied().unwrap_or_default();
    let dialog = gtk::CheckButton::builder().label("Show download complete dialog")
        .active(ctx.manager.settings().show_complete_dialog).build();
    let open = gtk::CheckButton::builder().label("Open the file when done").active(todo.open_file).build();
    let power = gtk::DropDown::from_strings(&["Do nothing", "Sleep", "Shut down"]);
    power.set_selected(todo.power as u32);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&gtk::Label::new(Some("When done, the computer should")));
    row.append(&power);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.append(&dialog);
    body.append(&open);
    body.append(&row);
    body.append(&note("Sleep and shut down wait 30 seconds first, so you can still cancel."));

    let c = ctx.clone();
    dialog.connect_toggled(move |b| {
        let mut s = c.manager.settings();
        s.show_complete_dialog = b.is_active();
        c.manager.update_settings(s);
    });
    let store = {
        let (c, id, open, power) = (ctx.clone(), id.to_string(), open.clone(), power.clone());
        move || {
            let todo = OnDone { open_file: open.is_active(), power: Power::from_index(power.selected()) };
            c.on_done.borrow_mut().insert(id.clone(), todo);
        }
    };
    let s = store.clone();
    open.connect_toggled(move |_| s());
    power.connect_selected_notify(move |_| store());
    body
}
