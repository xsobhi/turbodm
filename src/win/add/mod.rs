//! "Download file info", like IDM's: check the link, pick name, folder and connections, start.
//! The download quietly starts on one connection while the dialog is open (out of the list),
//! so Start feels instant; Cancel throws away what was downloaded.

mod link;

use super::wnd::{self, is_checked, place, set_text, text, Rows, Window, FIELD, LINE};
use super::{progress, shell, App};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use turbodm::config::MAX_CONNECTIONS;
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
    name: Cell<HWND>,
    folder: Cell<HWND>,
    remember: Cell<HWND>,
    connections: Cell<HWND>,
    info: Cell<HWND>,
    dir: RefCell<PathBuf>,
    dir_chosen: Cell<bool>,  // the user picked a folder: stop choosing by category
    name_edited: Cell<bool>, // the user typed a name: don't replace it
    probe_id: Cell<u64>,     // results of older checks are ignored
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

    fn show_folder(&self) {
        set_text(self.folder.get(), &self.dir.borrow().display().to_string());
    }

    /// "Always save documents to this folder", for this kind of file.
    fn update_remember(&self) {
        let name = text(self.name.get());
        let kind = kind_words(if name.is_empty() { self.url() } else { name }.as_str());
        set_text(self.remember.get(), &format!("Always save {kind} to this folder"));
    }
}

pub fn open(app: &Rc<App>, req: AddRequest) {
    let settings = app.manager.settings();
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let owner = app.main.is_visible().then(|| app.main.window.hwnd());
    let window = Window::new("Download File Info", style, WINDOW_EX_STYLE(0), owner, (560, 250));
    let guess = req.filename.clone().unwrap_or_else(|| filename_from_url(&req.url));
    let form = Rc::new(Form {
        window: window.clone(),
        url: Cell::default(), name: Cell::default(), folder: Cell::default(), remember: Cell::default(),
        connections: Cell::default(), info: Cell::default(),
        dir: RefCell::new(req.directory.clone().unwrap_or_else(|| settings.folder_for(&guess))),
        dir_chosen: Cell::new(req.directory.is_some()), // e.g. picked in the browser's "Save as"
        name_edited: Cell::new(req.filename.is_some()),
        probe_id: Cell::new(0),
        early: RefCell::new(None),
        probed: Default::default(),
        filled: Cell::new(false),
        req: req.clone(),
    });
    let f = Rc::downgrade(&form);
    form.url.set(window.edit(&req.url, move || if let Some(f) = f.upgrade() { link::changed(&f) }));
    let f = Rc::downgrade(&form);
    form.name.set(window.edit(req.filename.as_deref().unwrap_or(""), move || if let Some(f) = f.upgrade() {
        // SAFETY: plain query
        f.name_edited.set(unsafe { GetFocus() } == f.name.get());
        f.update_remember();
    }));
    let folder = window.control(windows::Win32::UI::Controls::WC_EDITW, "", WINDOW_STYLE(0x0800 | 0x0080), // ES_READONLY | ES_AUTOHSCROLL
                                WS_EX_CLIENTEDGE).0;
    form.folder.set(folder);
    let f = Rc::downgrade(&form);
    let browse = window.button("...", move || {
        let Some(f) = f.upgrade() else { return };
        if let Some(dir) = shell::choose_folder(f.window.hwnd(), "Save to folder", &f.dir.borrow().clone()) {
            *f.dir.borrow_mut() = dir;
            f.dir_chosen.set(true);
            f.show_folder();
        }
    });
    form.remember.set(window.checkbox("", false, |_| {}));
    form.connections.set(window.spin(1, MAX_CONNECTIONS as i32, settings.connections as i32, |_| {}));
    form.info.set(window.label("Enter a link"));
    let labels = ["URL", "Save as", "Folder", "Connections"].map(|l| window.label(l));
    let (a, f) = (app.clone(), form.clone());
    let start = window.default_button("Start Download", move || submit(&a, &f, true));
    let (a, f) = (app.clone(), form.clone());
    let later = window.button("Download Later", move || submit(&a, &f, false));
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

    let mut rows = Rows::new(14, 14, 90, 532);
    rows.field(Some(labels[0]), form.url.get(), FIELD);
    rows.field(Some(labels[1]), form.name.get(), FIELD);
    place(labels[2], 14, rows.y + 4, 82, LINE);
    place(folder, 104, rows.y, 400, FIELD);
    place(browse, 510, rows.y, 36, FIELD);
    rows.gap(FIELD + 6);
    rows.field(None, form.remember.get(), LINE + 4);
    rows.short(Some(labels[3]), form.connections.get(), 70);
    rows.field(None, form.info.get(), LINE);
    rows.gap(6);
    let bottom = rows.buttons(&[start, later, cancel]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    form.show_folder();
    form.update_remember();
    link::start_timer(&form, app);
    wnd::center(window.hwnd());
    window.show();
    // SAFETY: plain focus change in our dialog
    unsafe { let _ = SetFocus(Some(form.url.get())); }
    send_select_all(form.url.get());
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

fn send_select_all(edit: HWND) {
    wnd::send(edit, 0x00B1, 0, -1); // EM_SETSEL: all
}

fn submit(app: &Rc<App>, form: &Rc<Form>, start_now: bool) {
    let url = form.url();
    if !url.starts_with("http") {
        set_text(form.info.get(), "Please enter an http(s) link");
        return;
    }
    let name = text(form.name.get()).trim().to_string();
    let filename = (!name.is_empty()).then_some(name);
    let directory = form.dir.borrow().clone();
    let connections = text(form.connections.get()).parse().unwrap_or(app.manager.settings().connections)
        .clamp(1, MAX_CONNECTIONS);
    if is_checked(form.remember.get()) {
        let kind = turbodm::categories::category_for(filename.as_deref().unwrap_or(&url));
        let mut s = app.manager.settings();
        s.folders.insert(kind.into(), directory.clone());
        app.manager.update_settings(s);
    }
    let id = match form.take_early(app, Some(&url)) {
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
