//! The "Speed limiter" and "Options on completion" tabs of the progress window, as on Windows.

use super::super::finish::OnDone;
use super::super::frame;
use super::super::power::Power;
use super::Ctx;
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::engine::Snapshot;

fn note(text: &str) -> gtk::Label {
    let label = frame::note(text);
    label.add_css_class("dim-label");
    label
}

pub fn speed_limiter(ctx: &Rc<Ctx>, snap: &Snapshot) -> gtk::Box {
    let limited = snap.speed_limit_kib > 0;
    let torrent = snap.torrent.is_some();
    // (librqbit can't change one torrent's limit while it runs)
    let enable = gtk::CheckButton::builder().label("Use speed limiter").active(limited).sensitive(!torrent).build();
    let speed = gtk::SpinButton::with_range(1.0, 10_000_000.0, 64.0);
    speed.set_value(if limited { snap.speed_limit_kib as f64 } else { 1024.0 });
    speed.set_sensitive(limited);
    speed.set_size_request(130, frame::FIELD);
    let row = gtk::Box::builder().spacing(5).margin_top(16).build();
    row.append(&gtk::Label::builder().label("Maximum download speed").width_request(170).xalign(0.0).build());
    row.append(&speed);
    row.append(&gtk::Label::builder().label("KiB/s").margin_start(5).build());
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&enable);
    body.append(&row);
    let text = note(if torrent {
        "A torrent goes by the limit for all downloads together, in Preferences."
    } else {
        "Applies to this download only, right away. The limit for all downloads together is in Preferences."
    });
    text.set_margin_top(16);
    body.append(&text);
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
    let open = gtk::CheckButton::builder().label("Open the file when done").active(todo.open_file).margin_top(6).build();
    let power = frame::choice(&["Do nothing", "Sleep", "Shut down"], todo.power as u32);
    power.set_size_request(160, frame::FIELD);
    let row = gtk::Box::builder().spacing(5).margin_top(18).build();
    row.append(&gtk::Label::builder().label("When done, the computer should").width_request(210).xalign(0.0).build());
    row.append(&power);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&dialog);
    body.append(&open);
    body.append(&row);
    let text = note("Sleep and shut down wait 30 seconds first, so you can still cancel.");
    text.set_margin_top(16);
    body.append(&text);

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
