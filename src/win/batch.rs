//! Adding many downloads at once, as IDM's Tasks menu does: a numbered pattern ("Add batch
//! download": file*.zip from 1 to 50), the links in the clipboard or in a text file, and the
//! list written out to a text file.

use super::wnd::{self, field, text, Rows, Window, FIELD};
use super::{dialogs, shell, App};
use std::rc::Rc;
use turbodm::engine::AddRequest;
use windows::Win32::UI::Controls::WC_EDITW;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The most one batch adds (a typo in "to" shouldn't add a million downloads).
const MOST: usize = 10_000;

/// Add batch download: an address with `*` where a number (or a letter) goes, from, to.
pub fn pattern(app: &Rc<App>) {
    let window = Window::dialog("Add batch download", Some(app.main.window.hwnd()), (560, 260), false);
    let hint = window.note("Put * where the number goes, e.g. https://example.com/photos/img*.jpg, and say which \
                            numbers (or letters, a to z).");
    let preview = window.note("");
    let address = window.edit("", || {});
    let from = window.edit("1", || {});
    let to = window.edit("10", || {});
    let digits = window.spin(1, 9, 1, |_| {});
    let names = [window.name("Address"), window.name("From"), window.name("To"), window.name("Digits")];
    // what it'll add, said as it's typed
    let update = move || {
        let said = match expand(&text(address), &text(from), &text(to), text(digits).parse().unwrap_or(1)) {
            Ok(urls) => format!("{} downloads: {} … {}", urls.len(), urls[0], urls[urls.len() - 1]),
            Err(why) => why,
        };
        wnd::set_text(preview, &said);
    };
    for control in [address, from, to, digits] {
        window.on_command(control_id(control), move |code| if code == EN_CHANGE_CODE { update() });
    }
    update();
    let (a, w) = (app.clone(), window.clone());
    let add = window.default_button("Add", move || {
        match expand(&text(address), &text(from), &text(to), text(digits).parse().unwrap_or(1)) {
            Ok(urls) => {
                add_all(&a, urls, true);
                w.destroy();
            }
            Err(why) => dialogs::message(w.hwnd(), "Add batch download", &why, true),
        }
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let mut rows = Rows::new(16, 14, 80, 528);
    rows.full(hint, 34);
    rows.field(Some(names[0]), address, FIELD);
    rows.field(Some(names[1]), from, FIELD);
    rows.field(Some(names[2]), to, FIELD);
    rows.field(Some(names[3]), digits, FIELD);
    rows.full(preview, 34);
    let bottom = rows.buttons(&[add, cancel]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}

/// The control's id, as its WM_COMMAND says it.
fn control_id(hwnd: windows::Win32::Foundation::HWND) -> u16 {
    // SAFETY: plain query on our control
    (unsafe { GetDlgCtrlID(hwnd) }) as u16
}

const EN_CHANGE_CODE: u32 = 0x0300; // EN_CHANGE

/// The addresses a pattern stands for (or why it doesn't make sense yet).
pub fn expand(pattern: &str, from: &str, to: &str, digits: usize) -> Result<Vec<String>, String> {
    let pattern = pattern.trim();
    if !pattern.contains('*') {
        return Err("The address needs a * where the number goes".into());
    }
    let (from, to) = (from.trim(), to.trim());
    let items: Vec<String> = match (from.parse::<u64>(), to.parse::<u64>()) {
        (Ok(a), Ok(b)) if a <= b && (b - a) < MOST as u64 => (a..=b).map(|n| format!("{n:0digits$}")).collect(),
        (Ok(a), Ok(b)) if a <= b => return Err(format!("That's more than {MOST} downloads")),
        (Ok(_), Ok(_)) => return Err("\"From\" must not be bigger than \"To\"".into()),
        _ => {
            let letter = |s: &str| (s.len() == 1 && s.chars().all(|c| c.is_ascii_alphabetic())).then(|| s.as_bytes()[0]);
            match (letter(from), letter(to)) {
                (Some(a), Some(b)) if a <= b && a.is_ascii_lowercase() == b.is_ascii_lowercase() => {
                    (a..=b).map(|c| (c as char).to_string()).collect()
                }
                _ => return Err("\"From\" and \"To\" are numbers (1 to 50) or letters (a to z)".into()),
            }
        }
    };
    Ok(items.iter().map(|item| pattern.replacen('*', item, 1)).collect())
}

/// Add batch download from clipboard: the links in it, to look over first.
pub fn from_clipboard(app: &Rc<App>) {
    let links = links_in(&shell::clipboard_text().unwrap_or_default());
    if links.is_empty() {
        return dialogs::message(app.main.window.hwnd(), "Add batch download from clipboard",
                                "There are no download links in the clipboard.", false);
    }
    review(app, "Add batch download from clipboard", links);
}

/// Import: the links in a text file (one a line, as Export writes them).
pub fn import(app: &Rc<App>) {
    let start = app.manager.settings().download_dir;
    let Some(path) = shell::choose_open(app.main.window.hwnd(), "Import downloads", &start, ("Text files", "*.txt")) else { return };
    match std::fs::read_to_string(&path) {
        Ok(content) if !links_in(&content).is_empty() => review(app, "Import downloads", links_in(&content)),
        Ok(_) => dialogs::message(app.main.window.hwnd(), "Import downloads", "There are no download links in that file.", false),
        Err(e) => dialogs::message(app.main.window.hwnd(), "Import downloads", &format!("Couldn't read it: {e}"), true),
    }
}

/// Export: every download's address in a text file, one a line.
pub fn export(app: &Rc<App>) {
    let initial = app.manager.settings().download_dir.join("TurboDM downloads.txt");
    let Some(path) = shell::choose_save(app.main.window.hwnd(), "Export downloads", &initial) else { return };
    let lines: Vec<String> = app.manager.snapshot().into_iter().map(|s| s.url).collect();
    if let Err(e) = std::fs::write(&path, lines.join("\r\n") + "\r\n") {
        dialogs::message(app.main.window.hwnd(), "Export downloads", &format!("Couldn't write it: {e}"), true);
    }
}

/// The download links in some text: http(s), ftp and magnet links, once each, in order.
pub fn links_in(text: &str) -> Vec<String> {
    let mut found: Vec<String> = vec![];
    for word in text.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>') {
        let word = word.trim_start_matches(['(', '[', '{']).trim_end_matches([',', '.', ';', ')', ']', '}']);
        let link = ["http://", "https://", "ftp://", "magnet:?"].iter().any(|p| word.to_ascii_lowercase().starts_with(p));
        if link && word.len() > 10 && !found.iter().any(|f| f == word) {
            found.push(word.to_string());
        }
    }
    found
}

/// The links to add, to look over (and take some out of) first.
fn review(app: &Rc<App>, title: &str, links: Vec<String>) {
    let window = Window::dialog(title, Some(app.main.window.hwnd()), (620, 380), false);
    let hint = window.note(&format!("{} link{}, one a line. Take out any you don't want.", links.len(),
                                    if links.len() == 1 { "" } else { "s" }));
    let style = WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32);
    let (list, _) = window.control(WC_EDITW, &links.join("\r\n"), style, WINDOW_EX_STYLE(0));
    field::make_field(list, vec![]);
    wnd::scrollbar::slim(list);
    let start = window.checkbox("Start downloading now", true, |_| {});
    let (a, w) = (app.clone(), window.clone());
    let add = window.default_button("Add all", move || {
        add_all(&a, links_in(&text(list)), wnd::is_checked(start));
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let mut rows = Rows::new(16, 14, 0, 588);
    rows.full(hint, 22);
    rows.full(list, 250);
    rows.full(start, 24);
    let bottom = rows.buttons(&[add, cancel]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}

fn add_all(app: &Rc<App>, urls: Vec<String>, start: bool) {
    for url in urls.into_iter().take(MOST) {
        app.manager.add(AddRequest { url, start, ..AddRequest::default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        let urls = expand("https://x.org/img*.jpg", "8", "10", 2).unwrap();
        assert_eq!(urls, ["https://x.org/img08.jpg", "https://x.org/img09.jpg", "https://x.org/img10.jpg"]);
        assert_eq!(expand("https://x.org/*.zip", "a", "c", 1).unwrap().len(), 3);
        assert!(expand("https://x.org/a.zip", "1", "2", 1).is_err());
        assert!(expand("https://x.org/*.zip", "5", "1", 1).is_err());
    }

    #[test]
    fn links() {
        let text = "see https://a.org/f.zip, and (https://b.org/g.iso) twice https://a.org/f.zip magnet:?xt=urn:btih:abc";
        assert_eq!(links_in(text), ["https://a.org/f.zip", "https://b.org/g.iso", "magnet:?xt=urn:btih:abc"]);
    }
}
