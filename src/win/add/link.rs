//! Checking a new address. Like IDM, the download itself starts behind the dialog, on one
//! connection and out of sight; with that turned off, the server is just asked about the file.

use super::super::wnd::{set_text, text};
use super::super::App;
use super::Form;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use turbodm::engine::http::{self, Headers, ProbeInfo};
use turbodm::engine::{AddRequest, Status};
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

/// A new address: start downloading it right away, or just ask the server about it.
pub fn check(app: &Rc<App>, form: &Rc<Form>) {
    let url = form.url();
    form.take_early(app, None);
    form.probe_id.set(form.probe_id.get() + 1);
    form.filled.set(false);
    if !url.starts_with("http") {
        return;
    }
    set_text(form.info.get(), "Checking link…");
    let name = text(form.name.get()).trim().to_string();
    if app.manager.settings().predownload {
        let id = app.manager.prefetch(AddRequest {
            url: url.clone(),
            filename: form.name_edited.get().then_some(name).filter(|n| !n.is_empty()),
            directory: form.dir_chosen.get().then(|| form.dir.borrow().clone()),
            connections: text(form.connections.get()).parse().ok(),
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

/// Show what the early download or the check found out.
fn watch(app: &Rc<App>, form: &Rc<Form>) {
    let early = form.early.borrow().as_ref().map(|(_, id)| id.clone());
    if let Some(s) = early.and_then(|id| app.manager.get(&id)) {
        let line = match s.status {
            Status::Connecting | Status::Queued => "Checking link…".to_string(),
            Status::Error => format!("Couldn't check the link: {}", s.error.as_deref().unwrap_or("unknown error")),
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
        Some((_, Ok(probe))) => {
            set_text(form.info.get(), &info(probe.size, probe.resumable));
            show_found(app, form, &probe.filename, None);
        }
        Some((_, Err(err))) => set_text(form.info.get(), &format!("Couldn't check the link: {err}")),
        None => {}
    }
}

/// The server's name for the file, and its folder, unless the user chose them.
fn show_found(app: &Rc<App>, form: &Rc<Form>, filename: &str, directory: Option<std::path::PathBuf>) {
    if !form.name_edited.get() {
        set_text(form.name.get(), filename);
    }
    if !form.dir_chosen.get() {
        *form.dir.borrow_mut() = directory.unwrap_or_else(|| app.manager.settings().folder_for(filename));
        form.show_folder();
    }
}
