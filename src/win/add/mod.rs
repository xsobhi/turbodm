//! "Download file info", like IDM's: the link and where to save it (one path: folder and name),
//! then start. The download quietly starts on one connection while the dialog is open (out of the
//! list), so Start feels instant, and moves to the path chosen when it's confirmed, even if it
//! already finished; Cancel throws away what was downloaded. Connections: as in Preferences.
//! A web page with a video (video.rs) shows the video's title and its qualities instead.
//! A torrent (magnet link, .torrent file) is only looked up meanwhile: what's in it and how big.

mod form;
mod link;
mod start;
mod video;

pub use start::add_video;
pub mod torrent; // add_torrent_file, for the main window's menu

use super::theme::{self, icon};
use super::wnd::{self, field, is_checked, set_text, text, Window};
use super::{progress, shell, App};
use form::Form;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use turbodm::categories::category_for;
use turbodm::engine::manager::Confirm;
use turbodm::engine::torrent::{is_magnet, is_torrent};
use turbodm::engine::AddRequest;
use turbodm::util::name_for;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn open(app: &Rc<App>, req: AddRequest) {
    open_form(app, req, false);
}

/// The dialog, for a file (or whatever the address turns out to be), or for a `video`.
fn open_form(app: &Rc<App>, req: AddRequest, video: bool) {
    let settings = app.manager.settings();
    // a window of its own (with its own taskbar button), not the main window's: that may be
    // minimized or behind other programs when a download comes from the browser
    let window = Window::dialog(if video { "Download video" } else { "Download file info" }, None, (620, 220), false);
    let guess = name_for(&req.url, req.filename.as_deref());
    let folder = req.directory.clone().unwrap_or_else(|| settings.folder_for(&guess));
    let form = Rc::new(Form {
        window: window.clone(),
        url: Cell::default(), path: Cell::default(), browse: Cell::default(), remember: Cell::default(),
        info: Cell::default(), names: Cell::default(), buttons: Cell::default(),
        chosen: Cell::new(req.directory.is_some()), // e.g. picked in the browser's "Save as"
        probe_id: Cell::new(0),
        early: RefCell::new(None),
        probed: Default::default(),
        filled: Cell::new(false),
        req: req.clone(),
        video: video::State::new(video),
    });
    let f = Rc::downgrade(&form);
    form.url.set(window.edit(&req.url, move || if let Some(f) = f.upgrade() { link::changed(&f) }));
    let f = Rc::downgrade(&form);
    let open_torrent = window.icon_button(icon::OPEN_FILE, move || {
        let Some(f) = f.upgrade() else { return };
        if let Some(file) = torrent::pick(f.window.hwnd()) {
            set_text(f.url.get(), &file.display().to_string()); // its EN_CHANGE checks it
        }
    });
    field::make_field(form.url.get(), vec![open_torrent]); // a .torrent file instead of a link
    let f = Rc::downgrade(&form);
    let initial = if video { String::new() } else { folder.join(&guess).display().to_string() };
    let path = window.edit(&initial, move || if let Some(f) = f.upgrade() {
        // SAFETY: plain query (typed by the user, not filled in by us)
        if unsafe { GetFocus() } == f.path.get() {
            f.chosen.set(true);
        }
        f.update_remember();
    });
    form.path.set(path);
    let f = Rc::downgrade(&form);
    let browse = window.icon_button(icon::FOLDER_OPEN, move || {
        let Some(f) = f.upgrade() else { return };
        let now = PathBuf::from(text(f.path.get()).trim());
        if let Some(picked) = shell::choose_save(f.window.hwnd(), "Save as", &now) {
            set_text(f.path.get(), &picked.display().to_string());
            to_end(f.path.get());
            f.chosen.set(true);
            f.update_remember();
        }
    });
    form.browse.set(browse);
    field::make_field(path, vec![browse]); // the folder button inside the field, at its end
    form.remember.set(window.checkbox("", false, |_| {}));
    form.info.set(window.label("Enter a link"));
    window.set_text_color(form.info.get(), theme::DIM);
    form.video.title.set(window.label(""));
    form.names.set(["Address", "Save to", "Quality", "Format"].map(|l| window.name(l)));
    let (a, f) = (app.clone(), form.clone());
    let start = window.default_button("Start download", move || submit(&a, &f, true));
    let (a, f) = (app.clone(), form.clone());
    let later = window.button("Download later", move || submit(&a, &f, false));
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.close());
    form.buttons.set([cancel, later, start]);
    // Escape and the close button cancel too: delete what was downloaded early
    let (a, f) = (Rc::downgrade(app), Rc::downgrade(&form));
    window.on_close(move || {
        if let (Some(a), Some(f)) = (a.upgrade(), f.upgrade()) {
            f.take_early(&a, None);
        }
        true
    });
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.close());

    form.layout();
    to_end(path);
    form.update_remember();
    link::start_timer(&form, app);
    wnd::center(window.hwnd());
    window.show_in_front();
    // SAFETY: plain focus change in our dialog
    unsafe { let _ = SetFocus(Some(form.url.get())); }
    wnd::send(form.url.get(), 0x00B1, 0, -1); // EM_SETSEL: all
    if req.url.is_empty() {
        // like IDM: a link on the clipboard is the likely one
        if let Some(text) = shell::clipboard_text().map(|t| t.trim().to_string())
            .filter(|t| t.starts_with("http://") || t.starts_with("https://") || is_magnet(t)) {
            set_text(form.url.get(), &text); // its EN_CHANGE checks it
        }
    } else {
        link::check(app, &form);
    }
}

/// A long path shows its end, the file's name.
fn to_end(edit: HWND) {
    let end = text(edit).encode_utf16().count();
    wnd::send(edit, 0x00B1, end, end as isize); // EM_SETSEL: the caret at the end, nothing selected
    wnd::send(edit, 0x00B7, 0, 0); // EM_SCROLLCARET
}

/// Something the dialog can download: a web link, a magnet link or a .torrent file.
fn is_link(url: &str) -> bool {
    url.starts_with("http") || is_torrent(url, None)
}

fn submit(app: &Rc<App>, form: &Rc<Form>, start_now: bool) {
    let url = form.url();
    if !is_link(&url) {
        set_text(form.info.get(), "Please enter a link: http(s), a magnet link or a .torrent file");
        return;
    }
    if form.video.wanted || form.video.found.borrow().is_some() {
        start::submit(app, form, start_now);
        return;
    }
    let (directory, filename) = form.target(app);
    let connections = app.manager.settings().connections;
    remember_folder(app, form, filename.as_deref().unwrap_or(&url), &directory);
    let id = match form.take_early(app, Some(&url)) {
        // moved to the chosen path when confirmed, finished or not
        Some(id) => {
            app.manager.confirm(&id, Confirm { filename, directory, connections, start: start_now });
            id
        }
        None => app.manager.add(AddRequest { url, filename, directory: Some(directory), connections: Some(connections),
                                             start: start_now, ..form.req.clone() }),
    };
    done(app, form, &id, start_now);
}

/// "Always save … to this folder", if ticked.
fn remember_folder(app: &App, form: &Form, name: &str, directory: &Path) {
    if is_checked(form.remember.get()) {
        let mut s = app.manager.settings();
        s.folders.insert(category_for(name).into(), directory.to_path_buf());
        app.manager.update_settings(s);
    }
}

/// Added: the dialog goes, the progress window comes (if it's started and wanted).
fn done(app: &Rc<App>, form: &Form, id: &str, started: bool) {
    form.window.destroy(); // not close(): that would throw the download away
    if started && app.manager.settings().show_progress_window {
        progress::open(app, id);
    }
}
