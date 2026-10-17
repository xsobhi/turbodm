//! Small dialogs: Windows' message boxes, "Refresh download address", About.

use super::wnd::{self, text, Rows, Window, FIELD, LINE};
use super::App;
use std::rc::Rc;
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Windows' message box, with an information or error icon.
pub fn message(owner: HWND, title: &str, body: &str, error: bool) {
    let icon = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    // SAFETY: a modal message box with our texts
    unsafe { MessageBoxW(Some(owner), &HSTRING::from(body), &HSTRING::from(title), MB_OK | icon); }
}

/// Delete (the key, the toolbar, the menus), as IDM asks: off the list, and with "Also delete
/// the files from the disk" ticked, their files too. The box is as it was answered last, or
/// ticked for "Delete with file".
pub fn confirm_delete(app: &Rc<App>, ids: Vec<String>, with_files: bool) {
    use super::wnd::{self, is_checked, Rows, Window, LINE};
    if ids.is_empty() {
        return;
    }
    let names: Vec<String> = ids.iter().filter_map(|id| app.manager.get(id)).map(|s| s.filename).collect();
    let heading = match ids.len() {
        1 => "Delete this download?".to_string(),
        n => format!("Delete these {n} downloads?"),
    };
    let mut listed = names.iter().take(3).map(|n| format!("\u{201c}{n}\u{201d}")).collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        listed.push_str(&format!(" and {} more", names.len() - 3));
    }
    let window = Window::dialog("Delete", Some(app.main.window.hwnd()), (440, 160), false);
    let title = window.heading(&heading);
    let note = window.note(&format!("{listed} will be taken off the list."));
    let also = window.checkbox("Also delete the files from the disk", with_files || app.manager.settings().delete_files_too,
                               |_| {});
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let (a, w) = (app.clone(), window.clone());
    let delete = window.default_button("Delete", move || {
        let files = is_checked(also);
        ids.iter().for_each(|id| a.manager.remove(id, files));
        if !with_files {
            // (remembered for next time; "Delete with file" doesn't change it)
            let mut s = a.manager.settings();
            if s.delete_files_too != files {
                s.delete_files_too = files;
                a.manager.update_settings(s);
            }
        }
        w.destroy();
    });
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let mut rows = Rows::new(18, 16, 0, 404);
    rows.full(title, 24);
    rows.full(note, 2 * LINE);
    rows.full(also, 28);
    rows.gap(4);
    let bottom = rows.buttons(&[cancel, delete]);
    wnd::set_client_height(window.hwnd(), bottom + 16);
    wnd::center(window.hwnd());
    window.show();
}

/// IDM's Redownload: the file deleted, and downloaded again from the start.
pub fn confirm_redownload(app: &Rc<App>, ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    let what = match ids.len() {
        1 => app.manager.get(&ids[0]).map_or("it".into(), |s| format!("\u{201c}{}\u{201d}", s.filename)),
        n => format!("{n} downloads"),
    };
    let body = format!("Download {what} again from the start?\n\nWhat's downloaded so far is deleted.");
    // SAFETY: a modal question box
    let answer = unsafe {
        MessageBoxW(Some(app.main.window.hwnd()), &HSTRING::from(body), &HSTRING::from("Redownload"),
                    MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2)
    };
    if answer == IDYES {
        ids.iter().for_each(|id| app.manager.redownload(id));
    }
}

/// A finished download's file isn't there any more (moved or deleted outside TurboDM): download
/// it again, or take it off the list.
pub fn missing_file(app: &Rc<App>, id: &str) {
    let Some(snap) = app.manager.get(id) else { return };
    let window = Window::dialog("File missing", Some(app.main.window.hwnd()), (460, 150), false);
    let heading = window.heading("The file isn't there any more");
    let note = window.note(&format!("\u{201c}{}\u{201d} was moved or deleted outside TurboDM. It was in {}.",
                                    snap.filename, snap.directory.display()));
    let (a, w, i) = (app.clone(), window.clone(), id.to_string());
    let again = window.default_button("Download again", move || {
        a.manager.redownload(&i);
        w.destroy();
    });
    let (a, w, i) = (app.clone(), window.clone(), id.to_string());
    let remove = window.button("Remove from list", move || {
        a.manager.remove(&i, false);
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let mut rows = Rows::new(16, 14, 0, 428);
    rows.full(heading, 24);
    rows.full(note, 3 * LINE);
    rows.gap(4);
    let bottom = rows.buttons(&[cancel, remove, again]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}

/// IDM's "Refresh download address": paste a fresh link for an expired one.
pub fn refresh_address(app: &Rc<App>, id: &str) {
    let Some(snap) = app.manager.get(id) else { return };
    let window = Window::dialog("Refresh download address", Some(app.main.window.hwnd()), (520, 150), false);
    let hint = window.note(&format!("New link for \u{201c}{}\u{201d}. Progress is kept if the server returns the same file.",
                                    snap.filename));
    let field = window.edit(&snap.url, || {});
    let (a, w, id) = (app.clone(), window.clone(), id.to_string());
    let ok = window.default_button("Refresh", move || {
        let url = text(field).trim().to_string();
        if url.starts_with("http") {
            a.manager.refresh_address(&id, url);
        }
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let mut rows = Rows::new(14, 14, 0, 492);
    rows.full(hint, 32);
    rows.full(field, FIELD);
    rows.gap(4);
    let bottom = rows.buttons(&[ok, cancel]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}

pub fn about(app: &Rc<App>) {
    let body = format!("TurboDM {}\n\nFast multi-connection download manager, with browser integration.\n\n\
                        https://github.com/xsobhi/turbodm\nMIT license.", turbodm::config::VERSION);
    message(app.main.window.hwnd(), "About TurboDM", &body, false);
}
