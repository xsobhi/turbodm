//! When a download finishes or fails: IDM's "Download complete" dialog, the download's
//! "on completion" options, or a notification for downloads nobody was watching.

use super::wnd::{self, is_checked, Rows, Window, LINE};
use super::{power, progress, shell, tray, App};
use std::rc::Rc;
use turbodm::engine::{Snapshot, Status};
use turbodm::power::Power;
use turbodm::util::human_size;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn on_status(app: &Rc<App>, snap: Snapshot) {
    match snap.status {
        Status::Completed => completed(app, snap),
        Status::Error if app.manager.settings().notify_complete => {
            let error = snap.error.clone().unwrap_or_default();
            tray::notify(app, "Download failed", &format!("{}\n{error}", snap.filename), None);
        }
        _ => {}
    }
}

fn completed(app: &Rc<App>, snap: Snapshot) {
    progress::close(app, &snap.id);
    // downloads with a progress window (now or earlier) get the dialog, like IDM
    let watched = app.on_done.borrow_mut().remove(&snap.id);
    let settings = app.manager.settings();
    if let Some(todo) = watched {
        if todo.open_file {
            shell::open(&snap.path);
        }
        if settings.show_complete_dialog && todo.power == Power::Nothing {
            complete_dialog(app, &snap);
        }
        power::countdown(app, todo.power, &snap.filename);
        if settings.show_complete_dialog {
            return;
        }
    }
    if settings.notify_complete {
        tray::notify(app, "Download complete", &snap.filename, Some(snap.id.clone()));
    }
}

fn complete_dialog(app: &Rc<App>, snap: &Snapshot) {
    let window = Window::new("Download complete", WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU, WINDOW_EX_STYLE(0), None, (520, 200));
    let heading = window.heading("Download complete");
    let name = window.label(&snap.filename);
    let fields = [("Address", snap.url.clone()), ("File size", human_size(snap.size)),
                  ("Saved to", snap.path.display().to_string())]
        .map(|(label, value)| (window.label(label), window.label(&value)));
    let again = window.checkbox("Don't show this dialog again", false, |_| {});
    let (path, w) = (snap.path.clone(), window.clone());
    let open = window.default_button("Open", move || { shell::open(&path); w.close(); });
    let (path, w) = (snap.path.clone(), window.clone());
    let with = window.button("Open with...", move || { shell::open_with(&path); w.close(); });
    let (a, id, w) = (app.clone(), snap.id.clone(), window.clone());
    let folder = window.button("Open folder", move || { shell::show_download(&a, &id); w.close(); });
    let w = window.clone();
    let close = window.button("Close", move || w.close());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.close());
    let a = app.clone();
    window.on_close(move || {
        if is_checked(again) {
            let mut s = a.manager.settings();
            s.show_complete_dialog = false;
            a.manager.update_settings(s);
        }
        true
    });

    let mut rows = Rows::new(16, 14, 80, 488);
    rows.full(heading, 24);
    rows.full(name, LINE);
    rows.gap(6);
    for (label, value) in fields {
        rows.field(Some(label), value, LINE);
    }
    rows.gap(4);
    rows.full(again, LINE + 4);
    rows.gap(6);
    let bottom = rows.buttons(&[open, with, folder, close]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}
