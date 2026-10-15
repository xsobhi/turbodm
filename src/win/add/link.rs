//! Checking a new address. Like IDM, the download itself starts behind the dialog, on one
//! connection and out of sight; with that turned off, the server is just asked about the file.
//! A torrent is always looked up that way: it waits for Start before downloading anything.
//! A video site's page, or any address the server answers with a web page, is looked at for a
//! video instead (video.rs).

use super::super::wnd::set_text;
use super::super::App;
use super::{is_link, video, Form};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use turbodm::engine::http::{self, Headers, ProbeInfo};
use turbodm::engine::torrent::is_torrent;
use turbodm::engine::video::{is_video_site, is_web_page};
use turbodm::engine::{AddRequest, Snapshot, Status};
use turbodm::text::torrent_info;
use turbodm::util::human_size;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, WM_TIMER};

/// (check number, answer) from the network thread.
pub type Probed = Arc<Mutex<Option<(u64, Result<ProbeInfo, String>)>>>;

const TYPING: usize = 2; // wait for a pause in typing
const WATCH: usize = 3;  // follow the early download / the check

pub fn start_timer(form: &Rc<Form>, app: &Rc<App>) {
    let hwnd = form.window.hwnd();
    let (f, a) = (Rc::downgrade(form), Rc::downgrade(app));
    form.window.hook(move |msg, wparam, _| {
        let (Some(form), Some(app)) = (f.upgrade(), a.upgrade()) else { return None };
        match (msg, wparam.0) {
            (WM_TIMER, TYPING) => {
                // SAFETY: our own timer
                unsafe { let _ = KillTimer(Some(hwnd), TYPING); }
                check(&app, &form);
            }
            (WM_TIMER, WATCH) => watch(&app, &form),
            _ => return None,
        }
        Some(LRESULT(0))
    });
    // SAFETY: a timer on our window
    unsafe { SetTimer(Some(hwnd), WATCH, 150, None) };
}

/// The address was edited: check it once typing pauses (every new address starts a download).
pub fn changed(form: &Rc<Form>) {
    form.update_remember();
    // SAFETY: (re)starts our timer
    unsafe { SetTimer(Some(form.window.hwnd()), TYPING, 350, None) };
}

/// A new address: start downloading it right away, or just ask the server about it; a video
/// site's page (or any, in a dialog opened for a video): look for its video.
pub fn check(app: &Rc<App>, form: &Rc<Form>) {
    let url = form.url();
    form.take_early(app, None);
    form.probe_id.set(form.probe_id.get() + 1);
    form.filled.set(false);
    video::leave(form);
    if !is_link(&url) {
        return;
    }
    if form.video.wanted || (is_video_site(&url) && !form.video.not_here(&url)) {
        video::look(app, form, &url);
        return;
    }
    set_text(form.info.get(), "Checking link…");
    if app.manager.settings().predownload || is_torrent(&url, None) {
        let (directory, filename) = form.target(app);
        let chosen = form.chosen.get();
        let id = app.manager.prefetch(AddRequest {
            url: url.clone(),
            filename: filename.filter(|_| chosen),
            directory: chosen.then_some(directory),
            ..form.req.clone()
        });
        *form.early.borrow_mut() = Some((url, id));
        return;
    }
    let req = &form.req;
    let headers = Headers { user_agent: req.user_agent.clone(), referrer: req.referrer.clone(), cookies: req.cookies.clone() };
    let (client, probed, number) = (app.manager.shared.client(), form.probed.clone(), form.probe_id.get());
    app.rt.spawn(async move {
        let answer = http::probe(&client, &url, &headers).await.map_err(|e| e.to_string());
        *probed.lock().unwrap() = Some((number, answer));
    });
}

fn info(size: Option<u64>, resumable: bool) -> String {
    let resume = if resumable { "yes" } else { "no (single connection)" };
    format!("Size: {}   ·   Resume support: {resume}", human_size(size))
}

/// The server answered with a web page: look for a video on it (unless there was none).
fn page(app: &Rc<App>, form: &Rc<Form>, content_type: Option<&str>) -> bool {
    let url = form.url();
    if !content_type.is_some_and(is_web_page) || form.video.not_here(&url) {
        return false;
    }
    form.take_early(app, None); // the page itself isn't wanted
    video::look(app, form, &url);
    true
}

/// Show what the early download or the check found out.
fn watch(app: &Rc<App>, form: &Rc<Form>) {
    video::watch(app, form);
    let early = form.early.borrow().as_ref().map(|(_, id)| id.clone());
    if let Some(s) = early.and_then(|id| app.manager.get(&id)) {
        if page(app, form, s.content_type.as_deref()) {
            return;
        }
        let line = match s.status {
            _ if s.torrent.is_some() => torrent_found(app, form, &s),
            Status::Connecting | Status::Queued => "Checking link…".to_string(),
            Status::Error => format!("Couldn't check the link: {}", s.error.as_deref().unwrap_or("unknown error")),
            Status::Completed => {
                if !form.filled.replace(true) {
                    show_found(app, form, &s.filename, Some(s.directory.clone()));
                }
                format!("Downloaded already ({}): it goes where you say when you start it", human_size(s.size))
            }
            _ => {
                if !form.filled.replace(true) {
                    show_found(app, form, &s.filename, Some(s.directory.clone()));
                }
                info(s.size, s.resumable)
            }
        };
        set_text(form.info.get(), &line);
    }
    let answer = form.probed.lock().unwrap().take();
    match answer {
        Some((number, _)) if number != form.probe_id.get() => {} // the address changed meanwhile
        Some((_, Ok(probe))) if page(app, form, Some(&probe.content_type)) => {}
        Some((_, Ok(probe))) => {
            set_text(form.info.get(), &info(probe.size, probe.resumable));
            show_found(app, form, &probe.filename, None);
        }
        Some((_, Err(err))) => set_text(form.info.get(), &format!("Couldn't check the link: {err}")),
        None => {}
    }
}

/// A torrent looked up: what's in it, shown where it goes once that's known.
fn torrent_found(app: &Rc<App>, form: &Rc<Form>, s: &Snapshot) -> String {
    if s.size.is_some() && s.status != Status::Error && !form.filled.replace(true) {
        show_found(app, form, &s.filename, Some(s.directory.clone()));
    }
    torrent_info(s)
}

/// The server's name for the file, in its folder, unless the user chose where it goes.
fn show_found(app: &Rc<App>, form: &Rc<Form>, filename: &str, directory: Option<std::path::PathBuf>) {
    let folder = directory.unwrap_or_else(|| app.manager.settings().folder_for(filename));
    form.show(&folder, filename);
}
