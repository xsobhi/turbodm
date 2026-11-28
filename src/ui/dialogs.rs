//! Small dialogs, as on Windows (src/win/dialogs.rs): Delete, Redownload, File missing,
//! "Refresh download address" (a link pasted), About.

use super::frame::{self, LINE_GAP};
use super::Ctx;
use gtk::prelude::*;
use std::rc::Rc;

fn owner(ctx: &Ctx) -> Option<gtk::Window> {
    Some(ctx.win.window.clone().upcast())
}

/// Delete (the key, the toolbar, the menus), as IDM asks: off the list, and with "Also delete
/// the files from the disk" ticked, their files too. The box is as it was answered last, or
/// ticked for "Delete with file".
pub fn confirm_delete(ctx: &Rc<Ctx>, ids: Vec<String>, with_files: bool) {
    if ids.is_empty() {
        return;
    }
    let names: Vec<String> = ids.iter().filter_map(|id| ctx.manager.get(id)).map(|s| s.filename).collect();
    let heading = match ids.len() {
        1 => "Delete this download?".to_string(),
        n => format!("Delete these {n} downloads?"),
    };
    let mut listed = names.iter().take(3).map(|n| format!("\u{201c}{n}\u{201d}")).collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        listed.push_str(&format!(" and {} more", names.len() - 3));
    }
    let d = frame::dialog_with("Delete", owner(ctx).as_ref(), 440, ":close", (16, 18));
    d.body.append(&frame::heading(&heading));
    let note = frame::note(&format!("{listed} will be taken off the list."));
    note.set_margin_top(LINE_GAP);
    d.body.append(&note);
    let also = gtk::CheckButton::builder().label("Also delete the files from the disk")
        .active(with_files || ctx.manager.settings().delete_files_too).margin_top(10).build();
    d.body.append(&also);
    let cancel = frame::button("Cancel");
    let delete = frame::default_button(&d.window, "Delete");
    let w = d.window.clone();
    cancel.connect_clicked(move |_| w.close());
    let (c, w) = (ctx.clone(), d.window.clone());
    delete.connect_clicked(move |_| {
        let files = also.is_active();
        ids.iter().for_each(|id| c.manager.remove(id, files));
        if !with_files {
            // (remembered for next time; "Delete with file" doesn't change it)
            let mut s = c.manager.settings();
            if s.delete_files_too != files {
                s.delete_files_too = files;
                c.manager.update_settings(s);
            }
        }
        w.close();
    });
    frame::buttons(&d.body, &[&cancel, &delete], 14);
    d.window.present();
}

/// IDM's Redownload: the file deleted, and downloaded again from the start.
pub fn confirm_redownload(ctx: &Rc<Ctx>, ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    let what = match ids.len() {
        1 => ctx.manager.get(&ids[0]).map_or("it".into(), |s| format!("\u{201c}{}\u{201d}", s.filename)),
        n => format!("{n} downloads"),
    };
    let body = format!("Download {what} again from the start?\n\nWhat's downloaded so far is deleted.");
    let c = ctx.clone();
    frame::ask(owner(ctx).as_ref(), "Redownload", &body, move || {
        ids.iter().for_each(|id| c.manager.redownload(id));
    });
}

/// A finished download's file isn't there any more (moved or deleted outside TurboDM): download
/// it again, or take it off the list.
pub fn missing_file(ctx: &Rc<Ctx>, id: &str) {
    let Some(snap) = ctx.manager.get(id) else { return };
    let d = frame::dialog("File missing", owner(ctx).as_ref(), 460);
    d.body.set_margin_top(14);
    d.body.set_margin_bottom(14);
    d.body.append(&frame::heading("The file isn't there any more"));
    let note = frame::note(&format!("\u{201c}{}\u{201d} was moved or deleted outside TurboDM. It was in {}.",
                                    snap.filename, snap.directory.display()));
    note.set_margin_top(LINE_GAP);
    d.body.append(&note);
    let cancel = frame::button("Cancel");
    let remove = frame::button("Remove from list");
    let again = frame::default_button(&d.window, "Download again");
    let w = d.window.clone();
    cancel.connect_clicked(move |_| w.close());
    let (c, w, i) = (ctx.clone(), d.window.clone(), id.to_string());
    remove.connect_clicked(move |_| {
        c.manager.remove(&i, false);
        w.close();
    });
    let (c, w, i) = (ctx.clone(), d.window.clone(), id.to_string());
    again.connect_clicked(move |_| {
        c.manager.redownload(&i);
        w.close();
    });
    frame::buttons(&d.body, &[&cancel, &remove, &again], 14);
    d.window.present();
}

/// IDM's "Refresh download address": paste a fresh link for an expired one.
pub fn refresh_address(ctx: &Rc<Ctx>, id: &str) {
    let Some(snap) = ctx.manager.get(id) else { return };
    let d = frame::dialog_with("Refresh download address", owner(ctx).as_ref(), 520, ":close", (14, 14));
    d.body.set_margin_bottom(14);
    d.body.append(&frame::note(&format!(
        "New link for \u{201c}{}\u{201d}. Progress is kept if the server returns the same file.", snap.filename)));
    let field = frame::entry(&snap.url);
    field.set_margin_top(8);
    d.body.append(&field);
    let ok = frame::default_button(&d.window, "Refresh");
    let cancel = frame::button("Cancel");
    let (c, w, id) = (ctx.clone(), d.window.clone(), id.to_string());
    let f = field.clone();
    ok.connect_clicked(move |_| {
        let url = f.text().trim().to_string();
        if url.starts_with("http") {
            c.manager.refresh_address(&id, url);
        }
        w.close();
    });
    let w = d.window.clone();
    cancel.connect_clicked(move |_| w.close());
    frame::buttons(&d.body, &[&ok, &cancel], 12);
    d.window.present();
    field.grab_focus();
}

pub fn about(ctx: &Rc<Ctx>) {
    let name = turbodm::config::APP_NAME;
    let body = format!("{name} (TurboDM) {}\n\nFast multi-connection download manager, with browser integration.\n\n\
                        https://github.com/xsobhi/turbodm\nMIT license.", turbodm::config::VERSION);
    frame::message(owner(ctx).as_ref(), &format!("About {name}"), &body, false);
}
