//! Options, as on Windows (src/win/settings.rs) and IDM's: tabs (General, Save to, Connection,
//! Scheduler, Look), each a few short groups with a heading, what each setting is on the left and
//! its switch, number or folder on the right; OK and Cancel at the bottom.

use super::looks::LookPage;
use super::scheduler::SchedulerPage;
use super::{autostart, frame, Ctx};
use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::config::MAX_CONNECTIONS;

/// The window's width; around the pages.
const WIDTH: i32 = 600;
const PAD: i32 = 22;

/// A page's line: a group's heading, a setting (its name and its control, `width` wide), a
/// name with nothing beside it, or controls side by side across the page.
pub enum Line {
    Heading(&'static str),
    Setting(&'static str, gtk::Widget, i32),
    Name(&'static str),
    Row(Vec<gtk::Widget>),
}

/// A page of lines, on the content's colour.
fn page(lines: Vec<Line>) -> gtk::Box {
    let page = gtk::Box::builder().orientation(gtk::Orientation::Vertical).css_classes(["page"])
        .margin_top(0).build();
    let inner = gtk::Box::builder().orientation(gtk::Orientation::Vertical).margin_top(PAD - 6)
        .margin_bottom(PAD - 10).margin_start(PAD).margin_end(PAD).build();
    for (i, line) in lines.into_iter().enumerate() {
        match line {
            Line::Heading(name) => {
                let heading = gtk::Label::builder().label(name).xalign(0.0).css_classes(["heading"])
                    .margin_top(if i == 0 { 0 } else { 12 }).margin_bottom(10).build();
                inner.append(&heading);
            }
            Line::Setting(name, control, width) => {
                let row = gtk::Box::builder().height_request(frame::FIELD).margin_bottom(8).build();
                let label = gtk::Label::builder().label(name).xalign(0.0).hexpand(true).margin_start(12)
                    .max_width_chars(10).ellipsize(gtk::pango::EllipsizeMode::End).tooltip_text(name).build();
                // (a switch is its own size, as GTK draws it; the rest are as wide as said)
                if !control.is::<gtk::Switch>() {
                    control.set_size_request(width, frame::FIELD);
                }
                control.set_halign(gtk::Align::End);
                control.set_valign(gtk::Align::Center);
                row.append(&label);
                row.append(&control);
                inner.append(&row);
            }
            Line::Name(name) => {
                inner.append(&gtk::Label::builder().label(name).xalign(0.0).margin_start(12).margin_bottom(8).build());
            }
            Line::Row(controls) => {
                // side by side across the page, each as wide as it needs and the room shared
                let row = gtk::Box::builder().margin_start(12).margin_bottom(10).build();
                for control in controls {
                    control.set_hexpand(true);
                    control.set_halign(gtk::Align::Start);
                    row.append(&control);
                }
                inner.append(&row);
            }
        }
    }
    page.append(&inner);
    page
}

fn switch(on: bool) -> gtk::Widget {
    let switch = gtk::Switch::builder().active(on).build();
    switch.upcast()
}

fn is_on(widget: &gtk::Widget) -> bool {
    widget.downcast_ref::<gtk::Switch>().is_some_and(|s| s.is_active())
}

fn spin(min: f64, max: f64, value: f64) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(min, max, 1.0);
    spin.set_value(value);
    spin
}

/// How many, picked from a list (easier than typing), with the number set before kept.
fn pick(values: &[usize], now: usize) -> (gtk::DropDown, Rc<Vec<usize>>) {
    let mut values: Vec<usize> = values.to_vec();
    if !values.contains(&now) {
        values.push(now);
        values.sort_unstable();
    }
    let names: Vec<String> = values.iter().map(|v| v.to_string()).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let choice = frame::choice(&names, values.iter().position(|v| *v == now).unwrap_or(0) as u32);
    (choice, Rc::new(values))
}

pub fn open(ctx: &Rc<Ctx>) {
    let s = ctx.manager.settings();
    let d = frame::dialog_with("Options", Some(ctx.win.window.upcast_ref()), WIDTH, ":close", (0, 0));
    d.window.set_modal(true);
    d.body.set_margin_bottom(0);

    // Save to: the folder, category folders, the folders remembered for kinds of files
    let folder = Rc::new(RefCell::new(s.download_dir.clone()));
    let folder_button = gtk::Button::with_label(&s.download_dir.display().to_string());
    if let Some(label) = folder_button.child().and_downcast::<gtk::Label>() {
        label.set_ellipsize(gtk::pango::EllipsizeMode::Start); // (its end, the folder's name, shows)
        label.set_max_width_chars(10);
    }
    let (f, w) = (folder.clone(), d.window.clone());
    folder_button.connect_clicked(move |button| {
        let dialog = gtk::FileDialog::builder().title("Download folder").modal(true)
            .initial_folder(&gio::File::for_path(&*f.borrow())).build();
        let (f, button) = (f.clone(), button.clone());
        dialog.select_folder(Some(&w), gio::Cancellable::NONE, move |result| {
            if let Some(path) = result.ok().and_then(|file| file.path()) {
                if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                    label.set_text(&path.display().to_string());
                }
                *f.borrow_mut() = path;
            }
        });
    });
    let forget_all = Rc::new(Cell::new(false));
    let remembered = s.folders.len();
    let forget = gtk::Button::builder().label(format!("Forget ({remembered})")).sensitive(remembered > 0).build();
    let fa = forget_all.clone();
    forget.connect_clicked(move |b| {
        fa.set(true);
        b.set_label("Forgotten");
        b.set_sensitive(false);
    });
    let categories = switch(s.use_categories);

    // Connection
    let (connections, connection_values) = pick(&[1, 2, 4, 8, 16, 24, MAX_CONNECTIONS], s.connections);
    let (parallel, parallel_values) = pick(&[1, 2, 3, 4, 5, 6, 8, 10, 12, 16], s.max_parallel);
    let speed = spin(0.0, 10_000_000.0, s.speed_limit_kib as f64);
    let retries = spin(0.0, 100.0, s.retries as f64);
    let timeout = spin(5.0, 300.0, s.timeout_secs as f64);

    // General
    let (confirm, predownload) = (switch(s.show_add_dialog), switch(s.predownload));
    let (progress, complete, notify) = (switch(s.show_progress_window), switch(s.show_complete_dialog),
                                        switch(s.notify_complete));
    let (clipboard, resume, updates) = (switch(s.clipboard_monitor), switch(s.auto_resume), switch(s.check_updates));
    let startup = switch(s.start_with_system);
    let seed = switch(s.seed);
    let add_browser = gtk::Button::with_label("Add to your browser\u{2026}");
    let c = ctx.clone();
    add_browser.connect_clicked(move |_| super::browsers::open(&c));
    // (the helpers videos need, yt-dlp and FFmpeg, are the system's here)
    let ready = turbodm::engine::video::tools::yt_dlp_now().is_some() && turbodm::engine::video::tools::ffmpeg().is_some();
    let video_helpers = gtk::Button::builder().label(if ready { "Installed" } else { "Not installed" })
        .sensitive(false).tooltip_text("yt-dlp and FFmpeg, from your system's packages").build();
    let youtube = gtk::Button::with_label("Show how\u{2026}");
    let c = ctx.clone();
    youtube.connect_clicked(move |_| super::youtube::open(&c));

    // Look: the theme, the user's own colours; Scheduler
    let look = Rc::new(LookPage::new(&d.window, &s));
    let scheduler = SchedulerPage::new(&s.schedule);

    use Line::{Heading, Setting};
    let w = |widget: &gtk::Widget| widget.clone();
    let pages = [
        ("General", vec![Heading("Downloads from the browser"),
             Setting("Ask where to save them (Download file info)", w(&confirm), 44),
             Setting("Start downloading while it's open", w(&predownload), 44),
             Setting("Browser integration", add_browser.upcast(), 200),
             Heading("Videos"),
             Setting("Helpers for videos (yt-dlp and FFmpeg)", video_helpers.upcast(), 130),
             Setting("YouTube in Chrome, Edge and Brave", youtube.upcast(), 130),
             Heading("Windows and notices"),
             Setting("Open a progress window when a download starts", w(&progress), 44),
             Setting("Show the Download complete window", w(&complete), 44),
             Setting("Notify when downloads finish", w(&notify), 44),
             Heading("At start, and meanwhile"),
             Setting("Resume unfinished downloads at start", w(&resume), 44),
             Setting("Catch download links copied to the clipboard", w(&clipboard), 44),
             Setting("Check for updates", w(&updates), 44),
             Setting("Start when you log in, in the notification area", w(&startup), 44)]),
        ("Save to", vec![Heading("Where downloads go"),
             Setting("Download folder", folder_button.upcast(), WIDTH - 2 * PAD - 346),
             Setting("Sort into category folders (Video, Music\u{2026})", w(&categories), 44),
             Setting("Folders remembered for kinds of files", forget.upcast(), 130)]),
        ("Connection", vec![Heading("Each download"),
             Setting("Connections at once (servers may limit them)", connections.clone().upcast(), 150),
             Setting("Retries per connection", retries.clone().upcast(), 150),
             Setting("Reconnect after no data for (seconds)", timeout.clone().upcast(), 150),
             Heading("All downloads"),
             Setting("Downloads at the same time", parallel.clone().upcast(), 150),
             Setting("Speed limit, KB/s (0 = no limit)", speed.clone().upcast(), 150),
             Heading("Torrents"),
             Setting("Keep sharing finished torrents", w(&seed), 44)]),
        ("Scheduler", scheduler.lines()),
        ("Look", look.lines()),
    ];
    let tabs = gtk::Notebook::builder().show_border(false).build();
    for (name, lines) in pages {
        // switches sit on the right, as their row's control, not stretched
        tabs.append_page(&page(lines), Some(&gtk::Label::new(Some(name))));
    }
    d.body.append(&tabs);
    let ok = frame::default_button(&d.window, "OK");
    let cancel = frame::button("Cancel");
    let footer = gtk::Box::builder().spacing(8).halign(gtk::Align::End).margin_top(14).margin_bottom(14)
        .margin_end(PAD).build();
    footer.append(&cancel);
    footer.append(&ok);
    d.body.append(&gtk::Separator::builder().css_classes(["rule"]).build());
    d.body.append(&footer);

    let (c, win, l) = (ctx.clone(), d.window.clone(), look.clone());
    let saved = Rc::new(Cell::new(false));
    let sv = saved.clone();
    ok.connect_clicked(move |_| {
        let mut s = c.manager.settings();
        (s.theme, s.theme_colors) = l.chosen();
        s.download_dir = folder.borrow().clone();
        if forget_all.get() {
            s.folders.clear();
        }
        s.use_categories = is_on(&categories);
        s.connections = connection_values[connections.selected() as usize];
        s.max_parallel = parallel_values[parallel.selected() as usize];
        speed.update();
        retries.update();
        timeout.update();
        (s.speed_limit_kib, s.retries, s.timeout_secs) = (speed.value() as u64, retries.value() as u32, timeout.value() as u64);
        (s.show_add_dialog, s.predownload) = (is_on(&confirm), is_on(&predownload));
        (s.show_progress_window, s.show_complete_dialog, s.notify_complete) =
            (is_on(&progress), is_on(&complete), is_on(&notify));
        (s.clipboard_monitor, s.auto_resume, s.check_updates) = (is_on(&clipboard), is_on(&resume), is_on(&updates));
        s.start_with_system = is_on(&startup);
        autostart::sync(s.start_with_system);
        s.seed = is_on(&seed);
        s.schedule = scheduler.chosen();
        c.manager.update_settings(s.clamp());
        super::actions::update_enabled(&c);
        sv.set(true);
        win.close();
    });
    let w = d.window.clone();
    cancel.connect_clicked(move |_| w.close());
    // Cancel, Escape, the close button: the look there was, back
    d.window.connect_close_request(move |_| {
        if !saved.get() {
            look.revert();
        }
        gtk::glib::Propagation::Proceed
    });
    d.window.present();
}
