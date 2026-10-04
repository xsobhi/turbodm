//! Per-download window, like IDM's: opens when a download starts or resumes. Tabs for the
//! download status (with a per-connection view), its speed limiter and what to do when done.

mod details;
mod options;
mod segment_bar;

use super::{center, item::status_text, Ctx};
use details::Details;
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::engine::{Manager, Snapshot, Status};
use turbodm::util::{human_eta, human_size, human_speed};

pub struct ProgressWindow {
    id: String,
    pub window: gtk::Window,
    rows: Vec<gtk::Label>,
    overall: gtk::ProgressBar,
    details: Details,
    toggle: gtk::Button,
}

const FIELDS: [&str; 8] = ["Address", "Saved to", "Status", "File size", "Downloaded", "Transfer rate",
                           "Time left", "Resume capability"];

fn status_tab() -> (gtk::Grid, Vec<gtk::Label>) {
    let grid = gtk::Grid::builder().row_spacing(6).column_spacing(14).build();
    let rows = FIELDS
        .iter()
        .enumerate()
        .map(|(y, name)| {
            grid.attach(&gtk::Label::builder().label(*name).xalign(1.0).css_classes(["dim-label"]).build(),
                        0, y as i32, 1, 1);
            let value = gtk::Label::builder().xalign(0.0).hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::Middle).build();
            grid.attach(&value, 1, y as i32, 1, 1);
            value
        })
        .collect();
    (grid, rows)
}

fn page(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(10)
        .margin_top(14).margin_bottom(14).margin_start(14).margin_end(14).build();
    b.append(child);
    b
}

pub fn open(ctx: &Rc<Ctx>, id: &str) {
    if let Some(existing) = ctx.progress.borrow().get(id) {
        existing.window.present();
        return;
    }
    let Some(snap) = ctx.manager.get(id) else { return };
    ctx.on_done.borrow_mut().entry(id.to_string()).or_default(); // being watched: say when it's done
    let (grid, rows) = status_tab();
    let overall = gtk::ProgressBar::builder().show_text(true).build();
    let details = Details::new(snap.connections);
    let status_page = page(&grid);
    status_page.append(&overall);
    status_page.append(&details.widget);
    let notebook = gtk::Notebook::new();
    notebook.append_page(&status_page, Some(&gtk::Label::new(Some("Download status"))));
    notebook.append_page(&page(&options::speed_limiter(ctx, &snap)), Some(&gtk::Label::new(Some("Speed limiter"))));
    notebook.append_page(&page(&options::on_completion(ctx, id)), Some(&gtk::Label::new(Some("Options on completion"))));

    let more = gtk::Button::with_label("Hide details");
    let toggle = gtk::Button::builder().label("Pause").width_request(96).build();
    let cancel = gtk::Button::builder().label("Cancel").width_request(96).build();
    let buttons = gtk::Box::builder().spacing(8).margin_start(14).margin_end(14).margin_bottom(14).build();
    buttons.append(&more);
    buttons.append(&gtk::Box::builder().hexpand(true).build());
    buttons.append(&toggle);
    buttons.append(&cancel);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&notebook);
    body.append(&buttons);
    let window = gtk::Window::builder().title(&snap.filename).default_width(640).child(&body).build();
    center::on_screen(&window, true);

    let m = more.clone();
    notebook.connect_switch_page(move |_, _, page| m.set_visible(page == 0)); // details: status tab only
    let (d, w) = (details.widget.clone(), window.clone());
    more.connect_clicked(move |b| {
        let show = !d.is_visible();
        d.set_visible(show);
        b.set_label(if show { "Hide details" } else { "Show details" });
        center::fit_height(&w); // no empty space where the connections were, room when they're back
    });
    let (c, i) = (ctx.clone(), id.to_string());
    toggle.connect_clicked(move |_| match c.manager.get(&i).map(|s| s.status) {
        Some(s) if s.is_active() || s == Status::Queued => c.manager.pause(&i),
        Some(_) => c.manager.resume(&i),
        None => {}
    });
    let (c, i, w) = (ctx.clone(), id.to_string(), window.clone());
    cancel.connect_clicked(move |_| {
        c.manager.pause(&i); // IDM's Cancel: stop, keep what's downloaded
        c.on_done.borrow_mut().remove(&i);
        w.close();
    });
    let (c, i) = (Rc::downgrade(ctx), id.to_string());
    window.connect_close_request(move |_| {
        if let Some(ctx) = c.upgrade() {
            ctx.progress.borrow_mut().remove(&i);
        }
        gtk::glib::Propagation::Proceed
    });
    let pw = ProgressWindow { id: id.to_string(), window, rows, overall, details, toggle };
    pw.refresh(&ctx.manager);
    pw.window.present();
    ctx.progress.borrow_mut().insert(id.to_string(), pw);
}

/// Close a download's window (it finished or was removed). Safe to call from anywhere.
pub fn close(ctx: &Ctx, id: &str) -> bool {
    let window = ctx.progress.borrow().get(id).map(|p| p.window.clone());
    window.map(|w| w.close()).is_some()
}

fn status_line(s: &Snapshot) -> String {
    match (s.status, &s.error) {
        (Status::Error, Some(e)) => format!("Error: {e}"),
        (Status::Downloading, _) => "Receiving data…".into(),
        (status, _) => status.label().into(),
    }
}

impl ProgressWindow {
    /// Update from the engine; false when the download no longer exists.
    pub fn refresh(&self, manager: &Manager) -> bool {
        let Some(s) = manager.get(&self.id) else { return false };
        let downloaded = match s.progress {
            Some(p) => format!("{} ({:.2}%)", human_size(Some(s.downloaded)), p * 100.0),
            None => human_size(Some(s.downloaded)),
        };
        let values = [
            s.url.clone(),
            s.path.display().to_string(),
            status_line(&s),
            human_size(s.size),
            downloaded,
            human_speed(s.speed),
            human_eta(s.eta),
            if s.resumable { "Yes".into() } else { "No (single connection, can't pause and resume)".into() },
        ];
        for (label, value) in self.rows.iter().zip(values) {
            if label.text() != value {
                label.set_text(&value);
            }
        }
        let title = match s.progress {
            Some(p) => format!("{:.0}% {}", p * 100.0, s.filename),
            None => s.filename.clone(),
        };
        if self.window.title().as_deref() != Some(title.as_str()) {
            self.window.set_title(Some(&title));
        }
        let fraction = s.progress.unwrap_or(0.0);
        if self.overall.fraction() != fraction {
            self.overall.set_fraction(fraction);
        }
        self.overall.set_text(Some(&status_text(&s)));
        self.details.update(&s);
        let running = s.status.is_active() || s.status == Status::Queued;
        self.toggle.set_label(if running { "Pause" } else { "Resume" });
        self.toggle.set_sensitive(s.status != Status::Completed);
        true
    }
}
