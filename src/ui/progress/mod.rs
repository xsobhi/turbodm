//! A download's own window, as on Windows (src/win/progress) and like IDM's: opens when a
//! download starts or resumes. Tabs for the download status (with each connection's part), its
//! speed limiter and what to do when done; Hide puts it under the downloads' icon in the
//! notification area (queue.rs).

mod details;
mod options;
mod segment_bar;

use super::{frame, Ctx};
use details::Details;
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::engine::{Manager, Status};
use turbodm::text::{eta_text, joining_text, peers_text, rate_text, status_line, status_text};
use turbodm::util::human_size;

const WIDTH: i32 = 640;
const PAD: i32 = 16;

pub struct ProgressWindow {
    id: String,
    pub window: gtk::Window,
    title: gtk::Label,
    rows: Vec<gtk::Label>,
    overall: gtk::ProgressBar,
    details: Details,
    toggle: gtk::Button,
}

/// The status tab's rows: a file's, a torrent's.
const FIELDS: [&str; 8] = ["Address", "Saved to", "Status", "File size", "Downloaded", "Transfer rate",
                           "Time left", "Resume capability"];
const TORRENT_FIELDS: [&str; 8] = ["Address", "Saved to", "Status", "Size", "Downloaded", "Transfer rate",
                                   "Time left", "Peers"];

fn status_rows(torrent: bool) -> (gtk::Grid, Vec<gtk::Label>) {
    let grid = gtk::Grid::builder().column_spacing(14).build();
    let names = if torrent { TORRENT_FIELDS } else { FIELDS };
    let rows = names.iter().enumerate().map(|(y, name)| {
        let label = gtk::Label::builder().label(*name).xalign(1.0).css_classes(["dim-label"]).width_request(128)
            .height_request(24).build();
        grid.attach(&label, 0, y as i32, 1, 1);
        let ellipsize = if y == 1 { gtk::pango::EllipsizeMode::Middle } else { gtk::pango::EllipsizeMode::End };
        let value = gtk::Label::builder().xalign(0.0).hexpand(true).ellipsize(ellipsize).max_width_chars(10).build();
        grid.attach(&value, 1, y as i32, 1, 1);
        value
    }).collect();
    (grid, rows)
}

/// A tab's page: on the content's colour, 16 from its edges.
fn page(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(0).css_classes(["page"]).build();
    child.set_margin_top(PAD);
    child.set_margin_bottom(PAD);
    child.set_margin_start(PAD);
    child.set_margin_end(PAD);
    b.append(child);
    b
}

pub fn open(ctx: &Rc<Ctx>, id: &str) {
    if let Some(existing) = ctx.progress.borrow().get(id) {
        existing.window.present();
        super::queue::update(ctx);
        return;
    }
    let Some(snap) = ctx.manager.get(id) else { return };
    ctx.on_done.borrow_mut().entry(id.to_string()).or_default(); // being watched: say when it's done
    let torrent = snap.torrent.is_some();
    let (grid, rows) = status_rows(torrent);
    let overall = gtk::ProgressBar::builder().show_text(true).margin_top(8).height_request(26).build();
    let details = Details::new(torrent);
    let status_page = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    status_page.append(&grid);
    status_page.append(&overall);
    status_page.append(&details.widget);
    let tabs = gtk::Notebook::builder().show_border(false).build();
    tabs.append_page(&page(&status_page), Some(&gtk::Label::new(Some("Download status"))));
    tabs.append_page(&page(&options::speed_limiter(ctx, &snap)), Some(&gtk::Label::new(Some("Speed limiter"))));
    tabs.append_page(&page(&options::on_completion(ctx, id)), Some(&gtk::Label::new(Some("Options on completion"))));

    let d = frame::dialog_with(&snap.filename, None, WIDTH, ":minimize,close", (0, 0));
    d.body.set_margin_bottom(0);
    let window = d.window.clone();
    window.set_resizable(false);
    let more = gtk::Button::builder().label("_Hide details").use_underline(true).width_request(130)
        .height_request(frame::FIELD + 2).build(); // Alt+H
    let hide = frame::button("H_ide");
    hide.set_use_underline(true);
    let toggle = frame::default_button(&window, "Pause");
    let cancel = frame::button("Cancel");
    let buttons = gtk::Box::builder().spacing(8).margin_start(PAD).margin_end(PAD).margin_top(14).margin_bottom(14).build();
    buttons.append(&more);
    buttons.append(&gtk::Box::builder().hexpand(true).build());
    buttons.append(&hide);
    buttons.append(&toggle);
    buttons.append(&cancel);
    d.body.append(&tabs);
    d.body.append(&gtk::Separator::builder().css_classes(["rule"]).build());
    d.body.append(&buttons);

    let m = more.clone();
    tabs.connect_switch_page(move |_, _, page| m.set_visible(page == 0)); // details: status tab only
    let (det, w) = (details.widget.clone(), window.clone());
    more.connect_clicked(move |b| {
        let show = !det.is_visible();
        det.set_visible(show);
        b.set_label(if show { "_Hide details" } else { "_Show details" });
        super::center::fit_height(&w); // no empty space where the connections were, room when they're back
    });
    let (c, w) = (Rc::downgrade(ctx), window.clone());
    hide.connect_clicked(move |_| {
        w.set_visible(false);
        if let Some(c) = c.upgrade() {
            super::queue::update(&c);
        }
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
            super::queue::update(&ctx);
        }
        gtk::glib::Propagation::Proceed
    });
    // minimized: hidden under the downloads' icon instead, as Hide does (as on Windows)
    let c = Rc::downgrade(ctx);
    window.connect_realize(move |w| {
        let Some(surface) = w.surface() else { return };
        let (c, w) = (c.clone(), w.downgrade());
        surface.connect_notify_local(Some("state"), move |surface, _| {
            let minimized = surface.downcast_ref::<gtk::gdk::Toplevel>()
                .is_some_and(|t| t.state().contains(gtk::gdk::ToplevelState::MINIMIZED));
            if let (true, Some(c), Some(w)) = (minimized, c.upgrade(), w.upgrade())
                && c.tray.get() {
                    w.set_visible(false);
                    super::queue::update(&c);
                }
        });
    });
    let pw = ProgressWindow { id: id.to_string(), window, title: d.title, rows, overall, details, toggle };
    pw.refresh(&ctx.manager);
    pw.window.present();
    ctx.progress.borrow_mut().insert(id.to_string(), pw);
    super::queue::update(ctx);
}

