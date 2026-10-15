//! "Download file info", like IDM's: the link and where to save it (one path: folder and name),
//! then start. The download quietly starts on one connection while the dialog is open (out of the
//! list), so Start feels instant, and moves to the path chosen when it's confirmed, even if it
//! already finished; Cancel throws away what was downloaded. Connections: as in Preferences.

mod link;

use super::theme::{self, icon};
use super::wnd::{self, field, is_checked, set_text, text, Rows, Window, FIELD, LINE};
use super::{progress, shell, App};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use turbodm::categories::category_for;
use turbodm::engine::manager::Confirm;
use turbodm::engine::AddRequest;
use turbodm::text::kind_words;
use turbodm::util::filename_from_url;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Form {
    window: Rc<Window>,
    url: Cell<HWND>,
    path: Cell<HWND>, // where it's saved: folder and name
    remember: Cell<HWND>,
    info: Cell<HWND>,
    chosen: Cell<bool>,   // the user typed or picked the path: don't replace it
    probe_id: Cell<u64>,  // results of older checks are ignored
    early: RefCell<Option<(String, String)>>, // (url, id) of the download started early
    probed: link::Probed, // a check's answer, from the network
    filled: Cell<bool>,   // the early download's name and folder were shown
    req: AddRequest,
}

impl Form {
    /// The download started early for `url`, if any; any other is thrown away.
    fn take_early(&self, app: &App, url: Option<&str>) -> Option<String> {
        let (early_url, id) = self.early.take()?;
        if Some(early_url.as_str()) == url {
            return Some(id);
        }
        app.manager.remove(&id, true);
        None
    }

    fn url(&self) -> String {
        text(self.url.get()).trim().to_string()
    }

    /// The path in the field as (folder, name); a bare name goes to the folder for its kind.
    fn target(&self, app: &App) -> (PathBuf, Option<String>) {
        let path = PathBuf::from(text(self.path.get()).trim());
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.trim().is_empty());
        let folder = match path.parent() {
            Some(dir) if dir.is_absolute() => dir.to_path_buf(),
            _ => app.manager.settings().folder_for(name.as_deref().unwrap_or(&filename_from_url(&self.url()))),
        };
        (folder, name)
    }

    /// Show where it goes (unless the user said where).
    fn show(&self, folder: &Path, name: &str) {
        if !self.chosen.get() {
            set_text(self.path.get(), &folder.join(name).display().to_string());
            to_end(self.path.get());
            self.update_remember();
        }
    }

    /// "Always save documents to this folder", for this kind of file.
    fn update_remember(&self) {
        let path = PathBuf::from(text(self.path.get()));
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.url());
        set_text(self.remember.get(), &format!("Always save {} to this folder", kind_words(&name)));
    }
}

pub fn open(app: &Rc<App>, req: AddRequest) {
    let settings = app.manager.settings();
    // a window of its own (with its own taskbar button), not the main window's: that may be
    // minimized or behind other programs when a download comes from the browser
    let window = Window::dialog("Download file info", None, (620, 220), false);
    let guess = req.filename.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| filename_from_url(&req.url));
    let folder = req.directory.clone().unwrap_or_else(|| settings.folder_for(&guess));
    let form = Rc::new(Form {
        window: window.clone(),
        url: Cell::default(), path: Cell::default(), remember: Cell::default(), info: Cell::default(),
        chosen: Cell::new(req.directory.is_some()), // e.g. picked in the browser's "Save as"
        probe_id: Cell::new(0),
        early: RefCell::new(None),
        probed: Default::default(),
        filled: Cell::new(false),
        req: req.clone(),
    });
    let f = Rc::downgrade(&form);
    form.url.set(window.edit(&req.url, move || if let Some(f) = f.upgrade() { link::changed(&f) }));
    let f = Rc::downgrade(&form);
    let path = window.edit(&folder.join(&guess).display().to_string(), move || if let Some(f) = f.upgrade() {
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
    field::make_field(path, vec![browse]); // the folder button inside the field, at its end
    form.remember.set(window.checkbox("", false, |_| {}));
    form.info.set(window.label("Enter a link"));
    window.set_text_color(form.info.get(), theme::DIM);
    let labels = ["Address", "Save to"].map(|l| window.name(l));
    let (a, f) = (app.clone(), form.clone());
    let start = window.default_button("Start download", move || submit(&a, &f, true));
    let (a, f) = (app.clone(), form.clone());
    let later = window.button("Download later", move || submit(&a, &f, false));
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.close());
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

    // laid out as the Linux dialog: names on the right of their column, then the fields
    let mut rows = Rows::new(18, 18, 84, 584);
    rows.field(Some(labels[0]), form.url.get(), FIELD);
    rows.field(Some(labels[1]), path, FIELD);
    rows.field(None, form.remember.get(), LINE + 6);
    rows.field(None, form.info.get(), LINE);
    rows.gap(6);
    let bottom = rows.buttons(&[cancel, later, start]);
    wnd::set_client_height(window.hwnd(), bottom + 18);
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
            .filter(|t| t.starts_with("http://") || t.starts_with("https://")) {
            set_text(form.url.get(), &text); // its EN_CHANGE checks it
        }
    } else {
        link::check(app, &form);
    }
}

/// A long path shows its end, the file's name.
fn to_end(edit: HWND) {
    wnd::send(edit, 0x00B1, usize::MAX, -1); // EM_SETSEL: no selection, the caret at the end
    wnd::send(edit, 0x00B7, 0, 0); // EM_SCROLLCARET
}

fn submit(app: &Rc<App>, form: &Rc<Form>, start_now: bool) {
    let url = form.url();
    if !url.starts_with("http") {
        set_text(form.info.get(), "Please enter an http(s) link");
        return;
    }
    let (directory, filename) = form.target(app);
    let connections = app.manager.settings().connections;
    if is_checked(form.remember.get()) {
        let mut s = app.manager.settings();
        s.folders.insert(category_for(filename.as_deref().unwrap_or(&url)).into(), directory.clone());
        app.manager.update_settings(s);
    }
    let id = match form.take_early(app, Some(&url)) {
        // moved to the chosen path when confirmed, finished or not
        Some(id) => {
            app.manager.confirm(&id, Confirm { filename, directory, connections, start: start_now });
            id
        }
        None => app.manager.add(AddRequest { url, filename, directory: Some(directory), connections: Some(connections),
                                             start: start_now, ..form.req.clone() }),
    };
    form.window.destroy(); // not close(): that would throw the download away
    if start_now && app.manager.settings().show_progress_window {
        progress::open(app, &id);
    }
}
