//! "Show details", as on Windows: the map of every connection's part and the list of
//! connections, like IDM's. Both count what arrived, not only what's written, so they move live.
//! A torrent's peers come and go by the dozen: it shows its files instead.

use super::segment_bar::SegmentBar;
use gtk::prelude::*;
use std::cell::RefCell;
use turbodm::engine::segments::Segment;
use turbodm::engine::Snapshot;
use turbodm::text::{connection_info, part_info};
use turbodm::util::human_size;

/// The list's height: its header and 8 rows (more scroll).
const LIST: i32 = 28 + 8 * 22;

pub struct Details {
    pub widget: gtk::Box, // hidden by "Hide details"
    caption: gtk::Label,
    torrent: bool,
    bar: SegmentBar,
    grid: gtk::Grid,
    rows: RefCell<Vec<[gtk::Label; 4]>>,
}

fn cell(text: &str, xalign: f32, width: i32) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(xalign).width_request(width).height_request(22)
        .ellipsize(gtk::pango::EllipsizeMode::End).build()
}

impl Details {
    pub fn new(torrent: bool) -> Self {
        let bar = SegmentBar::new();
        let grid = gtk::Grid::builder().column_spacing(12).margin_end(12).margin_start(4).build();
        let (second, last) = if torrent { ("Size", "File") } else { ("Starts at", "Info") };
        for (x, (title, width)) in [("N.", 32), (second, 98), ("Downloaded", 98), (last, 200)].into_iter().enumerate() {
            let label = cell(title, if x == 3 { 0.0 } else { 1.0 }, width);
            label.set_height_request(28);
            label.add_css_class("dim-label");
            if x == 3 {
                label.set_hexpand(true);
            }
            grid.attach(&label, x as i32, 0, 1, 1);
        }
        let scroller = gtk::ScrolledWindow::builder().child(&grid).height_request(LIST)
            .hscrollbar_policy(gtk::PolicyType::Never).margin_top(10).css_classes(["view"]).build();
        let caption = gtk::Label::builder().xalign(0.0).css_classes(["dim-label"]).margin_bottom(6)
            .label(if torrent { "Progress of each file" } else { "Start positions and progress of each connection" })
            .build();
        let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).margin_top(16).build();
        body.append(&caption);
        body.append(&bar.area);
        body.append(&scroller);
        Details { widget: body, caption, torrent, bar, grid, rows: RefCell::new(Vec::new()) }
    }

    pub fn update(&self, s: &Snapshot) {
        // a torrent's files drawn as parts of it: where each starts and how much of it is here
        let files = s.torrent.as_ref().map(|t| t.files.clone()).unwrap_or_default();
        let segments: Vec<Segment> = match &s.torrent {
            Some(_) => files.iter().map(|f| Segment { start: f.offset, end: Some(f.offset + f.size), done: f.have,
                                                       ahead: 0, active: false, heard: None }).collect(),
            None => s.segments.clone(),
        };
        self.bar.set(s.size, segments.clone());
        let caption = match (self.torrent, s.by_ytdlp) {
            (true, _) => "Progress of each file",
            (_, true) => "The video and its sound, downloaded one after the other",
            _ => "Start positions and progress of each connection",
        };
        if self.caption.text() != caption {
            self.caption.set_text(caption);
        }
        if !self.widget.is_visible() {
            return;
        }
        let wanted: Vec<[String; 4]> = match &s.torrent {
            Some(_) => files.iter().enumerate().map(|(i, f)| {
                [(i + 1).to_string(), human_size(Some(f.size)), human_size(Some(f.have)), f.name.clone()]
            }).collect(),
            None => segments.iter().enumerate().map(|(i, seg)| {
                [(i + 1).to_string(), human_size(Some(seg.start)), human_size(Some(seg.got())),
                 if s.by_ytdlp { part_info(i, s) } else { connection_info(seg, s.status).to_string() }]
            }).collect(),
        };
        let mut rows = self.rows.borrow_mut();
        while rows.len() < wanted.len() {
            let y = rows.len() as i32 + 1;
            let row = [cell("", 1.0, 32), cell("", 1.0, 98), cell("", 1.0, 98), cell("", 0.0, 200)];
            for (x, label) in row.iter().enumerate() {
                self.grid.attach(label, x as i32, y, 1, 1);
            }
            rows.push(row);
        }
        for (i, row) in rows.iter().enumerate() {
            let values = wanted.get(i);
            for (x, label) in row.iter().enumerate() {
                let value = values.map_or("", |v| v[x].as_str());
                if label.text() != value {
                    label.set_text(value);
                }
                label.set_visible(values.is_some()); // (fewer than before: a video's next part)
            }
        }
    }
}
