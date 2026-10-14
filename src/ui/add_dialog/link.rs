//! Checking a new address. Like IDM, the download itself starts behind the dialog, on one
//! connection and out of sight, so it's already connected when the user presses Start; with
//! that turned off, the server is just asked about the file.

use super::Form;
use crate::ui::Ctx;
use gtk::glib;
use gtk::prelude::*;
use std::rc::Rc;
use std::time::Duration;
use turbodm::engine::http::{self, Headers};
use turbodm::engine::{AddRequest, Status};
use turbodm::util::human_size;

/// A new address: start downloading it right away, or just ask the server about it.
pub fn check(ctx: &Rc<Ctx>, form: &Rc<Form>, req: &AddRequest) {
    let url = form.url();
    form.take_early(ctx, None);
    if !url.starts_with("http") {
        return;
    }
    if !ctx.manager.settings().predownload {
        return probe(ctx, form, req);
    }
    let (directory, filename) = form.target(ctx);
    let chosen = form.chosen.get();
    let id = ctx.manager.prefetch(AddRequest {
        url: url.clone(),
        filename: filename.filter(|_| chosen),
        directory: chosen.then_some(directory),
        ..req.clone()
    });
    *form.early.borrow_mut() = Some((url, id.clone()));
    form.info.set_text("Checking link…"); // it's downloading, quietly: nothing to show yet
    let (ctx, form, mut filled) = (ctx.clone(), form.clone(), false);
    glib::timeout_add_local(Duration::from_millis(150), move || {
        if form.early.borrow().as_ref().is_none_or(|(_, early)| *early != id) {
            return glib::ControlFlow::Break; // started, cancelled, or another address
        }
        let Some(s) = ctx.manager.get(&id) else { return glib::ControlFlow::Break };
        let known = !matches!(s.status, Status::Connecting | Status::Queued | Status::Error);
        if known && !filled {
            filled = true;
            form.show(&s.directory, &s.filename); // the server's name, in its folder
        }
        let text = match s.status {
            Status::Connecting | Status::Queued => "Checking link…".to_string(),
            Status::Error => format!("Couldn't check the link: {}", s.error.as_deref().unwrap_or("unknown error")),
            Status::Completed => {
                format!("Downloaded already ({}): it goes where you say when you start it", human_size(s.size))
            }
            _ => info(s.size, s.resumable),
        };
        if form.info.text() != text {
            form.info.set_text(&text);
        }
        glib::ControlFlow::Continue
    });
}

/// Ask the server for name, size and resume support (runs on the tokio runtime).
fn probe(ctx: &Rc<Ctx>, form: &Rc<Form>, req: &AddRequest) {
    let url = form.url();
    let id = form.probe_id.get() + 1;
    form.probe_id.set(id);
    form.info.set_text("Checking link…");
    let headers = Headers { user_agent: req.user_agent.clone(), referrer: req.referrer.clone(),
                            cookies: req.cookies.clone() };
    let client = ctx.manager.shared.client();
    let job = ctx.rt.spawn(async move { http::probe(&client, &url, &headers).await });
    let (form, settings) = (form.clone(), ctx.manager.settings());
    glib::spawn_future_local(async move {
        let result = job.await;
        if form.probe_id.get() != id {
            return; // the address changed meanwhile
        }
        match result {
            Ok(Ok(found)) => {
                form.info.set_text(&info(found.size, found.resumable));
                form.show(&settings.folder_for(&found.filename), &found.filename);
            }
            Ok(Err(err)) => form.info.set_text(&format!("Couldn't check the link: {err}")),
            Err(_) => form.info.set_text("Couldn't check the link"),
        }
    });
}

fn info(size: Option<u64>, resumable: bool) -> String {
    let resume = if resumable { "yes" } else { "no (single connection)" };
    format!("Size: {}   ·   Resume support: {resume}", human_size(size))
}
