//! "Download file info", as on Windows (src/win/add) and like IDM's: the link and where to save
//! it (one path: folder and name), then start. The download quietly starts on one connection
//! while the dialog is open (out of the list), so Start feels instant, and moves to the path
//! chosen when it's confirmed, even if it already finished; Cancel throws away what was
//! downloaded. Connections: as in Options. A web page with a video (video.rs) shows the video's
//! title and its qualities instead. A torrent (magnet link, .torrent file) is only looked up
//! meanwhile: what's in it and how big.

mod exists;
mod form;
mod link;
mod torrent;
mod video;

pub use torrent::add_torrent_file;

use super::{frame, progress, Ctx};
use form::Form;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use turbodm::categories::category_for;
use turbodm::engine::manager::Confirm;
use turbodm::engine::torrent::{is_magnet, is_torrent};
use turbodm::engine::AddRequest;
use turbodm::util::name_for;

/// For a download from the browser, the clipboard or the command line.
pub fn open(ctx: &Rc<Ctx>, req: AddRequest) {
    open_form(ctx, req, false, false);
}

/// "Add URL" (or a link), from the main window.
pub fn open_from_menu(ctx: &Rc<Ctx>, req: AddRequest) {
    open_form(ctx, req, false, true);
}

/// "Download video…", from the main window: a link on the clipboard is filled in.
pub fn add_video(ctx: &Rc<Ctx>) {
    open_form(ctx, AddRequest::default(), true, true);
}

/// A video on a web page, from the browser: its qualities in the Add dialog, or (no dialog)
/// straight to the list in the quality picked last, once the page is looked at.
pub fn handle_video(ctx: &Rc<Ctx>, req: AddRequest, silent: bool) {
    if !silent && ctx.manager.settings().show_add_dialog {
        open_form(ctx, req, true, false);
    } else {
        // (the quality picked in the browser's menu, if it's from there; else the usual)
        let video = Some(req.video.clone().unwrap_or_default());
        let id = ctx.manager.add(AddRequest { start: true, video, ..req });
        if ctx.manager.settings().show_progress_window {
            progress::open(ctx, &id);
        }
    }
}

/// The dialog, for a file (or whatever the address turns out to be), or for a `video`.
/// `from_main`: asked for in the main window, it's over it; else a window of its own, as the main
/// window may be hidden when a download comes from the browser.
fn open_form(ctx: &Rc<Ctx>, req: AddRequest, video: bool, from_main: bool) {
    let settings = ctx.manager.settings();
    let owner: Option<gtk::Window> = from_main.then(|| ctx.win.window.clone().upcast());
    let d = frame::dialog_with(if video { "Download video" } else { "Download file info" }, owner.as_ref(), 620,
                               ":close", (18, 18));
    let guess = name_for(&req.url, req.filename.as_deref());
    let folder = req.directory.clone().unwrap_or_else(|| settings.folder_for(&guess));
    let (url, _) = frame::entry_with_button(&req.url, "text-x-generic-symbolic", "Open a torrent file");
    let (path, _) = frame::entry_with_button("", "folder-open-symbolic", "Choose where to save it");
    let form = Rc::new(Form {
        window: d.window.clone(),
        title: d.title.clone(),
        url,
        path,
        remember: gtk::CheckButton::new(),
        info: gtk::Label::builder().xalign(0.0).label("Enter a link").css_classes(["dim-label"])
            .ellipsize(gtk::pango::EllipsizeMode::End).max_width_chars(10).hexpand(true).build(),
        quality_row: gtk::Box::builder().spacing(8).visible(false).build(),
        quality_name: frame::name("Quality"),
        video_title: frame::label(""),
        chosen: Cell::new(req.directory.is_some()), // e.g. picked in the browser's "Save as"
        filling: Cell::new(false),
        probe_id: Cell::new(0),
        early: RefCell::new(None),
        probed: Default::default(),
        filled: Cell::new(false),
        req: req.clone(),
        video: video::State::new(video),
    });
    form.video_title.set_visible(false);
    form.quality_name.set_visible(false);
    if !video {
        form.set_path(&folder.join(&guess));
    }
    let grid = frame::grid();
    grid.attach(&frame::name_sized("Address", 72), 0, 0, 1, 1);
    grid.attach(&form.url, 1, 0, 1, 1);
    grid.attach(&frame::name_sized("Save to", 72), 0, 1, 1, 1);
    grid.attach(&form.path, 1, 1, 1, 1);
    grid.attach(&form.quality_name, 0, 2, 1, 1);
    grid.attach(&form.quality_row, 1, 2, 1, 1);
    grid.attach(&form.remember, 1, 3, 1, 1);
    grid.attach(&form.video_title, 1, 4, 1, 1);
    grid.attach(&form.info, 1, 5, 1, 1);
    video::build(ctx, &form);
    d.body.append(&grid);
    let cancel = frame::button("Cancel");
    let later = frame::button("Download later");
    let start = frame::default_button(&d.window, "Start download");
    frame::buttons(&d.body, &[&cancel, &later, &start], 16);

    let (f, c) = (Rc::downgrade(&form), Rc::downgrade(ctx));
    form.url.connect_changed(move |_| {
        let (Some(f), Some(c)) = (f.upgrade(), c.upgrade()) else { return };
        link::changed(&c, &f);
    });
    let f = Rc::downgrade(&form);
    form.url.connect_icon_press(move |_, _| {
        if let Some(f) = f.upgrade() {
            torrent::pick(&f);
        }
    });
    let f = Rc::downgrade(&form);
    form.path.connect_changed(move |_| {
        let Some(f) = f.upgrade() else { return };
        if !f.filling.get() {
            f.chosen.set(true); // typed by the user: it goes there
        }
        f.update_remember();
    });
    let f = Rc::downgrade(&form);
    form.path.connect_icon_press(move |_, _| {
        if let Some(f) = f.upgrade() {
            form::choose_path(&f);
        }
    });
    let (c, f) = (ctx.clone(), form.clone());
    start.connect_clicked(move |_| submit(&c, &f, true));
    let (c, f) = (ctx.clone(), form.clone());
    later.connect_clicked(move |_| submit(&c, &f, false));
    let w = d.window.clone();
    cancel.connect_clicked(move |_| w.close());
    // Escape and the close button cancel too: delete what was downloaded early
    let (c, f) = (Rc::downgrade(ctx), Rc::downgrade(&form));
    d.window.connect_close_request(move |_| {
        if let (Some(c), Some(f)) = (c.upgrade(), f.upgrade()) {
            f.take_early(&c, None);
        }
        glib::Propagation::Proceed
    });
    form.update_remember();
    link::start_timer(ctx, &form);
    d.window.present();
    form.url.grab_focus();
    if req.url.is_empty() {
        paste_from_clipboard(&form);
    } else {
        link::check(ctx, &form);
    }
}

