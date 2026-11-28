//! IDM-style bar showing every connection's segment and how much of it is done.

use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use turbodm::engine::segments::Segment;

#[derive(Default)]
struct Data {
    size: Option<u64>,
    segments: Vec<Segment>,
}

pub struct SegmentBar {
    pub area: gtk::DrawingArea,
    data: Rc<RefCell<Data>>,
}

impl SegmentBar {
    pub fn new() -> Self {
        let data = Rc::new(RefCell::new(Data::default()));
        let area = gtk::DrawingArea::builder().content_height(14).hexpand(true).build();
        let d = data.clone();
        area.set_draw_func(move |_, cr, width, height| {
            let data = d.borrow();
            let (w, h) = (width as f64, height as f64);
            let p = crate::ui::style::current().palette;
            let rgb = crate::ui::style::rgb;
            let (r, g, b) = rgb(p.track);
            cr.set_source_rgb(r, g, b);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
            let Some(size) = data.size.filter(|&s| s > 0) else { return };
            let scale = w / size as f64;
            for seg in &data.segments {
                let x = seg.start as f64 * scale;
                let done = (seg.got() as f64 * scale).max(if seg.got() > 0 { 1.0 } else { 0.0 }); // what arrived, written yet or not
                let (r, g, b) = rgb(p.accent);
                cr.set_source_rgb(r, g, b); // downloaded: the accent colour
                cr.rectangle(x, 0.0, done, h);
                let _ = cr.fill();
                if seg.active && !seg.finished() {
                    let (r, g, b) = rgb(p.green);
                    cr.set_source_rgb(r, g, b); // live connection head: green
                    cr.rectangle(x + done - 2.0, 0.0, 3.0, h);
                    let _ = cr.fill();
                }
                let (r, g, b) = rgb(p.view);
                cr.set_source_rgb(r, g, b); // segment boundary
                cr.rectangle(x, 0.0, 1.0, h);
                let _ = cr.fill();
            }
        });
        SegmentBar { area, data }
    }

    pub fn set(&self, size: Option<u64>, segments: Vec<Segment>) {
        let mut data = self.data.borrow_mut();
        if data.size != size || data.segments != segments {
            *data = Data { size, segments };
            self.area.queue_draw();
        }
    }
}
