//! "A file of that name is already there": Start, with the name of a file that's in the folder
//! already, asks whether to replace it (the old one goes to the Recycle Bin) or keep both (the
//! new one gets a number: "name (1).ext"), as IDM asks. Unless the file is this download's own,
//! started early while the dialog was open.

use super::super::wnd::{self, Rows, Window, LINE};
use super::super::{dialogs, shell, App};
use super::Form;
use std::path::Path;
use std::rc::Rc;
use turbodm::util::{unique_path, unnumbered};
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

/// Is there another file named `name` in `dir`, not the early download's? Then ask; `go` gets
/// the name to download as, once answered (not if cancelled).
pub fn check(app: &App, form: &Rc<Form>, dir: &Path, name: &str, go: impl Fn(String) + 'static) {
    let path = dir.join(name);
    let early = form.early.borrow().as_ref().and_then(|(_, id)| app.manager.get(id));
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
    let owner = form.window.hwnd();
    let window = Window::dialog("File already exists", Some(owner), (460, 150), false);
    let heading = window.heading("A file with this name is already there");
    let note = window.note(&format!(
        "\u{201c}{name}\u{201d} is in this folder already. Replace it (the old one goes to the Recycle Bin), or keep \
         both: this one is then saved as \u{201c}{numbered}\u{201d}."));
    let go = Rc::new(go);
    let (w, g, path) = (window.clone(), go.clone(), path.to_path_buf());
    let replace = window.button("Replace", move || {
        if !shell::recycle(&path) {
            dialogs::message(w.hwnd(), "Couldn't replace it",
                             "The file couldn't be moved to the Recycle Bin: it may be open in another program.", true);
            return;
        }
        w.close();
        g(name.clone());
    });
    let (w, g) = (window.clone(), go.clone());
    let both = window.default_button("Keep both", move || {
        w.close();
        g(numbered.clone());
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.close());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.close());
    // the dialog behind waits for the answer
    wnd::enable(owner, false);
    window.on_close(move || {
        wnd::enable(owner, true);
        true
    });
    let mut rows = Rows::new(16, 14, 0, 428);
    rows.full(heading, 24);
    rows.full(note, 3 * LINE);
    rows.gap(4);
    let bottom = rows.buttons(&[cancel, replace, both]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}
