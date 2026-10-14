//! Preferences, laid out as on Linux: what each setting is on the left, its switch, number or
//! folder on the right; with OK and Cancel, as in Windows' own dialogs.

use super::wnd::{self, enable, is_checked, place, set_text, text, Window, FIELD, LINE};
use super::{shell, App};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::config::MAX_CONNECTIONS;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The names' column and the controls' (96-DPI pixels).
const NAMES: i32 = 340;
const CONTROLS: i32 = 230;
const PAD: i32 = 18;
const ROW: i32 = 36;

pub fn open(app: &Rc<App>) {
    let s = app.manager.settings();
    let width = PAD + NAMES + 16 + CONTROLS + PAD;
    let window = Window::dialog("Preferences", Some(app.main.window.hwnd()), (width, 700), false);
    let folder = Rc::new(RefCell::new(s.download_dir.clone()));
    let (f, w) = (folder.clone(), window.hwnd());
    let button = Rc::new(Cell::new(HWND::default()));
    let b = button.clone();
    let folder_button = window.button(&s.download_dir.display().to_string(), move || {
        if let Some(dir) = shell::choose_folder(w, "Download folder", &f.borrow().clone()) {
            set_text(b.get(), &dir.display().to_string());
            *f.borrow_mut() = dir;
        }
    });
    button.set(folder_button);
    // "Always save … to this folder" choices from the add dialog
    let forget_all = Rc::new(Cell::new(false));
    let remembered = s.folders.len();
    let (fa, button) = (forget_all.clone(), Rc::new(Cell::new(HWND::default())));
    let b = button.clone();
    let forget = window.button(&format!("Forget ({remembered})"), move || {
        fa.set(true);
        set_text(b.get(), "Forgotten");
        enable(b.get(), false);
    });
    button.set(forget);
    enable(forget, remembered > 0);
    let categories = window.switch(s.use_categories, |_| {});
    let spins = [
        ("Connections per download (servers may limit this)", 1, MAX_CONNECTIONS as i32, s.connections as i32),
        ("Downloads at the same time", 1, 16, s.max_parallel as i32),
        ("Speed limit, KiB/s (0 = unlimited)", 0, 10_000_000, s.speed_limit_kib as i32),
        ("Retries per connection", 0, 100, s.retries as i32),
        ("Reconnect after no data for (seconds)", 5, 300, s.timeout_secs as i32),
    ].map(|(label, min, max, value)| (label, window.spin(min, max, value, |_| {})));
    let switches = [
        ("Confirm downloads from the browser", s.show_add_dialog),
        ("Start downloading while the confirm dialog is open", s.predownload),
        ("Open a progress window when a download starts", s.show_progress_window),
        ("Show the download complete dialog", s.show_complete_dialog),
        ("Notify when downloads finish", s.notify_complete),
        ("Catch download links copied to the clipboard", s.clipboard_monitor),
        ("Resume unfinished downloads at start", s.auto_resume),
        ("Check for updates", s.check_updates),
    ].map(|(label, on)| (label, window.switch(on, |_| {})));

    let (a, w) = (app.clone(), window.clone());
    let numbers: Vec<HWND> = spins.iter().map(|(_, field)| *field).collect();
    let toggles: Vec<HWND> = switches.iter().map(|(_, switch)| *switch).collect();
    let ok = window.default_button("OK", move || {
        let mut s = a.manager.settings();
        s.download_dir = folder.borrow().clone();
        if forget_all.get() {
            s.folders.clear();
        }
        s.use_categories = is_checked(categories);
        let number = |i: usize| text(numbers[i]).parse::<i64>().unwrap_or(0).max(0);
        (s.connections, s.max_parallel) = (number(0) as usize, number(1) as usize);
        (s.speed_limit_kib, s.retries, s.timeout_secs) = (number(2) as u64, number(3) as u32, number(4) as u64);
        let on: Vec<bool> = toggles.iter().map(|t| is_checked(*t)).collect();
        (s.show_add_dialog, s.predownload, s.show_progress_window, s.show_complete_dialog) = (on[0], on[1], on[2], on[3]);
        (s.notify_complete, s.clipboard_monitor, s.auto_resume, s.check_updates) = (on[4], on[5], on[6], on[7]);
        a.manager.update_settings(s.clamp());
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());

    // one row per setting: its name, then its control
    let mut y = PAD;
    let mut row = |name: &str, control: HWND, width: i32| {
        place(window.label(name), PAD, y + (FIELD - LINE) / 2, NAMES, LINE);
        place(control, PAD + NAMES + 16, y, width, FIELD);
        y += ROW;
    };
    row("Download folder", folder_button, CONTROLS);
    row("Folders remembered for kinds of files", forget, 130);
    row("Sort into category folders", categories, 44);
    for (name, field) in spins {
        row(name, field, 160);
    }
    for (name, switch) in switches {
        row(name, switch, 44);
    }
    let x = PAD + NAMES + 16 + CONTROLS;
    place(ok, x - 96, y + 10, 96, FIELD + 2);
    place(cancel, x - 2 * 96 - 8, y + 10, 96, FIELD + 2);
    wnd::set_client_height(window.hwnd(), y + 10 + FIELD + 2 + PAD);
    wnd::center(window.hwnd());
    window.show();
}
