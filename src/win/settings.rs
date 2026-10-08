//! Preferences, with OK and Cancel as in Windows' own dialogs.

use super::wnd::{self, enable, is_checked, place, set_text, text, Rows, Window, FIELD, LINE};
use super::{shell, App};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::config::MAX_CONNECTIONS;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn open(app: &Rc<App>) {
    let s = app.manager.settings();
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let window = Window::new("Preferences", style, WINDOW_EX_STYLE(0), Some(app.main.window.hwnd()), (520, 500));
    let folder = Rc::new(RefCell::new(s.download_dir.clone()));
    let folder_label = window.label("Download folder");
    let folder_field = window.control(windows::Win32::UI::Controls::WC_EDITW, &s.download_dir.display().to_string(),
                                      WINDOW_STYLE(0x0800 | 0x0080), WS_EX_CLIENTEDGE).0; // read-only, scrolls
    let (f, w) = (folder.clone(), window.hwnd());
    let browse = window.button("...", move || {
        if let Some(dir) = shell::choose_folder(w, "Download folder", &f.borrow().clone()) {
            set_text(folder_field, &dir.display().to_string());
            *f.borrow_mut() = dir;
        }
    });
    // "Always save … to this folder" choices from the add dialog
    let forget_all = Rc::new(Cell::new(false));
    let remembered = s.folders.len();
    let forget_label = window.label("Folders remembered for kinds of files");
    let (fa, button) = (forget_all.clone(), Rc::new(Cell::new(Default::default())));
    let b = button.clone();
    let forget = window.button(&format!("Forget ({remembered})"), move || {
        fa.set(true);
        set_text(b.get(), "Forgotten");
        enable(b.get(), false);
    });
    button.set(forget);
    enable(forget, remembered > 0);
    let spins = [
        ("Connections per download (servers may limit this)", 1, MAX_CONNECTIONS as i32, s.connections as i32),
        ("Downloads at the same time", 1, 16, s.max_parallel as i32),
        ("Speed limit, KiB/s (0 = unlimited)", 0, 10_000_000, s.speed_limit_kib as i32),
        ("Retries per connection", 0, 100, s.retries as i32),
        ("Reconnect after no data for (seconds)", 5, 300, s.timeout_secs as i32),
    ].map(|(label, min, max, value)| (window.label(label), window.spin(min, max, value, |_| {})));
    let checks = [
        ("Sort into category folders", s.use_categories),
        ("Confirm downloads from the browser", s.show_add_dialog),
        ("Start downloading while the confirm dialog is open", s.predownload),
        ("Open a progress window when a download starts", s.show_progress_window),
        ("Show the download complete dialog", s.show_complete_dialog),
        ("Notify when downloads finish", s.notify_complete),
        ("Catch download links copied to the clipboard", s.clipboard_monitor),
        ("Resume unfinished downloads at start", s.auto_resume),
        ("Check for updates", s.check_updates),
    ].map(|(label, on)| window.checkbox(label, on, |_| {}));

    let (a, w) = (app.clone(), window.clone());
    let ok = window.default_button("OK", move || {
        let mut s = a.manager.settings();
        s.download_dir = folder.borrow().clone();
        if forget_all.get() {
            s.folders.clear();
        }
        let number = |i: usize| text(spins[i].1).parse::<i64>().unwrap_or(0).max(0);
        s.connections = number(0) as usize;
        s.max_parallel = number(1) as usize;
        s.speed_limit_kib = number(2) as u64;
        s.retries = number(3) as u32;
        s.timeout_secs = number(4) as u64;
        let on: Vec<bool> = checks.iter().map(|c| is_checked(*c)).collect();
        (s.use_categories, s.show_add_dialog, s.predownload, s.show_progress_window, s.show_complete_dialog) =
            (on[0], on[1], on[2], on[3], on[4]);
        (s.notify_complete, s.clipboard_monitor, s.auto_resume, s.check_updates) = (on[5], on[6], on[7], on[8]);
        a.manager.update_settings(s.clamp());
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());

    let mut rows = Rows::new(16, 16, 330, 488);
    place(folder_label, 16, rows.y + 4, 120, LINE);
    place(folder_field, 140, rows.y, 320, FIELD);
    place(browse, 466, rows.y, 38, FIELD);
    rows.gap(FIELD + 8);
    rows.short(Some(forget_label), forget, 100);
    for (label, field) in spins {
        rows.short(Some(label), field, 100);
    }
    rows.gap(4);
    for check in checks {
        rows.full(check, LINE + 2);
    }
    rows.gap(8);
    let bottom = rows.buttons(&[ok, cancel]);
    wnd::set_client_height(window.hwnd(), bottom + 16);
    wnd::center(window.hwnd());
    window.show();
}
