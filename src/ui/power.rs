//! "When done: sleep / shut down", as on Windows (a download's, or the scheduled queue's), with a 30-second
//! countdown that can be cancelled.

use super::Ctx;
use gtk::glib;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
pub use turbodm::power::{Power, COUNTDOWN_SECONDS};

const SECONDS: u32 = COUNTDOWN_SECONDS;

/// `done` says what finished: "“a.zip” has finished downloading."
pub fn countdown(ctx: &Rc<Ctx>, power: Power, done: &str) {
    if power == Power::Nothing {
        return;
    }
    let (what, now) = if power == Power::Sleep { ("go to sleep", "Sleep now") } else { ("shut down", "Shut down now") };
    let others = ctx.manager.snapshot().iter().filter(|s| s.status.is_active()).count();
    let mut detail = done.to_string();
    if others > 0 {
        detail.push_str(&format!(" {others} other download(s) will be paused and can be resumed later."));
    }
    let d = super::frame::dialog_with(turbodm::config::APP_NAME, None, 420, ":close", (14, 16));
    d.body.set_margin_bottom(14);
    let title = super::frame::heading("");
    d.body.append(&title);
    let body_text = super::frame::note(&detail);
    body_text.set_margin_top(super::frame::LINE_GAP);
    d.body.append(&body_text);
    let go = super::frame::button(now);
    let cancel = super::frame::default_button(&d.window, "Cancel");
    super::frame::buttons(&d.body, &[&go, &cancel], 14);
    let window = d.window.clone();

    let left = Rc::new(Cell::new(SECONDS));
    let label = move |t: &gtk::Label, n: u32| t.set_text(&format!("The computer will {what} in {n} seconds"));
    label(&title, SECONDS);
    let fire = {
        let (m, w) = (ctx.manager.clone(), window.clone());
        move || {
            w.close();
            if power == Power::Shutdown {
                m.pause_all(); // saved on the way out, resumable after boot
            }
            power.run();
        }
    };
    let (l, w, t, f) = (left.clone(), window.clone(), title.clone(), fire.clone());
    glib::timeout_add_seconds_local(1, move || {
        if !w.is_visible() {
            return glib::ControlFlow::Break; // cancelled or already done
        }
        l.set(l.get().saturating_sub(1));
        label(&t, l.get());
        if l.get() == 0 {
            f();
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
    go.connect_clicked(move |_| fire());
    let w = window.clone();
    cancel.connect_clicked(move |_| w.close());
    window.present();
}
