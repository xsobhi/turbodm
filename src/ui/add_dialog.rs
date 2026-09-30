//! "Download file info" dialog: probe the link, pick name/folder/connections, start.

use super::{progress, Ctx};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use turbodm::categories::target_dir;
use turbodm::config::MAX_CONNECTIONS;
use turbodm::engine::http::{self, Headers};
use turbodm::engine::AddRequest;
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
        dir: RefCell::new(settings.download_dir.clone()),
        dir_chosen: Cell::new(false),
        name_edited: Cell::new(req.filename.is_some()),
        probe_id: Cell::new(0),
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
    let dialog = gtk::Window::builder().title("Download file info").default_width(620)
        .transient_for(&ctx.win.window).child(&body).build();
    update_folder_label(&form);

    let (f, c, r) = (form.clone(), ctx.clone(), req.clone());
    form.url.connect_changed(move |_| probe(&c, &f, &r));
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
            let id = ctx.manager.add(AddRequest {
                url,
                filename: (!name.is_empty()).then_some(name),
                directory: Some(form.dir.borrow().clone()),
                connections: Some(form.connections.value() as usize),
                start: start_now,
                ..req.clone()
            });
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
    dialog.set_default_widget(Some(&start));
    dialog.present();
    if req.url.is_empty() {
        paste_from_clipboard(&form, &dialog);
    } else {
        probe(ctx, &form, &req);
    }
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
    if !url.starts_with("http") {
        return;
    }
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
