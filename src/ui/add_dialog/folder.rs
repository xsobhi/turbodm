//! The dialog's folder: choosing it, and remembering it for that kind of file.

use super::Form;
use gtk::gio;
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::categories::category_for;

/// The file's category, from its name (or the address while the name isn't known).
pub fn kind_of(form: &Form) -> &'static str {
    let name = form.name.text();
    category_for(if name.is_empty() { form.url.text() } else { name }.as_str())
}

pub fn update_remember_label(form: &Form) {
    let kind = match kind_of(form) {
        "Video" => "videos",
        "Music" => "music",
        "Documents" => "documents",
        "Compressed" => "archives",
        "Programs" => "programs",
        "Images" => "images",
        _ => "other files",
    };
    form.remember.set_label(Some(&format!("Always save {kind} to this folder")));
}

pub fn update_folder_label(form: &Form) {
    form.folder.set_label(&form.dir.borrow().display().to_string());
}

pub fn choose_folder(form: &Rc<Form>, parent: &gtk::Window) {
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
