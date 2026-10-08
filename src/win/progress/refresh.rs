//! Keeping progress windows up to date, and closing them.

use super::super::wnd::{set_progress, set_text};
use super::super::App;
use super::ProgressWindow;
use turbodm::text::status_line;
use std::rc::Rc;
use turbodm::engine::{Manager, Status};
use turbodm::util::{human_eta, human_size, human_speed};

impl ProgressWindow {
    /// Update from the engine; false when the download no longer exists.
    pub fn refresh(&self, manager: &Manager) -> bool {
        let Some(s) = manager.get(&self.id) else { return false };
        let downloaded = match s.progress {
            Some(p) => format!("{} ({:.2}%)", human_size(Some(s.downloaded)), p * 100.0),
            None => human_size(Some(s.downloaded)),
        };
        let values = [
            s.url.clone(),
            s.path.display().to_string(),
            status_line(&s),
            human_size(s.size),
            downloaded,
            human_speed(s.speed),
            human_eta(s.eta),
            if s.resumable { "Yes".into() } else { "No (single connection, can't pause and resume)".into() },
        ];
        for (label, value) in self.values.iter().zip(values) {
            set_text(*label, &value);
        }
        let title = match s.progress {
            Some(p) => format!("{:.0}% {}", p * 100.0, s.filename),
            None => s.filename.clone(),
        };
        self.window.set_title(&title);
        set_progress(self.bar, s.progress.unwrap_or(0.0));
        set_text(self.percent, &s.progress.map(|p| format!("{:.1}%", p * 100.0)).unwrap_or_default());
        self.details.update(&s);
        let running = s.status.is_active() || s.status == Status::Queued;
        set_text(self.toggle, if running { "Pause" } else { "Resume" });
        super::super::wnd::enable(self.toggle, s.status != Status::Completed);
        true
    }
}

/// Every open progress window (and closes those of removed downloads).
pub fn refresh_all(app: &Rc<App>) {
    let gone: Vec<String> = app.progress.borrow().iter()
        .filter(|(_, p)| !p.refresh(&app.manager)).map(|(id, _)| id.clone()).collect();
    for id in gone {
        close(app, &id);
    }
}

/// Close a download's window (it finished or was removed). True if it had one.
pub fn close(app: &Rc<App>, id: &str) -> bool {
    let window = app.progress.borrow().get(id).map(|p| p.window.clone());
    window.map(|w| w.destroy()).is_some() // its WM_DESTROY forgets it
}
