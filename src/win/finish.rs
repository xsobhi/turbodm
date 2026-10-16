//! When a download finishes or fails: the "Download complete" dialog (complete.rs), the
//! download's "on completion" options, or a notification for downloads nobody was watching.

use super::{power, progress, refresh, shell, tray, App};
use std::rc::Rc;
use turbodm::engine::{Snapshot, Status};
use turbodm::power::Power;

pub fn on_status(app: &Rc<App>, snap: Snapshot) {
    if matches!(snap.status, Status::Completed | Status::Error) {
        let then = app.when_done.borrow_mut().remove(&snap.id);
        if let Some(then) = then {
            return then(app, &snap); // a download kept out of the list: what it's for
        }
    }
    match snap.status {
        Status::Completed => completed(app, snap),
        Status::Error => {
            if app.manager.settings().notify_complete {
                let error = snap.error.clone().unwrap_or_default();
                tray::notify(app, "Download failed", &format!("{}\n{error}", snap.filename), None);
            }
            refresh::offer(app, &snap); // (its link expired: refresh it?)
        }
        _ => {}
    }
}

fn completed(app: &Rc<App>, snap: Snapshot) {
    progress::close(app, &snap.id);
    // downloads with a progress window (now or earlier) get the dialog, like IDM
    let watched = app.on_done.borrow_mut().remove(&snap.id);
    let settings = app.manager.settings();
    if let Some(todo) = watched {
        if todo.open_file {
            shell::open(&snap.path);
        }
        if settings.show_complete_dialog && todo.power == Power::Nothing {
            super::complete::open(app, &snap);
        }
        power::countdown(app, todo.power, &snap.filename);
        if settings.show_complete_dialog {
            return;
        }
    }
    if settings.notify_complete {
        tray::notify(app, "Download complete", &snap.filename, Some(snap.id.clone()));
    }
}
