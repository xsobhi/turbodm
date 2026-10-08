//! The progress window's "Speed limiter" and "Options on completion" tabs.

use super::super::wnd::{enable, place, text, Window, FIELD, LINE};
use super::super::App;
use std::rc::Rc;
use turbodm::engine::Snapshot;
use turbodm::power::{OnDone, Power};
use windows::Win32::Foundation::HWND;

/// The controls of the speed limiter tab, placed from (`x`, `y`).
pub fn speed_limiter(window: &Window, app: &Rc<App>, snap: &Snapshot, x: i32, y: i32) -> Vec<HWND> {
    let limited = snap.speed_limit_kib > 0;
    let start = if limited { snap.speed_limit_kib as i32 } else { 1024 };
    let apply = {
        let (app, id) = (app.clone(), snap.id.clone());
        move |on: bool, kib: i32| app.manager.set_speed_limit(&id, if on { kib.max(1) as u64 } else { 0 })
    };
    let speed = Rc::new(std::cell::Cell::new(HWND::default()));
    let (a, s) = (apply.clone(), speed.clone());
    let enabled = window.checkbox("Use speed limiter", limited, move |on| {
        enable(s.get(), on);
        a(on, text(s.get()).parse().unwrap_or(start));
    });
    let label = window.label("Maximum download speed");
    let field = window.spin(1, 10_000_000, start, move |kib| apply(true, kib));
    speed.set(field);
    enable(field, limited);
    let unit = window.label("KiB/s");
    let note = window.note("Applies to this download only, right away. The limit for all downloads \
                            together is in Preferences.");
    place(enabled, x, y, 300, LINE + 4);
    place(label, x, y + 36, 150, LINE);
    place(field, x + 155, y + 32, 100, FIELD);
    place(unit, x + 262, y + 36, 60, LINE);
    place(note, x, y + 72, 460, 36);
    vec![enabled, label, field, unit, note]
}

/// The controls of the completion tab, placed from (`x`, `y`).
pub fn on_completion(window: &Window, app: &Rc<App>, id: &str, x: i32, y: i32) -> Vec<HWND> {
    let todo = app.on_done.borrow().get(id).copied().unwrap_or_default();
    let a = app.clone();
    let dialog = window.checkbox("Show download complete dialog", app.manager.settings().show_complete_dialog, move |on| {
        let mut s = a.manager.settings();
        s.show_complete_dialog = on;
        a.manager.update_settings(s);
    });
    let store = {
        let (app, id) = (app.clone(), id.to_string());
        move |change: &dyn Fn(&mut OnDone)| {
            let mut on_done = app.on_done.borrow_mut();
            change(on_done.entry(id.clone()).or_default());
        }
    };
    let s = store.clone();
    let open = window.checkbox("Open the file when done", todo.open_file, move |on| s(&|t| t.open_file = on));
    let label = window.label("When done, the computer should");
    let power = window.choice(&["Do nothing", "Sleep", "Shut down"], todo.power as usize, move |i| {
        store(&|t| t.power = Power::from_index(i as u32));
    });
    let note = window.note("Sleep and shut down wait 30 seconds first, so you can still cancel.");
    place(dialog, x, y, 400, LINE + 4);
    place(open, x, y + 26, 400, LINE + 4);
    place(label, x, y + 62, 190, LINE);
    place(power, x + 195, y + 58, 140, 200);
    place(note, x, y + 96, 460, 36);
    vec![dialog, open, label, power, note]
}
