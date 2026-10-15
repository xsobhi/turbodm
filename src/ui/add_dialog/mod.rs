//! "Download file info", like IDM's: the link and where to save it (one path: folder and name),
//! then start. The download quietly starts on one connection while the dialog is open (out of the
//! list), so Start feels instant, and moves to the path chosen when it's confirmed, even if it
//! already finished; Cancel throws away what was downloaded. Connections: as in Preferences.

mod folder;
mod link;

use super::{progress, Ctx};
use folder::choose_path;
use link::check;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
use turbodm::categories::category_for;
use turbodm::engine::manager::Confirm;
use turbodm::engine::AddRequest;
use turbodm::text::kind_words;
use turbodm::util::filename_from_url;

struct Form {
    url: gtk::Entry,
    path: gtk::Entry, // where it's saved: folder and name
    info: gtk::Label,
    remember: gtk::CheckButton, // "Always save documents to this folder", like IDM
    chosen: Cell<bool>,  // the user typed or picked the path: don't replace it
    filling: Cell<bool>, // the path is being filled in by us, not typed
    probe_id: Cell<u64>, // ignore results of outdated probes
    early: RefCell<Option<(String, String)>>, // (url, id) of the download started early
}

impl Form {
    /// The download started early for `url`, if there is one; any other is thrown away.
    fn take_early(&self, ctx: &Ctx, url: Option<&str>) -> Option<String> {
        let (early_url, id) = self.early.take()?;
        if Some(early_url.as_str()) == url {
            return Some(id);
        }
        ctx.manager.remove(&id, true);
        None
    }

    fn url(&self) -> String {
        self.url.text().trim().to_string()
    }

    /// The path in the field as (folder, name); a bare name goes to the folder for its kind.
    fn target(&self, ctx: &Ctx) -> (PathBuf, Option<String>) {
        let path = PathBuf::from(self.path.text().trim());
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.trim().is_empty());
        let folder = match path.parent() {
            Some(dir) if dir.is_absolute() => dir.to_path_buf(),
            _ => ctx.manager.settings().folder_for(name.as_deref().unwrap_or(&filename_from_url(&self.url()))),
        };
        (folder, name)
    }

    /// Show where it goes (unless the user said where).
    fn show(&self, folder: &Path, name: &str) {
        if !self.chosen.get() {
            self.set_path(&folder.join(name));
        }
    }

    fn set_path(&self, path: &Path) {
        self.filling.set(true);
        self.path.set_text(&path.display().to_string());
        self.path.set_position(-1); // a long path shows its end, the file's name
        self.filling.set(false);
        self.update_remember();
    }

    /// "Always save documents to this folder", for this kind of file.
    fn update_remember(&self) {
        let path = PathBuf::from(self.path.text().as_str());
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.url());
        self.remember.set_label(Some(&format!("Always save {} to this folder", kind_words(&name))));
    }
}

fn row(grid: &gtk::Grid, y: i32, label: &str, widget: &impl IsA<gtk::Widget>) {
    grid.attach(&gtk::Label::builder().label(label).xalign(1.0).build(), 0, y, 1, 1);
    grid.attach(widget, 1, y, 1, 1);
}

