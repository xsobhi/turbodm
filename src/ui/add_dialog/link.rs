//! Checking a new address. Like IDM, the download itself starts behind the dialog, on one
//! connection and out of sight, so it's already connected when the user presses Start; with
//! that turned off, the server is just asked about the file.

use super::{update_folder_label, Form};
use crate::ui::Ctx;
use gtk::glib;
use gtk::prelude::*;
use std::rc::Rc;
use std::time::Duration;
use turbodm::categories::target_dir;
use turbodm::engine::http::{self, Headers};
use turbodm::engine::{AddRequest, Status};
use turbodm::util::human_size;

/// A new address: start downloading it right away, or just ask the server about it.
pub fn check(ctx: &Rc<Ctx>, form: &Rc<Form>, req: &AddRequest) {
    let url = form.url.text().trim().to_string();
    form.take_early(ctx, None);
    if !url.starts_with("http") {
        return;
    }
    if !ctx.manager.settings().predownload {
        return probe(ctx, form, req);
    }
    let id = ctx.manager.prefetch(AddRequest {
        url: url.clone(),
        filename: form.name_edited.get().then(|| form.name.text().trim().to_string()).filter(|n| !n.is_empty()),
        directory: form.dir_chosen.get().then(|| form.dir.borrow().clone()),
        connections: Some(form.connections.value() as usize),
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
        let text = match s.status {
            Status::Connecting | Status::Queued => "Checking link…".to_string(),
            Status::Error => format!("Couldn't check the link: {}", s.error.as_deref().unwrap_or("unknown error")),
            _ => {
                if !filled {
                    filled = true;
                    if !form.name_edited.get() {
                        form.name.set_text(&s.filename);
                    }
                    if !form.dir_chosen.get() {
                        *form.dir.borrow_mut() = s.directory.clone();
                        update_folder_label(&form);
                    }
                }
                let resume = if s.resumable { "yes" } else { "no (single connection)" };
                format!("Size: {}   ·   Resume support: {resume}", human_size(s.size))
            }
        };
        if form.info.text() != text {
            form.info.set_text(&text);
        }
        glib::ControlFlow::Continue
    });
}

/// Ask the server for name, size and resume support (runs on the tokio runtime).
fn probe(ctx: &Rc<Ctx>, form: &Rc<Form>, req: &AddRequest) {
    let url = form.url.text().trim().to_string();
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
            Ok(Ok(info)) => {
                let resume = if info.resumable { "yes" } else { "no (single connection)" };
                form.info.set_text(&format!("Size: {}   ·   Resume support: {resume}", human_size(info.size)));
                if !form.name_edited.get() {
                    form.name.set_text(&info.filename);
                }
                if !form.dir_chosen.get() {
                    let name = form.name.text();
                    *form.dir.borrow_mut() = target_dir(&settings.download_dir, &name, settings.use_categories);
                    update_folder_label(&form);
                }
            }
            Ok(Err(err)) => form.info.set_text(&format!("Couldn't check the link: {err}")),
            Err(_) => form.info.set_text("Couldn't check the link"),
        }
    });
}
