//! "Download file info" dialog: probe the link, pick name/folder/connections, start. Like IDM,
//! the download quietly starts on one connection while the dialog is open (hidden from the
//! list), so pressing Start feels instant; Cancel throws away what was downloaded.

mod folder;
mod link;

use super::{progress, Ctx};
use folder::{choose_folder, kind_of, update_folder_label, update_remember_label};
use link::check;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use turbodm::config::MAX_CONNECTIONS;
use turbodm::engine::manager::Confirm;
use turbodm::engine::AddRequest;
use turbodm::util::filename_from_url;

struct Form {
    url: gtk::Entry,
    name: gtk::Entry,
    folder: gtk::Button,
    info: gtk::Label,
    connections: gtk::SpinButton,
    remember: gtk::CheckButton, // "Always save documents to this folder", like IDM
    dir: RefCell<PathBuf>,
    dir_chosen: Cell<bool>,  // user picked a folder: stop auto-categorizing
    name_edited: Cell<bool>, // user typed a name: don't overwrite it
    probe_id: Cell<u64>,     // ignore results of outdated probes
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
}

fn row(grid: &gtk::Grid, y: i32, label: &str, widget: &impl IsA<gtk::Widget>) {
    grid.attach(&gtk::Label::builder().label(label).xalign(1.0).build(), 0, y, 1, 1);
    grid.attach(widget, 1, y, 1, 1);
}

pub fn open(ctx: &Rc<Ctx>, req: AddRequest) {
    let settings = ctx.manager.settings();
    let form = Rc::new(Form {
        url: gtk::Entry::builder().text(&req.url).hexpand(true).placeholder_text("https://…")
            .activates_default(true).build(), // Enter: Start download
        name: gtk::Entry::builder().text(req.filename.clone().unwrap_or_default()).activates_default(true).build(),
        folder: gtk::Button::new(),
        info: gtk::Label::builder().xalign(0.0).label("Enter a link").build(),
        connections: gtk::SpinButton::with_range(1.0, MAX_CONNECTIONS as f64, 1.0),
        remember: gtk::CheckButton::new(),
        dir: RefCell::new(req.directory.clone().unwrap_or_else(|| {
            settings.folder_for(&req.filename.clone().unwrap_or_else(|| filename_from_url(&req.url)))
        })),
        dir_chosen: Cell::new(req.directory.is_some()), // e.g. picked in the browser's "Save As"
        name_edited: Cell::new(req.filename.is_some()),
        probe_id: Cell::new(0),
        early: RefCell::new(None),
    });
    form.connections.set_value(settings.connections as f64);
    let grid = gtk::Grid::builder().row_spacing(10).column_spacing(12).build();
    row(&grid, 0, "Address", &form.url);
    row(&grid, 1, "Save as", &form.name);
    row(&grid, 2, "Folder", &form.folder);
    row(&grid, 3, "", &form.remember);
    row(&grid, 4, "Connections", &form.connections);
    row(&grid, 5, "", &form.info);
    update_remember_label(&form);
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
    update_folder_label(&form);

    let (f, c, r) = (form.clone(), ctx.clone(), req.clone());
    form.url.connect_changed(move |_| {
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
    form.name.connect_changed(move |e| {
        f.name_edited.set(e.has_focus());
        update_remember_label(&f);
    });
    let (f, d) = (form.clone(), dialog.clone());
    form.folder.connect_clicked(move |_| choose_folder(&f, &d));
    let submit = |start_now: bool, form: Rc<Form>, ctx: Rc<Ctx>, dialog: gtk::Window, req: AddRequest| {
        move |_: &gtk::Button| {
            let url = form.url.text().trim().to_string();
            if !url.starts_with("http") {
                form.info.set_text("Please enter an http(s) link");
                return;
            }
            let name = form.name.text().trim().to_string();
            let filename = (!name.is_empty()).then_some(name);
            let (directory, connections) = (form.dir.borrow().clone(), form.connections.value() as usize);
            if form.remember.is_active() {
                let mut s = ctx.manager.settings();
                s.folders.insert(kind_of(&form).into(), directory.clone());
                ctx.manager.update_settings(s);
            }
            let id = match form.take_early(&ctx, Some(&url)) {
                Some(id) => {
                    ctx.manager.confirm(&id, Confirm { filename, directory, connections, start: start_now });
                    id
                }
                None => ctx.manager.add(AddRequest {
                    url,
                    filename,
                    directory: Some(directory),
                    connections: Some(connections),
                    start: start_now,
                    ..req.clone()
                }),
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