/// Close a download's window (it finished or was removed). Safe to call from anywhere.
pub fn close(ctx: &Ctx, id: &str) -> bool {
    let window = ctx.progress.borrow().get(id).map(|p| p.window.clone());
    window.map(|w| w.close()).is_some()
}

impl ProgressWindow {
    /// Update from the engine; false when the download no longer exists.
    pub fn refresh(&self, manager: &Manager) -> bool {
        let Some(s) = manager.get(&self.id) else { return false };
        let downloaded = match s.progress {
            Some(p) => format!("{} ({:.2}%)", human_size(Some(s.downloaded)), p * 100.0),
            None => human_size(Some(s.downloaded)),
        };
        let resume = match &s.torrent {
            Some(t) if t.uploaded > 0 => format!("{}   ·   shared {}", peers_text(&s), human_size(Some(t.uploaded))),
            Some(_) => peers_text(&s),
            None if s.resumable => "Yes".into(),
            None => "No (single connection, can't pause and resume)".into(),
        };
        let values = [
            s.url.clone(),
            s.path.display().to_string(),
            status_line(&s),
            human_size(s.size),
            downloaded,
            rate_text(&s),
            eta_text(&s),
            resume,
        ];
        for (label, value) in self.rows.iter().zip(values) {
            if label.text() != value {
                label.set_text(&value);
            }
        }
        // the status in the accent colour while it works (IDM's blue "Receiving data…"), errors red
        let class = match s.status {
            Status::Error => Some("danger"),
            status if status.is_active() => Some("accent"),
            _ => None,
        };
        for c in ["danger", "accent"] {
            if Some(c) != class && self.rows[2].has_css_class(c) {
                self.rows[2].remove_css_class(c);
            }
        }
        if let Some(c) = class {
            self.rows[2].add_css_class(c);
        }
        // its parts all in and being put together: the same window, saying so, its bar how far
        // that is; then it closes and "Download complete" opens
        let title = match (s.joining, s.progress) {
            (Some(_), _) => format!("{} \u{2013} {}", joining_text(&s).trim_end_matches('…'), s.filename),
            (None, Some(p)) => format!("{:.0}% {}", p * 100.0, s.filename),
            (None, None) => s.filename.clone(),
        };
        if self.window.title().as_deref() != Some(title.as_str()) {
            self.window.set_title(Some(&title));
            self.title.set_text(&title);
        }
        let fraction = s.joining.or(s.progress).unwrap_or(0.0);
        if self.overall.fraction() != fraction {
            self.overall.set_fraction(fraction);
        }
        let text = status_text(&s);
        if self.overall.text().as_deref() != Some(text.as_str()) {
            self.overall.set_text(Some(&text));
        }
        self.details.update(&s);
        let running = s.status.is_active() || s.status == Status::Queued;
        let label = if running { "Pause" } else { "Resume" };
        if self.toggle.label().as_deref() != Some(label) {
            self.toggle.set_label(label);
        }
        self.toggle.set_sensitive(s.status != Status::Completed && s.joining.is_none()); // (nothing left to pause)
        true
    }
}
