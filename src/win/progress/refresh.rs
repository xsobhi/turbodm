//! Keeping progress windows up to date, and closing them.

use super::super::theme;
use super::super::wnd::{enable, set_text};
use super::super::App;
use super::ProgressWindow;
use std::rc::Rc;
use turbodm::engine::{Manager, Status};
use turbodm::text::{eta_text, rate_text, status_line, status_text};
use turbodm::util::human_size;

impl ProgressWindow {
    /// Update from the engine; false when the download no longer exists.
    pub fn refresh(&self, manager: &Manager) -> bool {
        let Some(s) = manager.get(&self.id) else { return false };
        let downloaded = match s.progress {
            Some(p) => format!("{} ({:.2}%)", human_size(Some(s.downloaded)), p * 100.0),
            None => human_size(Some(s.downloaded)),
        };
        // the status in the accent colour while it works (IDM's blue "Receiving data…")
        let status = match s.status {
            Status::Error => theme::DANGER,
            status if status.is_active() => theme::ACCENT,
            _ => theme::TEXT,
        };
        let resume = if s.resumable { "Yes" } else { "No (single connection, can't pause and resume)" };
        self.info.set(vec![
            (s.url.clone(), theme::TEXT),
            (s.path.display().to_string(), theme::TEXT),
            (status_line(&s), status),
            (human_size(s.size), theme::TEXT),
            (downloaded, theme::TEXT),
            (rate_text(&s), theme::TEXT),
            (eta_text(&s), theme::TEXT),
            (resume.into(), theme::TEXT),
        ]);
        let title = match s.progress {
            Some(p) => format!("{:.0}% {}", p * 100.0, s.filename),
            None => s.filename.clone(),
        };
        self.window.set_title(&title);
        self.bar.set(s.progress.unwrap_or(0.0), status_text(&s));
        self.details.update(&s);
        let running = s.status.is_active() || s.status == Status::Queued;
        set_text(self.toggle, if running { "Pause" } else { "Resume" });
        enable(self.toggle, s.status != Status::Completed);
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
