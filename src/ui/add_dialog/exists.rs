//! "A file of that name is already there", as on Windows: Start, with the name of a file that's
//! in the folder already, asks whether to replace it (the old one goes to the Trash) or keep both
//! (the new one gets a number: "name (1).ext"), as IDM asks. Unless the file is this download's
//! own, started early while the dialog was open.

use super::Form;
use crate::ui::{frame, Ctx};
use gtk::gio;
use gtk::prelude::*;
use std::path::Path;
use std::rc::Rc;
use turbodm::util::{unique_path, unnumbered};

/// Is there another file named `name` in `dir`, not the early download's? Then ask; `go` gets
/// the name to download as, once answered (not if cancelled).
pub fn check(ctx: &Ctx, form: &Rc<Form>, dir: &Path, name: &str, go: impl Fn(String) + 'static) {
    let path = dir.join(name);
    let early = form.early.borrow().as_ref().and_then(|(_, id)| ctx.manager.get(id));
    if !path.is_file() || early.as_ref().is_some_and(|s| s.path == path) {
        return go(name.to_string());
    }
    // keeping both: the early download's own numbered name, when it's this one's; else the next
    let numbered = early.filter(|s| s.directory == dir && unnumbered(dir, &s.filename).as_deref() == Some(name))
        .map(|s| s.filename)
        .unwrap_or_else(|| unique_path(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    ask(form, &path, name.to_string(), numbered, go);
}

fn ask(form: &Rc<Form>, path: &Path, name: String, numbered: String, go: impl Fn(String) + 'static) {
    let d = frame::dialog_with("File already exists", Some(&form.window), 460, ":close", (14, 16));
    d.window.set_modal(true); // the dialog behind waits for the answer
    d.body.set_margin_bottom(14);
    d.body.append(&frame::heading("A file with this name is already there"));
    let note = frame::note(&format!(
        "\u{201c}{name}\u{201d} is in this folder already. Replace it (the old one goes to the Trash), or keep \
         both: this one is then saved as \u{201c}{numbered}\u{201d}."));
    note.set_margin_top(frame::LINE_GAP);
    d.body.append(&note);
    let cancel = frame::button("Cancel");
    let replace = frame::button("Replace");
    let both = frame::default_button(&d.window, "Keep both");
    let go = Rc::new(go);
    let (w, g, path) = (d.window.clone(), go.clone(), path.to_path_buf());
    replace.connect_clicked(move |_| {
        if gio::File::for_path(&path).trash(gio::Cancellable::NONE).is_err() {
            frame::message(Some(&w), "Couldn't replace it",
                           "The file couldn't be moved to the Trash: it may be open in another program.", true);
            return;
        }
        w.close();
        g(name.clone());
    });
    let (w, g) = (d.window.clone(), go.clone());
    both.connect_clicked(move |_| {
        w.close();
        g(numbered.clone());
    });
    let w = d.window.clone();
    cancel.connect_clicked(move |_| w.close());
    frame::buttons(&d.body, &[&cancel, &replace, &both], 12);
    d.window.present();
}
