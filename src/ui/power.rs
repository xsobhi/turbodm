//! "When done: sleep / shut down", with a 30-second countdown that can be cancelled.

use super::Ctx;
use gtk::glib;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Power {
    #[default]
    Nothing = 0,
    Sleep = 1,
    Shutdown = 2,
}

impl Power {
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Power::Sleep,
            2 => Power::Shutdown,
            _ => Power::Nothing,
        }
    }

    fn run(self) {
        // logind lets the active local session do this without a password
        #[cfg(not(windows))]
        let command = ("systemctl", vec![if self == Power::Sleep { "suspend" } else { "poweroff" }]);
        #[cfg(windows)]
        let command = match self {
            Power::Sleep => ("rundll32.exe", vec!["powrprof.dll,SetSuspendState", "0,1,0"]),
            _ => ("shutdown.exe", vec!["/s", "/t", "0"]),
        };
        if let Err(err) = std::process::Command::new(command.0).args(&command.1).spawn() {
            eprintln!("turbodm: {}: {err}", command.0);
        }
    }
}

const SECONDS: u32 = 30;

pub fn countdown(ctx: &Rc<Ctx>, power: Power, filename: &str) {
    if power == Power::Nothing {
        return;
    }
    let (what, now) = if power == Power::Sleep { ("go to sleep", "Sleep now") } else { ("shut down", "Shut down now") };
    let others = ctx.manager.snapshot().iter().filter(|s| s.status.is_active()).count();
    let mut detail = format!("“{filename}” has finished downloading.");
    if others > 0 {
        detail.push_str(&format!(" {others} other download(s) will be paused and can be resumed later."));
    }
    let title = gtk::Label::builder().xalign(0.0).css_classes(["title-3"]).build();
    let body_text = gtk::Label::builder().label(&detail).xalign(0.0).wrap(true).max_width_chars(50).build();
    let cancel = gtk::Button::builder().label("Cancel").css_classes(["suggested-action"]).build();
    let go = gtk::Button::with_label(now);
    let buttons = gtk::Box::builder().spacing(8).halign(gtk::Align::End).build();
    buttons.append(&go);
    buttons.append(&cancel);
    let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12)
        .margin_top(18).margin_bottom(18).margin_start(18).margin_end(18).build();
    body.append(&title);
    body.append(&body_text);
    body.append(&buttons);
    let window = gtk::Window::builder().title("TurboDM").child(&body).build();

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
