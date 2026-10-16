//! IDM's "Refresh download address": the page a download came from opens in the browser, and the
//! next download caught from there (its link clicked again) gives the waiting download a fresh
//! address, cookies and referrer; it goes on from where it was. Asked for by itself when a
//! download's link has expired (the server refuses it).

use super::wnd::{self, Rows, Window};
use super::{dialogs, progress, shell, App};
use std::path::Path;
use std::rc::Rc;
use turbodm::engine::{AddRequest, Snapshot};
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::*;

/// A download waiting for its fresh address, and the window that says so.
pub struct Waiting {
    id: String,
    window: Rc<Window>,
}

/// Open the download's page in the browser and wait for its link to be clicked again.
pub fn start(app: &Rc<App>, id: &str) {
    let (Some(snap), Some(req)) = (app.manager.get(id), app.manager.request_of(id)) else { return };
    cancel(app);
    // the page it came from; when the browser didn't say, the site it's on (its home page)
    let page = req.referrer.filter(|r| r.starts_with("http"));
    let site = site_of(&req.url);
    if let Some(open) = page.as_ref().or(site.as_ref()) {
        shell::open(Path::new(open));
    }
    let said = match (&page, &site) {
        (Some(_), _) => format!("The page \u{201c}{}\u{201d} came from is open in your browser. Click its download link \
                                 (or button) there again: TurboDM takes the fresh address and goes on from where it was.",
                                snap.filename),
        (None, Some(site)) => format!("{site} is open in your browser. Find \u{201c}{}\u{201d} there and click its download \
                                       link again: TurboDM takes the fresh address and goes on from where it was.",
                                      snap.filename),
        (None, None) => format!("Open the page \u{201c}{}\u{201d} came from in your browser and click its download link \
                                 (or button) again: TurboDM takes the fresh address and goes on from where it was.",
                                snap.filename),
    };
    let window = Window::dialog("Refresh download address", None, (520, 150), false);
    let note = window.note(&said);
    let waiting = window.note("Waiting for the link\u{2026}");
    let (a, i) = (app.clone(), id.to_string());
    let paste = window.button("Paste a link instead\u{2026}", move || {
        cancel(&a);
        dialogs::refresh_address(&a, &i);
    });
    let a = app.clone();
    let close = window.button("Cancel", move || cancel(&a));
    let a = app.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| cancel(&a));
    let a = app.clone();
    window.on_close(move || {
        a.refreshing.borrow_mut().take();
        true
    });
    let mut rows = Rows::new(16, 14, 0, 488);
    rows.full(note, 52);
    rows.full(waiting, 22);
    let bottom = rows.buttons(&[paste, close]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show_in_front();
    *app.refreshing.borrow_mut() = Some(Waiting { id: id.to_string(), window });
}

/// Stop waiting (and close its window).
fn cancel(app: &Rc<App>) {
    let waiting = app.refreshing.borrow_mut().take();
    if let Some(waiting) = waiting {
        waiting.window.destroy();
    }
}

/// A download caught from the browser: the fresh address of the one waiting for it, if one is
/// (then it's taken: true); else it's a new download (false).
pub fn take(app: &Rc<App>, req: &AddRequest) -> bool {
    let waiting = app.refreshing.borrow_mut().take();
    let Some(waiting) = waiting else { return false };
    waiting.window.destroy();
    app.manager.refresh_from(&waiting.id, req.clone());
    if app.manager.settings().show_progress_window {
        progress::open(app, &waiting.id);
    }
    true
}

/// The download failed because its link expired: offer to refresh it (once a download, while
/// TurboDM runs).
pub fn offer(app: &Rc<App>, snap: &Snapshot) {
    let error = snap.error.clone().unwrap_or_default();
    if !error.contains("link may have expired") || !app.offered_refresh.borrow_mut().insert(snap.id.clone()) {
        return;
    }
    let body = format!("The address of \u{201c}{}\u{201d} seems to have expired ({}).\n\nRefresh it from the page it \
                        came from and go on from where it was?", snap.filename,
                       error.split(" - ").next().unwrap_or(&error));
    // SAFETY: a modal question box, in front
    let answer = unsafe {
        MessageBoxW(None, &HSTRING::from(body), &HSTRING::from("Refresh download address"),
                    MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND)
    };
    if answer == IDYES {
        start(app, &snap.id);
    }
}

/// A web address's site: "https://example.com/" for "https://example.com/files/a.zip?x=1".
fn site_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://").filter(|(s, _)| s.starts_with("http"))?;
    let host = rest.split(['/', '?', '#']).next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}/"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn site_of_an_address() {
        assert_eq!(super::site_of("https://cdn.example.com/files/a.zip?x=1").as_deref(), Some("https://cdn.example.com/"));
        assert_eq!(super::site_of("http://host:8080").as_deref(), Some("http://host:8080/"));
        assert_eq!(super::site_of("magnet:?xt=urn:btih:abc"), None);
    }
}
