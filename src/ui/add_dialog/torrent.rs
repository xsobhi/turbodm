//! .torrent files from this computer: picked for the add dialog's address, or added straight
//! from the menu ("Add torrent file…").

use super::Form;
use crate::ui::Ctx;
use gtk::gio;
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::engine::AddRequest;

fn torrents() -> gio::ListStore {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Torrent files"));
    filter.add_pattern("*.torrent");
    filter.add_mime_type("application/x-bittorrent");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    filters
}

/// Pick one .torrent file for the address.
pub fn pick(form: &Rc<Form>) {
    let dialog = gtk::FileDialog::builder().title("Open a torrent file").modal(true).filters(&torrents()).build();
    let f = form.clone();
    dialog.open(Some(&form.window), gio::Cancellable::NONE, move |file| {
        if let Some(path) = file.ok().and_then(|f| f.path()) {
            f.url.set_text(&path.display().to_string()); // checked once it's in
        }
    });
}

/// "Add torrent file…": pick .torrent files and add each like a link (with the "Download file
/// info" dialog, unless that's turned off).
pub fn add_torrent_file(ctx: &Rc<Ctx>) {
    let dialog = gtk::FileDialog::builder().title("Add torrent files").modal(true).filters(&torrents()).build();
    let c = ctx.clone();
    let parent = Some(&ctx.win.window).filter(|w| w.is_visible());
    dialog.open_multiple(parent, gio::Cancellable::NONE, move |files| {
        let Ok(files) = files else { return };
        for file in (0..files.n_items()).filter_map(|i| files.item(i).and_downcast::<gio::File>()) {
            if let Some(path) = file.path() {
                c.handle_download(AddRequest { url: path.display().to_string(), ..Default::default() }, false);
            }
        }
    });
}
