//! Checking a new address. Like IDM, the download itself starts behind the dialog, on one
//! connection and out of sight; with that turned off, the server is just asked about the file.
//! A torrent is always looked up that way: it waits for Start before downloading anything.
//! A video site's page, or any address the server answers with a web page, is looked at for a
//! video instead (video.rs).

use super::{is_link, video, Form};
use crate::ui::Ctx;
use gtk::glib;
use gtk::prelude::*;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use turbodm::engine::http::{self, Headers, ProbeInfo};
use turbodm::engine::torrent::is_torrent;
use turbodm::engine::video::{is_video_site, is_web_page};
use turbodm::engine::{AddRequest, Snapshot, Status};
use turbodm::text::torrent_info;
use turbodm::util::{human_size, unnumbered};

/// (check number, answer) from the network.
pub type Probed = Arc<Mutex<Option<(u64, Result<ProbeInfo, String>)>>>;

/// Follow the early download and the checks every 150 ms while the dialog is open.
pub fn start_timer(ctx: &Rc<Ctx>, form: &Rc<Form>) {
    let (f, c) = (Rc::downgrade(form), Rc::downgrade(ctx));
    glib::timeout_add_local(Duration::from_millis(150), move || {
        let (Some(form), Some(ctx)) = (f.upgrade(), c.upgrade()) else { return glib::ControlFlow::Break };
        if !form.window.is_visible() {
            return glib::ControlFlow::Break; // (closed)
        }
        watch(&ctx, &form);
        glib::ControlFlow::Continue
    });
}

/// The address was edited: check it once typing pauses (every new address starts a download).
pub fn changed(ctx: &Rc<Ctx>, form: &Rc<Form>) {
    form.update_remember();
    let number = form.probe_id.get() + 1;
    form.probe_id.set(number);
    let (f, c) = (Rc::downgrade(form), Rc::downgrade(ctx));
    glib::timeout_add_local_once(Duration::from_millis(350), move || {
        if let (Some(form), Some(ctx)) = (f.upgrade(), c.upgrade())
            && form.probe_id.get() == number {
                check(&ctx, &form);
            }
    });
}

/// A new address: start downloading it right away, or just ask the server about it; a video
/// site's page (or any, in a dialog opened for a video): look for its video.
pub fn check(ctx: &Rc<Ctx>, form: &Rc<Form>) {
    let url = form.url();
    form.take_early(ctx, None);
    form.probe_id.set(form.probe_id.get() + 1);
    form.filled.set(false);
    video::leave(form);
    if !is_link(&url) {
        return;
    }
    if form.video.wanted || (is_video_site(&url) && !form.video.not_here(&url)) {
        video::look(ctx, form, &url);
        return;
    }
    form.info.set_text("Checking link…");
    if ctx.manager.settings().predownload || is_torrent(&url, None) {
        let (directory, filename) = form.target(ctx);
        let chosen = form.chosen.get();
        let id = ctx.manager.prefetch(AddRequest {
            url: url.clone(),
            filename: filename.filter(|_| chosen),
            directory: chosen.then_some(directory),
            ..form.req.clone()
        });
        *form.early.borrow_mut() = Some((url, id));
        return;
    }
    let req = &form.req;
    let headers = Headers { user_agent: req.user_agent.clone(), referrer: req.referrer.clone(),
                            cookies: req.cookies.clone(), extra: vec![] };
    let (client, probed, number) = (ctx.manager.shared.client(), form.probed.clone(), form.probe_id.get());
    ctx.rt.spawn(async move {
        let answer = http::probe(&client, &url, &headers).await.map_err(|e| e.to_string());
        *probed.lock().unwrap() = Some((number, answer));
    });
}

fn info(size: Option<u64>, resumable: bool) -> String {
    let resume = if resumable { "yes" } else { "no (single connection)" };
    format!("Size: {}   ·   Resume support: {resume}", human_size(size))
}

/// The server answered with a web page: look for a video on it (unless there was none).
fn page(ctx: &Rc<Ctx>, form: &Rc<Form>, content_type: Option<&str>) -> bool {
    let url = form.url();
    if !content_type.is_some_and(is_web_page) || form.video.not_here(&url) {
        return false;
    }
    form.take_early(ctx, None); // the page itself isn't wanted
    video::look(ctx, form, &url);
    true
}

fn set_info(form: &Form, text: &str) {
    if form.info.text() != text {
        form.info.set_text(text);
    }
}

/// Show what the early download or the check found out.
fn watch(ctx: &Rc<Ctx>, form: &Rc<Form>) {
    video::watch(ctx, form);
    // (a video's early download says nothing here: its quality's line does)
    let file = form.video.found.borrow().is_none();
    let early = form.early.borrow().as_ref().filter(|_| file).map(|(_, id)| id.clone());
    if let Some(s) = early.and_then(|id| ctx.manager.get(&id)) {
        if page(ctx, form, s.content_type.as_deref()) {
            return;
        }
        let line = match s.status {
            _ if s.torrent.is_some() => torrent_found(ctx, form, &s),
            Status::Connecting | Status::Queued => "Checking link…".to_string(),
            Status::Error => format!("Couldn't check the link: {}", s.error.as_deref().unwrap_or("unknown error")),
            Status::Completed => {
                if !form.filled.replace(true) {
                    show_found(ctx, form, &s.filename, Some(s.directory.clone()));
                }
                format!("Downloaded already ({}): it goes where you say when you start it", human_size(s.size))
            }
            _ => {
                if !form.filled.replace(true) {
                    show_found(ctx, form, &s.filename, Some(s.directory.clone()));
                }
                info(s.size, s.resumable)
            }
        };
        set_info(form, &line);
    }
    let answer = form.probed.lock().unwrap().take();
    match answer {
        Some((number, _)) if number != form.probe_id.get() => {} // the address changed meanwhile
        Some((_, Ok(probe))) if page(ctx, form, Some(&probe.content_type)) => {}
        Some((_, Ok(probe))) => {
            set_info(form, &info(probe.size, probe.resumable));
            show_found(ctx, form, &probe.filename, None);
        }
        Some((_, Err(err))) => set_info(form, &format!("Couldn't check the link: {err}")),
        None => {}
    }
}

/// A torrent looked up: what's in it, shown where it goes once that's known.
fn torrent_found(ctx: &Rc<Ctx>, form: &Rc<Form>, s: &Snapshot) -> String {
    if s.size.is_some() && s.status != Status::Error && !form.filled.replace(true) {
        show_found(ctx, form, &s.filename, Some(s.directory.clone()));
    }
    torrent_info(s)
}

/// The server's name for the file, in its folder, unless the user chose where it goes. (The
/// early download is "name (1).ext" when "name.ext" is there already: the window shows the
/// server's name, and Start asks whether to replace that file or keep both: exists.rs.)
fn show_found(ctx: &Rc<Ctx>, form: &Rc<Form>, filename: &str, directory: Option<std::path::PathBuf>) {
    let early = directory.is_some();
    let folder = directory.unwrap_or_else(|| ctx.manager.settings().folder_for(filename));
    let name = early.then(|| unnumbered(&folder, filename)).flatten().unwrap_or_else(|| filename.to_string());
    form.show(&folder, &name);
}
