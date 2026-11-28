//! The dialog's fields and what's known so far.

use super::{link, video};
use crate::ui::Ctx;
use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use turbodm::engine::AddRequest;
use turbodm::text::kind_words;
use turbodm::util::name_guess;

pub struct Form {
    pub window: gtk::Window,
    pub title: gtk::Label,
    pub url: gtk::Entry,
    pub path: gtk::Entry, // where it's saved: folder and name
    pub remember: gtk::CheckButton, // "Always save documents to this folder", like IDM
    pub info: gtk::Label,
    pub quality_row: gtk::Box, // a video's [Quality ▾]  Format [MP4 ▾]
    pub quality_name: gtk::Label,
    pub video_title: gtk::Label,
    pub chosen: Cell<bool>,  // the user typed or picked the path: don't replace it
    pub filling: Cell<bool>, // the path is being filled in by us, not typed
    pub probe_id: Cell<u64>, // results of older checks are ignored
    pub early: RefCell<Option<(String, String)>>, // (url, id) of the download started early
    pub probed: link::Probed, // a check's answer, from the network
    pub filled: Cell<bool>,   // the early download's name and folder were shown
    pub req: AddRequest,
    pub video: video::State, // the video on the page, when the address is one
}

impl Form {
    /// The download started early for `url`, if any; any other is thrown away.
    pub fn take_early(&self, ctx: &Ctx, url: Option<&str>) -> Option<String> {
        let (early_url, id) = self.early.take()?;
        if Some(early_url.as_str()) == url {
            return Some(id);
        }
        ctx.manager.remove(&id, true);
        None
    }

    pub fn url(&self) -> String {
        self.url.text().trim().to_string()
    }

    /// The path in the field as (folder, name); a bare name goes to the folder for its kind. A
    /// folder alone (ending in "/"): a torrent whose name isn't known yet goes in it.
    pub fn target(&self, ctx: &Ctx) -> (PathBuf, Option<String>) {
        let typed = self.path.text().trim().to_string();
        let path = PathBuf::from(&typed);
        if typed.ends_with('/') && path.is_absolute() {
            return (path, None);
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.trim().is_empty());
        let folder = match path.parent() {
            Some(dir) if dir.is_absolute() => dir.to_path_buf(),
            _ => ctx.manager.settings().folder_for(name.as_deref().unwrap_or(&name_guess(&self.url()))),
        };
        (folder, name)
    }

    /// Show where it goes (unless the user said where).
    pub fn show(&self, folder: &Path, name: &str) {
        if !self.chosen.get() {
            self.set_path(&folder.join(name));
        }
    }

    pub fn set_path(&self, path: &Path) {
        self.filling.set(true);
        self.path.set_text(&path.display().to_string());
        self.path.set_position(-1); // a long path shows its end, the file's name
        self.filling.set(false);
        self.update_remember();
    }

    /// "Always save documents to this folder", for this kind of file.
    pub fn update_remember(&self) {
        let path = PathBuf::from(self.path.text().as_str());
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.url());
        self.remember.set_label(Some(&format!("Always save {} to this folder", kind_words(&name))));
    }
}

/// Choosing where the file goes: the save dialog, opened at the path in the field.
pub fn choose_path(form: &Rc<Form>) {
    let now = PathBuf::from(form.path.text().trim());
    let mut dialog = gtk::FileDialog::builder().title("Save as").modal(true);
    // the folder (or the nearest one above it that exists yet), and the name
    if let Some(folder) = now.parent().and_then(|p| p.ancestors().find(|a| a.is_dir())) {
        dialog = dialog.initial_folder(&gio::File::for_path(folder));
    }
    if let Some(name) = now.file_name() {
        dialog = dialog.initial_name(name.to_string_lossy().as_ref());
    }
    let f = form.clone();
    dialog.build().save(Some(&form.window), gio::Cancellable::NONE, move |result| {
        if let Some(path) = result.ok().and_then(|f| f.path()) {
            f.set_path(&path);
            f.chosen.set(true);
        }
    });
}
