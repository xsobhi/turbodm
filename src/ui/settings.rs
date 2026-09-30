//! Preferences window; changes are applied and saved when it closes.

use super::Ctx;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;
use turbodm::config::MAX_CONNECTIONS;

fn spin(min: f64, max: f64, value: f64) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(min, max, 1.0);
    spin.set_value(value);
    spin
}

fn switch(active: bool) -> gtk::Switch {
    gtk::Switch::builder().active(active).halign(gtk::Align::Start).build()
}

pub fn open(ctx: &Rc<Ctx>) {
    let s = ctx.manager.settings();
    let folder = Rc::new(RefCell::new(s.download_dir.clone()));
    let folder_btn = gtk::Button::with_label(&s.download_dir.display().to_string());
    let categories = switch(s.use_categories);
    let connections = spin(1.0, MAX_CONNECTIONS as f64, s.connections as f64);
    let parallel = spin(1.0, 16.0, s.max_parallel as f64);
    let limit = spin(0.0, 10_000_000.0, s.speed_limit_kib as f64);
    let retries = spin(0.0, 100.0, s.retries as f64);
    let timeout = spin(5.0, 300.0, s.timeout_secs as f64);
    let confirm = switch(s.show_add_dialog);
    let progress = switch(s.show_progress_window);
    let notify = switch(s.notify_complete);
    let clipboard = switch(s.clipboard_monitor);
    let resume = switch(s.auto_resume);

    let grid = gtk::Grid::builder().row_spacing(10).column_spacing(16)
        .margin_top(18).margin_bottom(18).margin_start(18).margin_end(18).build();
    let rows: [(&str, &gtk::Widget); 12] = [
        ("Download folder", folder_btn.upcast_ref()),
        ("Sort into category folders", categories.upcast_ref()),
        ("Connections per download (servers may limit this)", connections.upcast_ref()),
        ("Downloads at the same time", parallel.upcast_ref()),
        ("Speed limit, KiB/s (0 = unlimited)", limit.upcast_ref()),
        ("Retries per connection", retries.upcast_ref()),
        ("Reconnect after no data for (seconds)", timeout.upcast_ref()),
        ("Confirm downloads from the browser", confirm.upcast_ref()),
        ("Open a progress window per download", progress.upcast_ref()),
        ("Notify when downloads finish", notify.upcast_ref()),
        ("Catch download links copied to the clipboard", clipboard.upcast_ref()),
        ("Resume unfinished downloads at start", resume.upcast_ref()),
    ];
    for (y, (label, widget)) in rows.iter().enumerate() {
        grid.attach(&gtk::Label::builder().label(*label).xalign(0.0).hexpand(true).build(), 0, y as i32, 1, 1);
        grid.attach(*widget, 1, y as i32, 1, 1);
    }
    let window = gtk::Window::builder().title("Preferences").modal(true)
        .transient_for(&ctx.win.window).child(&grid).build();

    let (f, w) = (folder.clone(), window.clone());
    folder_btn.connect_clicked(move |btn| {
        let dialog = gtk::FileDialog::builder().title("Download folder").modal(true)
            .initial_folder(&gio::File::for_path(&*f.borrow())).build();
        let (f, btn) = (f.clone(), btn.clone());
        dialog.select_folder(Some(&w), gio::Cancellable::NONE, move |result| {
            if let Some(path) = result.ok().and_then(|file| file.path()) {
                btn.set_label(&path.display().to_string());
                *f.borrow_mut() = path;
            }
        });
    });
    let c = ctx.clone();
    window.connect_close_request(move |_| {
        let mut s = c.manager.settings();
        s.download_dir = folder.borrow().clone();
        s.use_categories = categories.is_active();
        s.connections = connections.value() as usize;
        s.max_parallel = parallel.value() as usize;
        s.speed_limit_kib = limit.value() as u64;
        s.retries = retries.value() as u32;
        s.timeout_secs = timeout.value() as u64;
        s.show_add_dialog = confirm.is_active();
        s.show_progress_window = progress.is_active();
        s.notify_complete = notify.is_active();
        s.clipboard_monitor = clipboard.is_active();
        s.auto_resume = resume.is_active();
        c.manager.update_settings(s.clamp());
        glib::Propagation::Proceed
    });
    window.present();
}
