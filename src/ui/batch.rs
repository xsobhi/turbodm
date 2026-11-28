//! Adding many downloads at once, as on Windows (src/win/batch.rs) and IDM's Tasks menu: a
//! numbered pattern ("Add batch download"), the links in the clipboard or in a text file, and
//! the list written out to a text file.

use super::{frame, Ctx};
use gtk::prelude::*;
use gtk::gio;
use std::rc::Rc;
use turbodm::batch::{expand, links_in, MOST};
use turbodm::engine::AddRequest;

fn owner(ctx: &Ctx) -> Option<gtk::Window> {
    Some(ctx.win.window.clone().upcast())
}

/// Add batch download: an address with `*` where a number (or a letter) goes, from, to.
pub fn pattern(ctx: &Rc<Ctx>) {
    let d = frame::dialog_with("Add batch download", owner(ctx).as_ref(), 560, ":close", (14, 16));
    d.body.set_margin_bottom(14);
    d.body.append(&frame::note("Put * where the number goes, e.g. https://example.com/photos/img*.jpg, and say which \
                                numbers (or letters, a to z)."));
    let (grid, field) = frame::form(80);
    grid.set_margin_top(10);
    let address = frame::entry("");
    let from = frame::entry("1");
    let to = frame::entry("10");
    let digits = gtk::SpinButton::with_range(1.0, 9.0, 1.0);
    digits.set_height_request(frame::FIELD);
    field(&grid, "Address", address.upcast_ref());
    field(&grid, "From", from.upcast_ref());
    field(&grid, "To", to.upcast_ref());
    field(&grid, "Digits", digits.upcast_ref());
    d.body.append(&grid);
    let preview = frame::note("");
    preview.set_margin_top(10);
    preview.set_height_request(34);
    d.body.append(&preview);
    // what it'll add, said as it's typed
    let read = {
        let (address, from, to, digits) = (address.clone(), from.clone(), to.clone(), digits.clone());
        move || expand(&address.text(), &from.text(), &to.text(), digits.value() as usize)
    };
    let update = {
        let (read, preview) = (read.clone(), preview.clone());
        move || preview.set_text(&match read() {
            Ok(urls) => format!("{} downloads: {} … {}", urls.len(), urls[0], urls[urls.len() - 1]),
            Err(why) => why,
        })
    };
    for entry in [&address, &from, &to] {
        let update = update.clone();
        entry.connect_changed(move |_| update());
    }
    let u = update.clone();
    digits.connect_value_changed(move |_| u());
    update();
    let add = frame::default_button(&d.window, "Add");
    let cancel = frame::button("Cancel");
    let (c, w) = (ctx.clone(), d.window.clone());
    add.connect_clicked(move |_| match read() {
        Ok(urls) => {
            add_all(&c, urls, true);
            w.close();
        }
        Err(why) => frame::message(Some(w.upcast_ref()), "Add batch download", &why, true),
    });
    let w = d.window.clone();
    cancel.connect_clicked(move |_| w.close());
    frame::buttons(&d.body, &[&add, &cancel], 0);
    d.window.present();
    address.grab_focus();
}

/// Add batch download from clipboard: the links in it, to look over first.
pub fn from_clipboard(ctx: &Rc<Ctx>) {
    let c = ctx.clone();
    ctx.win.window.clipboard().read_text_async(gio::Cancellable::NONE, move |text| {
        let text = text.ok().flatten().map(|t| t.to_string()).unwrap_or_default();
        let links = links_in(&text);
        if links.is_empty() {
            return frame::message(owner(&c).as_ref(), "Add batch download from clipboard",
                                  "There are no download links in the clipboard.", false);
        }
        review(&c, "Add batch download from clipboard", links);
    });
}

/// Import: the links in a text file (one a line, as Export writes them).
pub fn import(ctx: &Rc<Ctx>) {
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    let text = gtk::FileFilter::new();
    text.set_name(Some("Text files"));
    text.add_pattern("*.txt");
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    filters.append(&text);
    filters.append(&all);
    let dialog = gtk::FileDialog::builder().title("Import downloads").modal(true).filters(&filters)
        .initial_folder(&gio::File::for_path(ctx.manager.settings().download_dir)).build();
    let c = ctx.clone();
    dialog.open(Some(&ctx.win.window), gio::Cancellable::NONE, move |file| {
        let Some(path) = file.ok().and_then(|f| f.path()) else { return };
        let title = "Import downloads";
        match std::fs::read_to_string(&path) {
            Ok(content) if !links_in(&content).is_empty() => review(&c, title, links_in(&content)),
            Ok(_) => frame::message(owner(&c).as_ref(), title, "There are no download links in that file.", false),
            Err(e) => frame::message(owner(&c).as_ref(), title, &format!("Couldn't read it: {e}"), true),
        }
    });
}

/// Export: every download's address in a text file, one a line.
pub fn export(ctx: &Rc<Ctx>) {
    let dir = ctx.manager.settings().download_dir;
    let dialog = gtk::FileDialog::builder().title("Export downloads").modal(true)
        .initial_folder(&gio::File::for_path(&dir)).initial_name("TurboDM downloads.txt").build();
    let c = ctx.clone();
    dialog.save(Some(&ctx.win.window), gio::Cancellable::NONE, move |file| {
        let Some(path) = file.ok().and_then(|f| f.path()) else { return };
        let lines: Vec<String> = c.manager.snapshot().into_iter().map(|s| s.url).collect();
        if let Err(e) = std::fs::write(&path, lines.join("\n") + "\n") {
            frame::message(owner(&c).as_ref(), "Export downloads", &format!("Couldn't write it: {e}"), true);
        }
    });
}

/// The links to add, to look over (and take some out of) first.
fn review(ctx: &Rc<Ctx>, title: &str, links: Vec<String>) {
    let d = frame::dialog_with(title, owner(ctx).as_ref(), 620, ":close", (14, 16));
    d.body.set_margin_bottom(14);
    d.body.append(&frame::note(&format!("{} link{}, one a line. Take out any you don't want.", links.len(),
                                        if links.len() == 1 { "" } else { "s" })));
    let text = gtk::TextView::builder().wrap_mode(gtk::WrapMode::None).build();
    text.buffer().set_text(&links.join("\n"));
    let scroller = gtk::ScrolledWindow::builder().child(&text).height_request(250).build();
    let framed = gtk::Box::builder().css_classes(["field-frame"]).margin_top(8).build();
    scroller.set_hexpand(true);
    framed.append(&scroller);
    d.body.append(&framed);
    let start = gtk::CheckButton::builder().label("Start downloading now").active(true).margin_top(8).build();
    d.body.append(&start);
    let add = frame::default_button(&d.window, "Add all");
    let cancel = frame::button("Cancel");
    let (c, w) = (ctx.clone(), d.window.clone());
    add.connect_clicked(move |_| {
        let buffer = text.buffer();
        let all = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        add_all(&c, links_in(&all), start.is_active());
        w.close();
    });
    let w = d.window.clone();
    cancel.connect_clicked(move |_| w.close());
    frame::buttons(&d.body, &[&add, &cancel], 6);
    d.window.present();
}

fn add_all(ctx: &Rc<Ctx>, urls: Vec<String>, start: bool) {
    for url in urls.into_iter().take(MOST) {
        ctx.manager.add(AddRequest { url, start, ..AddRequest::default() });
    }
}
