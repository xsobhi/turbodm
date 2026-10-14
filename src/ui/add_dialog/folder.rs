//! Choosing where the file goes: the system's save dialog, opened at the path in the field.

use super::Form;
use gtk::gio;
use gtk::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;

pub fn choose_path(form: &Rc<Form>, parent: &gtk::Window) {
    let now = PathBuf::from(form.path.text().trim());
    let mut dialog = gtk::FileDialog::builder().title("Save as").modal(true);
    // the folder (or the nearest one above it that exists yet), and the name
    if let Some(folder) = now.parent().and_then(|p| p.ancestors().find(|a| a.is_dir())) {
        dialog = dialog.initial_folder(&gio::File::for_path(folder));
    }
    if let Some(name) = now.file_name() {
        dialog = dialog.initial_name(name.to_string_lossy().as_ref());
    }
    let form = form.clone();
    dialog.build().save(Some(parent), gio::Cancellable::NONE, move |result| {
        if let Some(path) = result.ok().and_then(|f| f.path()) {
            form.set_path(&path);
            form.chosen.set(true);
        }
    });
}