/// Like IDM: a link on the clipboard is the likely one.
fn paste_from_clipboard(form: &Rc<Form>) {
    let f = Rc::downgrade(form);
    form.window.clipboard().read_text_async(gio::Cancellable::NONE, move |text| {
        let (Some(form), Ok(Some(text))) = (f.upgrade(), text) else { return };
        let text = text.trim();
        if text.starts_with("http://") || text.starts_with("https://") || is_magnet(text) {
            form.url.set_text(text); // checked once it's in
            form.url.select_region(0, -1);
        }
    });
}

/// Something the dialog can download: a web link, a magnet link or a .torrent file.
fn is_link(url: &str) -> bool {
    url.starts_with("http") || is_torrent(url, None)
}

fn submit(ctx: &Rc<Ctx>, form: &Rc<Form>, start_now: bool) {
    let url = form.url();
    if !is_link(&url) {
        form.info.set_text("Please enter a link: http(s), a magnet link or a .torrent file");
        return;
    }
    if form.video.wanted || form.video.found.borrow().is_some() {
        video::submit(ctx, form, start_now);
        return;
    }
    let (directory, filename) = form.target(ctx);
    let Some(name) = filename else { return add_file(ctx, form, start_now, url, directory, None) };
    let (c, f, dir) = (ctx.clone(), form.clone(), directory.clone());
    exists::check(ctx, form, &directory, &name, move |name| {
        add_file(&c, &f, start_now, url.clone(), dir.clone(), Some(name));
    });
}

/// Add the file download (taking over the early one, if it's this link), to `directory` as
/// `filename` (or the name the server gives).
fn add_file(ctx: &Rc<Ctx>, form: &Rc<Form>, start_now: bool, url: String, directory: PathBuf, filename: Option<String>) {
    let connections = ctx.manager.settings().connections;
    remember_folder(ctx, form, filename.as_deref().unwrap_or(&url), &directory);
    let id = match form.take_early(ctx, Some(&url)) {
        // moved to the chosen path when confirmed, finished or not
        Some(id) => {
            ctx.manager.confirm(&id, Confirm { filename, directory, connections, start: start_now });
            id
        }
        None => ctx.manager.add(AddRequest { url, filename, directory: Some(directory), connections: Some(connections),
                                             start: start_now, ..form.req.clone() }),
    };
    done(ctx, form, &id, start_now);
}

/// "Always save … to this folder", if ticked.
fn remember_folder(ctx: &Ctx, form: &Form, name: &str, directory: &Path) {
    if form.remember.is_active() {
        let mut s = ctx.manager.settings();
        s.folders.insert(category_for(name).into(), directory.to_path_buf());
        ctx.manager.update_settings(s);
    }
}

/// Added: the dialog goes (keeping the download), the progress window comes (if it's started
/// and wanted).
fn done(ctx: &Rc<Ctx>, form: &Form, id: &str, started: bool) {
    form.early.take(); // (it's the download now: closing mustn't throw it away)
    form.window.close();
    if started && ctx.manager.settings().show_progress_window {
        progress::open(ctx, id);
    }
}
