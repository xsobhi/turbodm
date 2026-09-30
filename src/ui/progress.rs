//! Per-download progress window (like IDM's): details, segment map, pause/resume.

use super::segment_bar::SegmentBar;
use super::{item::status_text, Ctx};
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::engine::{Manager, Status};
use turbodm::util::{human_eta, human_size, human_speed};

pub struct ProgressWindow {
    id: String,
    window: gtk::Window,
    rows: Vec<gtk::Label>,
    overall: gtk::ProgressBar,
    bar: SegmentBar,
    toggle: gtk::Button,
}

const FIELDS: [&str; 8] = ["File", "Address", "Status", "Size", "Downloaded", "Speed", "Time left",
                           "Resume support"];

pub fn open(ctx: &Rc<Ctx>, id: &str) {
    if let Some(existing) = ctx.progress.borrow().get(id) {
        existing.window.present();
        return;
    }
    let grid = gtk::Grid::builder().row_spacing(6).column_spacing(14).build();
    let rows: Vec<gtk::Label> = FIELDS
        .iter()
        .enumerate()
        .map(|(y, name)| {
            grid.attach(&gtk::Label::builder().label(*name).xalign(1.0).css_classes(["dim-label"]).build(),
                        0, y as i32, 1, 1);
            let value = gtk::Label::builder().xalign(0.0).selectable(true).hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::Middle).build();
            grid.attach(&value, 1, y as i32, 1, 1);
            value
        })
        .collect();
    let overall = gtk::ProgressBar::builder().show_text(true).build();
    let bar = SegmentBar::new();
    let toggle = gtk::Button::with_label("Pause");
    let cancel = gtk::Button::with_label("Cancel");
    let hide = gtk::Button::with_label("Hide");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    for b in [&cancel, &hide, &toggle] {
        buttons.append(b);
    }
    let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12)
        .margin_top(16).margin_bottom(16).margin_start(16).margin_end(16).build();
    body.append(&grid);
    body.append(&overall);
    body.append(&gtk::Label::builder().label("Connections").xalign(0.0).css_classes(["dim-label"]).build());
    body.append(&bar.area);
    body.append(&buttons);
    let window = gtk::Window::builder().title("Download progress").default_width(620).child(&body).build();

    let (c, i) = (ctx.clone(), id.to_string());
    toggle.connect_clicked(move |_| match c.manager.get(&i).map(|s| s.status) {
        Some(s) if s.is_active() || s == Status::Queued => c.manager.pause(&i),
        Some(_) => c.manager.resume(&i),
        None => {}
    });
    let (c, i, w) = (ctx.clone(), id.to_string(), window.clone());
    cancel.connect_clicked(move |_| {
        c.manager.pause(&i);
        w.close();
    });
    let w = window.clone();
    hide.connect_clicked(move |_| w.close());
    let (c, i) = (Rc::downgrade(ctx), id.to_string());
    window.connect_close_request(move |_| {
        if let Some(ctx) = c.upgrade() {
            ctx.progress.borrow_mut().remove(&i);
        }
        gtk::glib::Propagation::Proceed
    });
    let pw = ProgressWindow { id: id.to_string(), window, rows, overall, bar, toggle };
    pw.refresh(&ctx.manager);
    pw.window.present();
    ctx.progress.borrow_mut().insert(id.to_string(), pw);
}

impl ProgressWindow {
    pub fn refresh(&self, manager: &Manager) {
        let Some(s) = manager.get(&self.id) else {
            self.window.close();
            return;
        };
        let values = [
            s.filename.clone(),
            s.url.clone(),
            status_text(&s),
            human_size(s.size),
            human_size(Some(s.downloaded)),
            human_speed(s.speed),
            human_eta(s.eta),
            if s.resumable { format!("Yes · {} of {} connections active", s.active_connections, s.connections) }
            else { "No (single connection)".into() },
        ];
        for (label, value) in self.rows.iter().zip(values) {
            if label.text() != value {
                label.set_text(&value);
            }
        }
        self.window.set_title(Some(&match s.progress {
            Some(p) => format!("{:.0}% {}", p * 100.0, s.filename),
            None => s.filename.clone(),
        }));
        self.overall.set_fraction(s.progress.unwrap_or(0.0));
        self.overall.set_text(Some(&status_text(&s)));
        self.bar.set(s.size, s.segments);
        let label = if s.status.is_active() || s.status == Status::Queued { "Pause" }
                    else if s.status == Status::Completed { "Done" } else { "Resume" };
        self.toggle.set_label(label);
        self.toggle.set_sensitive(s.status != Status::Completed);
    }
}
