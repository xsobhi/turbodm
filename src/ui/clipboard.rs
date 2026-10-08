//! Clipboard monitoring (opt-in): offer to download links to files you copy.

use super::Ctx;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;
use turbodm::engine::AddRequest;
use turbodm::text::looks_like_file_link;

pub fn start(ctx: &Rc<Ctx>) {
    let last = Rc::new(RefCell::new(String::new()));
    let primed = Rc::new(Cell::new(false)); // ignore whatever was copied before
    let weak = Rc::downgrade(ctx);
    glib::timeout_add_local(Duration::from_millis(1500), move || {
        let Some(ctx) = weak.upgrade() else { return glib::ControlFlow::Break };
        if !ctx.manager.settings().clipboard_monitor {
            primed.set(false);
            return glib::ControlFlow::Continue;
        }
        let Some(display) = gdk::Display::default() else { return glib::ControlFlow::Continue };
        let (last, primed) = (last.clone(), primed.clone());
        display.clipboard().read_text_async(gio::Cancellable::NONE, move |result| {
            let Ok(Some(text)) = result else { return };
            let text = text.trim().to_string();
            if *last.borrow() == text {
                return;
            }
            *last.borrow_mut() = text.clone();
            if primed.replace(true) && looks_like_file_link(&text) {
                ctx.handle_download(AddRequest { url: text, ..Default::default() }, false);
            }
        });
        glib::ControlFlow::Continue
    });
}
