//! "When done: sleep / shut down", with a 30-second countdown that can be cancelled.

use super::wnd::{self, set_text, Rows, Window};
use super::App;
use std::cell::Cell;
use std::rc::Rc;
use turbodm::power::{Power, COUNTDOWN_SECONDS};
use windows::Win32::Foundation::LRESULT;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn countdown(app: &Rc<App>, power: Power, filename: &str) {
    if power == Power::Nothing {
        return;
    }
    let (what, now) = if power == Power::Sleep { ("go to sleep", "Sleep now") } else { ("shut down", "Shut down now") };
    let others = app.manager.snapshot().iter().filter(|s| s.status.is_active()).count();
    let mut detail = format!("\u{201c}{filename}\u{201d} has finished downloading.");
    if others > 0 {
        detail.push_str(&format!(" {others} other download(s) will be paused and can be resumed later."));
    }
    let window = Window::new("TurboDM", WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU, WS_EX_TOPMOST, None, (420, 150));
    let title = window.heading("");
    let body = window.note(&detail);
    let fire = {
        let (m, w) = (app.manager.clone(), window.clone());
        move || {
            w.destroy();
            if power == Power::Shutdown {
                m.pause_all(); // saved on the way out, resumable after boot
            }
            power.run();
        }
    };
    let f = fire.clone();
    let go = window.button(now, f);
    let w = window.clone();
    let cancel = window.default_button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let left = Rc::new(Cell::new(COUNTDOWN_SECONDS));
    let label = move |n: u32| set_text(title, &format!("The computer will {what} in {n} seconds"));
    label(COUNTDOWN_SECONDS);
    window.hook(move |msg, _, _| {
        if msg != WM_TIMER {
            return None;
        }
        left.set(left.get().saturating_sub(1));
        label(left.get());
        if left.get() == 0 {
            fire();
        }
        Some(LRESULT(0))
    });
    let mut rows = Rows::new(16, 14, 0, 388);
    rows.full(title, 24);
    rows.full(body, 34);
    rows.gap(6);
    let bottom = rows.buttons(&[go, cancel]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    // SAFETY: a one-second timer on our window
    unsafe { SetTimer(Some(window.hwnd()), 1, 1000, None) };
    window.show();
}
