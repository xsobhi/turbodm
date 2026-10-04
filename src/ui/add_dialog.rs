//! "Download file info" dialog: probe the link, pick name/folder/connections, start. Like IDM,
//! the download quietly starts on one connection while the dialog is open (hidden from the
//! list), so pressing Start feels instant; Cancel throws away what was downloaded.

use super::{center, progress, Ctx};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use turbodm::categories::target_dir;
use turbodm::config::MAX_CONNECTIONS;
use turbodm::engine::http::{self, Headers};
use turbodm::engine::manager::Confirm;
use turbodm::engine::{AddRequest, Status};
use turbodm::util::human_size;

struct Form {
    url: gtk::Entry,
    name: gtk::Entry,
    folder: gtk::Button,
    info: gtk::Label,
    connections: gtk::SpinButton,
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
        url: gtk::Entry::builder().text(&req.url).hexpand(true).placeholder_text("https://…").build(),
        name: gtk::Entry::builder().text(req.filename.clone().unwrap_or_default()).build(),
        folder: gtk::Button::new(),
        info: gtk::Label::builder().xalign(0.0).label("Enter a link").build(),
        connections: gtk::SpinButton::with_range(1.0, MAX_CONNECTIONS as f64, 1.0),
        dir: RefCell::new(req.directory.clone().unwrap_or_else(|| settings.download_dir.clone())),
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
    row(&grid, 3, "Connections", &form.connections);
    row(&grid, 4, "", &form.info);
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
        dialog.set_transient_for(Some(&ctx.win.window)); // centred over the main window
    } else {
        center::on_screen(&dialog, false); // started by the browser: centred on screen
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
    if req.url.is_empty() {
        paste_from_clipboard(&form, &dialog);
    } else {
        check(ctx, &form, &req);
    }
}

/// A new address: start downloading it right away, or just ask the server about it.
fn check(ctx: &Rc<Ctx>, form: &Rc<Form>, req: &AddRequest) {
    let url = form.url.text().trim().to_string();
    form.take_early(ctx, None);
    if !url.starts_with("http") {
        return;
    }
    if !ctx.manager.settings().predownload {
        return probe(ctx, form, req);
    }
    let id = ctx.manager.prefetch(AddRequest {
        url: url.clone(),
        filename: form.name_edited.get().then(|| form.name.text().trim().to_string()).filter(|n| !n.is_empty()),
        directory: form.dir_chosen.get().then(|| form.dir.borrow().clone()),
        connections: Some(form.connections.value() as usize),
        ..req.clone()
    });
    *form.early.borrow_mut() = Some((url, id.clone()));
    form.info.set_text("Checking link…"); // it's downloading, quietly: nothing to show yet
    let (ctx, form, mut filled) = (ctx.clone(), form.clone(), false);
    glib::timeout_add_local(Duration::from_millis(150), move || {
        if form.early.borrow().as_ref().is_none_or(|(_, early)| *early != id) {
            return glib::ControlFlow::Break; // started, cancelled, or another address
        }
        let Some(s) = ctx.manager.get(&id) else { return glib::ControlFlow::Break };
        let text = match s.status {
            Status::Connecting | Status::Queued => "Checking link…".to_string(),
            Status::Error => format!("Couldn't check the link: {}", s.error.as_deref().unwrap_or("unknown error")),
            _ => {
                if !filled {
                    filled = true;
                    if !form.name_edited.get() {
                        form.name.set_text(&s.filename);
                    }
                    if !form.dir_chosen.get() {
                        *form.dir.borrow_mut() = s.directory.clone();
                        update_folder_label(&form);
                    }
                }
                let resume = if s.resumable { "yes" } else { "no (single connection)" };
                format!("Size: {}   ·   Resume support: {resume}", human_size(s.size))
            }
        };
        if form.info.text() != text {
            form.info.set_text(&text);
        }
        glib::ControlFlow::Continue
    });
}

fn update_folder_label(form: &Form) {
    form.folder.set_label(&form.dir.borrow().display().to_string());
}

fn choose_folder(form: &Rc<Form>, parent: &gtk::Window) {
    let dialog = gtk::FileDialog::builder().title("Save to folder").modal(true)
        .initial_folder(&gio::File::for_path(&*form.dir.borrow())).build();
    let form = form.clone();
    dialog.select_folder(Some(parent), gio::Cancellable::NONE, move |result| {
        if let Some(path) = result.ok().and_then(|f| f.path()) {
            *form.dir.borrow_mut() = path;
            form.dir_chosen.set(true);
            update_folder_label(&form);
        }
    });
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

/// Ask the server for name, size and resume support (runs on the tokio runtime).
fn probe(ctx: &Rc<Ctx>, form: &Rc<Form>, req: &AddRequest) {
    let url = form.url.text().trim().to_string();
    let id = form.probe_id.get() + 1;
    form.probe_id.set(id);
    form.info.set_text("Checking link…");
    let headers = Headers { user_agent: req.user_agent.clone(), referrer: req.referrer.clone(),
                            cookies: req.cookies.clone() };
    let client = ctx.manager.shared.client();
    let job = ctx.rt.spawn(async move { http::probe(&client, &url, &headers).await });
    let (form, settings) = (form.clone(), ctx.manager.settings());
    glib::spawn_future_local(async move {
        let result = job.await;
        if form.probe_id.get() != id {
            return; // the address changed meanwhile
        }
        match result {
            Ok(Ok(info)) => {
                let resume = if info.resumable { "yes" } else { "no (single connection)" };
                form.info.set_text(&format!("Size: {}   ·   Resume support: {resume}", human_size(info.size)));
                if !form.name_edited.get() {
                    form.name.set_text(&info.filename);
                }
                if !form.dir_chosen.get() {
                    let name = form.name.text();
                    *form.dir.borrow_mut() = target_dir(&settings.download_dir, &name, settings.use_categories);
                    update_folder_label(&form);
                }
            }
            Ok(Err(err)) => form.info.set_text(&format!("Couldn't check the link: {err}")),
            Err(_) => form.info.set_text("Couldn't check the link"),
        }
    });
}
