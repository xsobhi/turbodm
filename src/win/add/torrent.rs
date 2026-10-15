//! .torrent files from this computer: picked for the add dialog's address, or added straight
//! from the menu ("Add torrent file…").

use super::super::{shell, App};
use std::path::PathBuf;
use std::rc::Rc;
use turbodm::engine::AddRequest;
use windows::Win32::Foundation::HWND;

const TORRENTS: (&str, &str) = ("Torrent files", "*.torrent");

/// Pick one .torrent file.
pub fn pick(owner: HWND) -> Option<PathBuf> {
    shell::choose_files(owner, "Open a torrent file", TORRENTS, false).into_iter().next()
}

/// "Add torrent file…": pick .torrent files and add each like a link (with the "Download file
/// info" dialog, unless that's turned off).
#[allow(dead_code)] // for the main window's menu
pub fn add_torrent_file(app: &Rc<App>) {
    let owner = if app.main.is_visible() { app.main.window.hwnd() } else { HWND::default() };
    for file in shell::choose_files(owner, "Add torrent files", TORRENTS, true) {
        app.handle_download(AddRequest { url: file.display().to_string(), ..Default::default() }, false);
    }
}