pub fn open(ctx: &Rc<Ctx>, req: AddRequest) {
    let settings = ctx.manager.settings();
    let guess = req.filename.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| filename_from_url(&req.url));
    let folder = req.directory.clone().unwrap_or_else(|| settings.folder_for(&guess));
    let form = Rc::new(Form {
        url: gtk::Entry::builder().text(&req.url).hexpand(true).placeholder_text("https://…")
            .activates_default(true).build(), // Enter: Start download
        path: gtk::Entry::builder().hexpand(true).activates_default(true)
            .secondary_icon_name("folder-open-symbolic").secondary_icon_tooltip_text("Choose where to save it").build(),
        info: gtk::Label::builder().xalign(0.0).label("Enter a link").css_classes(["dim-label"]).build(),
        remember: gtk::CheckButton::new(),
        chosen: Cell::new(req.directory.is_some()), // e.g. picked in the browser's "Save as"
        filling: Cell::new(false),
        probe_id: Cell::new(0),
        early: RefCell::new(None),
    });
    form.set_path(&folder.join(&guess));
    let grid = gtk::Grid::builder().row_spacing(10).column_spacing(12).build();
    row(&grid, 0, "Address", &form.url);
    row(&grid, 1, "Save to", &form.path);
    row(&grid, 2, "", &form.remember);
    row(&grid, 3, "", &form.info);
    let (later, start, cancel) = (gtk::Button::with_label("Download later"),
        gtk::Button::builder().label("Start download").css_classes(["suggested-action"]).build(),
        gtk::Button::with_label("Cancel"));
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    for b in [&cancel, &later, &start] {
        buttons.append(b);
    }
    let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(16)
        .margin_top(18).margin_bottom(18).margin_start(18).margin_end(18).build();
    body.append(&grid);
    body.append(&buttons);
    let dialog = gtk::Window::builder().title("Download file info").default_width(620).child(&body).build();
    if ctx.win.window.is_visible() {
        dialog.set_transient_for(Some(&ctx.win.window));
    }

    let (f, c, r) = (form.clone(), ctx.clone(), req.clone());
    form.url.connect_changed(move |_| {
        f.update_remember();
        // wait for a pause in typing: every new address starts a download
        let id = f.probe_id.get() + 1;
        f.probe_id.set(id);
        let (f, c, r) = (f.clone(), c.clone(), r.clone());
        glib::timeout_add_local_once(Duration::from_millis(350), move || {
            if f.probe_id.get() == id {
                check(&c, &f, &r);
            }
        });
    });
    let f = form.clone();
    form.path.connect_changed(move |_| {
        if !f.filling.get() {
            f.chosen.set(true); // typed by the user: it goes there
        }
        f.update_remember();
    });
    let (f, d) = (form.clone(), dialog.clone());
    form.path.connect_icon_press(move |_, _| choose_path(&f, &d));
    let submit = |start_now: bool, form: Rc<Form>, ctx: Rc<Ctx>, dialog: gtk::Window, req: AddRequest| {
        move |_: &gtk::Button| {
            let url = form.url();
            if !url.starts_with("http") {
                form.info.set_text("Please enter an http(s) link");
                return;
            }
            if turbodm::engine::video::is_video_site(&url) {
                // a video site's page: its video, in the quality picked last (no qualities here yet)
                form.take_early(&ctx, None);
                let id = ctx.manager.add(AddRequest { url, start: start_now, video: Some(Default::default()), ..req.clone() });
                if start_now && ctx.manager.settings().show_progress_window {
                    progress::open(&ctx, &id);
                }
                dialog.close();
                return;
            }
            let (directory, filename) = form.target(&ctx);
            let connections = ctx.manager.settings().connections;
            if form.remember.is_active() {
                let mut s = ctx.manager.settings();
                s.folders.insert(category_for(filename.as_deref().unwrap_or(&url)).into(), directory.clone());
                ctx.manager.update_settings(s);
            }
            let id = match form.take_early(&ctx, Some(&url)) {
                // moved to the chosen path when confirmed, finished or not
                Some(id) => {
                    ctx.manager.confirm(&id, Confirm { filename, directory, connections, start: start_now });
                    id
                }
                None => ctx.manager.add(AddRequest { url, filename, directory: Some(directory),
                                                     connections: Some(connections), start: start_now, ..req.clone() }),
            };
            if start_now && ctx.manager.settings().show_progress_window {
                progress::open(&ctx, &id);
            }
            dialog.close();
        }
    };
    start.connect_clicked(submit(true, form.clone(), ctx.clone(), dialog.clone(), req.clone()));
    later.connect_clicked(submit(false, form.clone(), ctx.clone(), dialog.clone(), req.clone()));
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let (f, c) = (form.clone(), ctx.clone());
    dialog.connect_close_request(move |_| {
        f.take_early(&c, None); // cancelled: delete what was downloaded early
        glib::Propagation::Proceed
    });
    dialog.set_default_widget(Some(&start));
    dialog.present();
    form.url.grab_focus();
    if req.url.is_empty() {
        paste_from_clipboard(&form, &dialog);
    } else {
        check(ctx, &form, &req);
    }
}

/// Pre-fill the address from the clipboard when it holds a link (IDM behaviour).
fn paste_from_clipboard(form: &Rc<Form>, dialog: &gtk::Window) {
    let form = form.clone();
    dialog.clipboard().read_text_async(gio::Cancellable::NONE, move |text| {
        if let Ok(Some(text)) = text {
            let text = text.trim();
            if text.starts_with("http://") || text.starts_with("https://") {
                form.url.set_text(text); // triggers a probe
            }
        }
    });
}
