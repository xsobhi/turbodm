//! When a download finishes or fails: IDM's "Download complete" dialog, the download's
//! "on completion" options, or a notification for downloads nobody was watching.

use super::theme;
use super::wnd::{self, is_checked, place, Rows, Window, LINE};
use std::path::Path;
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::HWND;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Controls::{IImageList, ImageList_Draw, HIMAGELIST, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHGetImageList, SHFILEINFOW, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE};
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

/// The file icon's size (Windows' "extra large" icons, 96-DPI pixels).
const ICON: i32 = 48;

fn complete_dialog(app: &Rc<App>, snap: &Snapshot) {
    let window = Window::new("Download complete", WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU, WINDOW_EX_STYLE(0), None, (560, 240));
    let icon = file_icon(&window, &snap.path);
    let heading = window.heading("Download complete");
    let name = window.label(&snap.filename);
    let fields = [("Address", snap.url.clone()), ("File size", human_size(snap.size)),
                  ("Saved to", snap.path.display().to_string())]
        .map(|(label, value)| (window.name(label), window.label(&value)));
    for (label, _) in &fields {
        window.set_text_color(*label, theme::DIM);
    }
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

    // as on Linux: the file's icon beside the heading and name, then the details
    place(icon, 18, 18, ICON, ICON);
    place(heading, 18 + ICON + 14, 18, 560 - 36 - ICON - 14, 28);
    place(name, 18 + ICON + 14, 48, 560 - 36 - ICON - 14, LINE);
    let mut rows = Rows::new(18, 18 + ICON + 16, 84, 524);
    for (label, value) in fields {
        rows.field(Some(label), value, LINE);
    }
    rows.gap(2);
    rows.full(again, LINE + 6);
    rows.gap(8);
    let bottom = rows.buttons(&[open, with, folder, close]);
    wnd::set_client_height(window.hwnd(), bottom + 18);
    wnd::center(window.hwnd());
    window.show();
}

/// The file's icon, big, as Explorer shows it (the program's own, for a program).
fn file_icon(window: &Window, path: &Path) -> HWND {
    let mut info = SHFILEINFOW::default();
    // SAFETY: queries into our struct; the image list is Windows' own, kept by the canvas
    let list = unsafe {
        SHGetFileInfoW(&HSTRING::from(path.as_os_str()), FILE_ATTRIBUTE_NORMAL, Some(&mut info),
                       std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX);
        SHGetImageList::<IImageList>(SHIL_EXTRALARGE as i32).ok()
    };
    window.canvas(move |p| {
        p.fill(p.rect, theme::CHROME);
        if let Some(list) = &list {
            // SAFETY: drawing an icon of the list into our canvas
            unsafe { let _ = ImageList_Draw(HIMAGELIST(list.as_raw() as isize), info.iIcon, p.dc, 0, 0, ILD_TRANSPARENT); }
        }
    })
}
