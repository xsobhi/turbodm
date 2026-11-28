//! IDM's "Refresh download address", as on Windows (src/win/refresh.rs): the page a download came
//! from opens in the browser, and the next download caught from there (its link clicked again)
//! gives the waiting download a fresh address, cookies and referrer; it goes on from where it
//! was. Asked for by itself when a download's link has expired (the server refuses it).

use super::{actions, dialogs, frame, progress, Ctx};
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::engine::{AddRequest, Snapshot};

/// Open the download's page in the browser and wait for its link to be clicked again.
pub fn start(ctx: &Rc<Ctx>, id: &str) {
    let (Some(snap), Some(req)) = (ctx.manager.get(id), ctx.manager.request_of(id)) else { return };
    cancel(ctx);
    // the page it came from; when the browser didn't say, the site it's on (its home page)
    let page = req.referrer.filter(|r| r.starts_with("http"));
    let site = site_of(&req.url);
    if let Some(open) = page.as_ref().or(site.as_ref()) {
        actions::open_uri(ctx, open);
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
    let d = frame::dialog_with("Refresh download address", None, 520, ":close", (14, 16));
    d.body.set_margin_bottom(14);
    d.body.append(&frame::note(&said));
    let waiting = frame::note("Waiting for the link\u{2026}");
    waiting.set_margin_top(10);
    d.body.append(&waiting);
    let paste = frame::button("Paste a link instead\u{2026}");
    let close = frame::button("Cancel");
    let (c, i) = (ctx.clone(), id.to_string());
    paste.connect_clicked(move |_| {
        cancel(&c);
        dialogs::refresh_address(&c, &i);
    });
    let c = ctx.clone();
    close.connect_clicked(move |_| cancel(&c));
    let c = Rc::downgrade(ctx);
    d.window.connect_close_request(move |_| {
        if let Some(c) = c.upgrade() {
            c.refreshing.borrow_mut().take();
        }
        gtk::glib::Propagation::Proceed
    });
    frame::buttons(&d.body, &[&paste, &close], 12);
    d.window.present();
    *ctx.refreshing.borrow_mut() = Some((id.to_string(), d.window));
}

/// Stop waiting (and close its window).
fn cancel(ctx: &Rc<Ctx>) {
    let waiting = ctx.refreshing.borrow_mut().take();
    if let Some((_, window)) = waiting {
        window.close();
    }
}

/// A download caught from the browser: the fresh address of the one waiting for it, if one is
/// (then it's taken: true); else it's a new download (false).
pub fn take(ctx: &Rc<Ctx>, req: &AddRequest) -> bool {
    let waiting = ctx.refreshing.borrow_mut().take();
    let Some((id, window)) = waiting else { return false };
    window.close();
    ctx.manager.refresh_from(&id, req.clone());
    if ctx.manager.settings().show_progress_window {
        progress::open(ctx, &id);
    }
    true
}

/// The download failed because its link expired: offer to refresh it (once a download, while
/// TurboDM runs).
pub fn offer(ctx: &Rc<Ctx>, snap: &Snapshot) {
    let error = snap.error.clone().unwrap_or_default();
    if !error.contains("link may have expired") || !ctx.offered_refresh.borrow_mut().insert(snap.id.clone()) {
        return;
    }
    let body = format!("The address of \u{201c}{}\u{201d} seems to have expired ({}).\n\nRefresh it from the page it \
                        came from and go on from where it was?", snap.filename,
                       error.split(" - ").next().unwrap_or(&error));
    let (c, id) = (ctx.clone(), snap.id.clone());
    frame::ask(None, "Refresh download address", &body, move || start(&c, &id));
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
