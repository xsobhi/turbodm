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
        let area = gtk::DrawingArea::builder().content_height(28).hexpand(true).build();
        let d = data.clone();
        area.set_draw_func(move |_, cr, width, height| {
            let data = d.borrow();
            let (w, h) = (width as f64, height as f64);
            cr.set_source_rgba(0.5, 0.5, 0.5, 0.22);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
            let Some(size) = data.size.filter(|&s| s > 0) else { return };
            let scale = w / size as f64;
            for seg in &data.segments {
                let x = seg.start as f64 * scale;
                let done = (seg.done as f64 * scale).max(if seg.done > 0 { 1.0 } else { 0.0 });
                cr.set_source_rgb(0.21, 0.52, 0.89); // downloaded: blue
                cr.rectangle(x, 0.0, done, h);
                let _ = cr.fill();
                if seg.active && !seg.finished() {
                    cr.set_source_rgb(0.2, 0.82, 0.48); // live connection head: green
                    cr.rectangle(x + done - 2.0, 0.0, 3.0, h);
                    let _ = cr.fill();
                }
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.5); // segment boundary
                cr.rectangle(x, 0.0, 1.0, h);
                let _ = cr.fill();
            }
        });
        SegmentBar { area, data }
    }

    pub fn set(&self, size: Option<u64>, segments: Vec<Segment>) {
        *self.data.borrow_mut() = Data { size, segments };
        self.area.queue_draw();
    }
}
