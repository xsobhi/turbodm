//! The dialog's controls and what's known so far, and laying them out (a video adds its quality
//! and its title).

use super::super::wnd::{self, set_text, text, Rows, Window, FIELD, LINE};
use super::super::App;
use super::{link, to_end, video};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use turbodm::engine::AddRequest;
use turbodm::text::kind_words;
use turbodm::util::name_guess;
use windows::Win32::Foundation::HWND;

pub struct Form {
    pub(super) window: Rc<Window>,
    pub(super) url: Cell<HWND>,
    pub(super) path: Cell<HWND>, // where it's saved: folder and name
    pub(super) browse: Cell<HWND>, // the folder button in the path's field
    pub(super) remember: Cell<HWND>,
    pub(super) info: Cell<HWND>,
    pub(super) names: Cell<[HWND; 3]>,   // "Address", "Save to", "Quality" (videos)
    pub(super) buttons: Cell<[HWND; 3]>, // Cancel, Download later, Start download
    pub(super) chosen: Cell<bool>,   // the user typed or picked the path: don't replace it
    pub(super) probe_id: Cell<u64>,  // results of older checks are ignored
    pub(super) early: RefCell<Option<(String, String)>>, // (url, id) of the download started early
    pub(super) probed: link::Probed, // a check's answer, from the network
    pub(super) filled: Cell<bool>,   // the early download's name and folder were shown
    pub(super) req: AddRequest,
    pub(super) video: video::State,  // the video on the page, when the address is one
}

impl Form {
    /// The download started early for `url`, if any; any other is thrown away.
    pub(super) fn take_early(&self, app: &App, url: Option<&str>) -> Option<String> {
        let (early_url, id) = self.early.take()?;
        if Some(early_url.as_str()) == url {
            return Some(id);
        }
        app.manager.remove(&id, true);
        None
    }

    pub(super) fn url(&self) -> String {
        text(self.url.get()).trim().to_string()
    }

    /// The path in the field as (folder, name); a bare name goes to the folder for its kind. A
    /// folder alone (ending in "\"): a torrent whose name isn't known yet goes in it.
    pub(super) fn target(&self, app: &App) -> (PathBuf, Option<String>) {
        let typed = text(self.path.get()).trim().to_string();
        let path = PathBuf::from(&typed);
        if typed.ends_with(['\\', '/']) && path.is_absolute() {
            return (path, None);
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.trim().is_empty());
        let folder = match path.parent() {
            Some(dir) if dir.is_absolute() => dir.to_path_buf(),
            _ => app.manager.settings().folder_for(name.as_deref().unwrap_or(&name_guess(&self.url()))),
        };
        (folder, name)
    }

    /// Show where it goes (unless the user said where).
    pub(super) fn show(&self, folder: &Path, name: &str) {
        if !self.chosen.get() {
            set_text(self.path.get(), &folder.join(name).display().to_string());
            to_end(self.path.get());
            self.update_remember();
        }
    }

    /// "Always save documents to this folder", for this kind of file.
    pub(super) fn update_remember(&self) {
        let path = PathBuf::from(text(self.path.get()));
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.url());
        set_text(self.remember.get(), &format!("Always save {} to this folder", kind_words(&name)));
    }

    /// Place everything (again: a video adds its quality and title), and fit the window to it.
    pub(super) fn layout(&self) {
        let video = self.video.found.borrow().is_some();
        let [address, save_to, quality] = self.names.get();
        wnd::set_visible(quality, video);
        wnd::set_visible(self.video.title.get(), video);
        // laid out as the Linux dialog: names on the right of their column, then the fields
        let mut rows = Rows::new(18, 18, 84, 584);
        rows.field(Some(address), self.url.get(), FIELD);
        rows.field(Some(save_to), self.path.get(), FIELD);
        if video {
            rows.field(Some(quality), self.video.quality.get(), FIELD);
        }
        rows.field(None, self.remember.get(), LINE + 6);
        if video {
            rows.field(None, self.video.title.get(), LINE);
        }
        rows.field(None, self.info.get(), LINE);
        rows.gap(6);
        let bottom = rows.buttons(&self.buttons.get());
        wnd::set_client_height(self.window.hwnd(), bottom + 18);
    }
}
