//! "Show details": the segment map and one row per connection, like IDM's connection list.

use super::segment_bar::SegmentBar;
use gtk::prelude::*;
use std::cell::RefCell;
use turbodm::engine::segments::Segment;
use turbodm::engine::{Snapshot, Status};
use turbodm::util::human_size;

pub struct Details {
    pub revealer: gtk::Revealer,
    bar: SegmentBar,
    grid: gtk::Grid,
    rows: RefCell<Vec<[gtk::Label; 4]>>,
}

fn cell(text: &str, xalign: f32, css: &[&str]) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(xalign).css_classes(css.to_vec()).build()
}

fn info(seg: &Segment, status: Status) -> &'static str {
    if seg.finished() {
        "Complete"
    } else if seg.active {
        if status == Status::Connecting { "Connecting…" } else { "Receiving data…" }
    } else if status.is_active() {
        "Waiting for a free connection"
    } else {
        status.label()
    }
}

impl Details {
    pub fn new() -> Self {
        let bar = SegmentBar::new();
        let grid = gtk::Grid::builder().row_spacing(3).column_spacing(18).margin_end(12).build();
        for (x, title) in ["N.", "Starts at", "Downloaded", "Info"].into_iter().enumerate() {
            grid.attach(&cell(title, if x == 3 { 0.0 } else { 1.0 }, &["dim-label"]), x as i32, 0, 1, 1);
        }
        let scroller = gtk::ScrolledWindow::builder().child(&grid).max_content_height(200)
            .propagate_natural_height(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
        body.append(&cell("Start positions and progress of each connection", 0.0, &["dim-label"]));
        body.append(&bar.area);
        body.append(&scroller);
        let revealer = gtk::Revealer::builder().child(&body).reveal_child(true).build();
        Details { revealer, bar, grid, rows: RefCell::new(Vec::new()) }
    }

    pub fn update(&self, s: &Snapshot) {
        self.bar.set(s.size, s.segments.clone());
        if !self.revealer.reveals_child() {
            return;
        }
        let mut rows = self.rows.borrow_mut();
        while rows.len() < s.segments.len() {
            let y = rows.len() as i32 + 1;
            let row = [cell("", 1.0, &[]), cell("", 1.0, &["numeric"]), cell("", 1.0, &["numeric"]), cell("", 0.0, &[])];
            for (x, label) in row.iter().enumerate() {
                self.grid.attach(label, x as i32, y, 1, 1);
            }
            rows.push(row);
        }
        for (i, row) in rows.iter().enumerate() {
            let seg = s.segments.get(i);
            let values = match seg {
                Some(seg) => [(i + 1).to_string(), human_size(Some(seg.start)), human_size(Some(seg.done)),
                              info(seg, s.status).to_string()],
                None => Default::default(),
            };
            for (label, value) in row.iter().zip(values) {
                if label.text() != value {
                    label.set_text(&value);
                }
                label.set_visible(seg.is_some());
            }
        }
    }
}
