//! "A new version is available", like IDM: checked at start and once a day. The new installer
//! is downloaded by TurboDM itself, in a progress window as any download (but not in the list),
//! then run straight away: it replaces this version and opens the new one.

use super::wnd::{self, Rows, Window, LINE};
use super::{dialogs, post, progress, App};
use std::rc::Rc;
use std::time::Duration;
use turbodm::engine::{AddRequest, Snapshot};
use turbodm::update::{self, installed_by_setup, run_installer, RELEASES_PAGE};
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn start(app: &Rc<App>) {
    app.rt.spawn(async {
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            post(|app| check(app, false));
            tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        }
    });
}

/// Ask GitHub. `manual`: from the menu, so also say when there's nothing new.
pub fn check(app: &Rc<App>, manual: bool) {
    let settings = app.manager.settings();
    if !manual && !settings.check_updates {
        return;
    }
    let client = app.manager.shared.client();
    app.rt.spawn(async move {
        let found = update::check(&client).await;
        post(move |app| match found {
            Ok(Some(release)) if manual || release.version != app.manager.settings().skipped_update => {
                *app.update.borrow_mut() = Some(release);
                if manual || app.main.is_visible() {
                    show_pending(app);
                }
            }
            Ok(_) if manual => super::dialogs::message(app.main.window.hwnd(), "TurboDM is up to date",
                &format!("You have the latest version, {}.", turbodm::config::VERSION), false),
            Err(err) if manual => super::dialogs::message(app.main.window.hwnd(), "Couldn't check for updates", &err, true),
            _ => {}
        });
    });
}

/// Show the update found earlier (it waits while TurboDM is hidden in the notification area).
pub fn show_pending(app: &Rc<App>) {
    let Some(release) = app.update.borrow_mut().take() else { return };
    let can_install = installed_by_setup() && release.installer.is_some();
    let mut body = format!("You have version {}.", turbodm::config::VERSION);
    if !release.notes.is_empty() {
        body.push_str(&format!("\n\nWhat's new:\n{}", release.notes));
    }
    let lines = body.lines().map(|l| 1 + l.len() as i32 / 70).sum::<i32>().min(20);
    let window = Window::dialog("TurboDM update", None, (480, 200), false);
    let heading = window.heading(&format!("TurboDM {} is available", release.version));
    let text = window.note(&body.replace('\n', "\r\n"));
    let (a, w, version) = (app.clone(), window.clone(), release.version.clone());
    let skip = window.button("Skip this version", move || {
        let mut s = a.manager.settings();
        s.skipped_update = version.clone();
        a.manager.update_settings(s);
        w.destroy();
    });
    let w = window.clone();
    let later = window.button("Later", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let (a, w) = (app.clone(), window.clone());
    let go = window.default_button(if can_install { "Update now" } else { "Download" }, move || {
        match release.installer.clone().filter(|_| can_install) {
            Some(url) => install(&a, &url, &release.version),
            None => {
                let page = if release.page.is_empty() { RELEASES_PAGE } else { &release.page };
                super::shell::open(std::path::Path::new(page));
            }
        }
        w.destroy();
    });
    let mut rows = Rows::new(16, 14, 0, 448);
    rows.full(heading, 24);
    rows.full(text, lines * LINE);
    let bottom = rows.buttons(&[skip, later, go]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}

/// Download the installer with TurboDM, in a progress window, kept out of the list; `downloaded`
/// runs it when it's here.
fn install(app: &Rc<App>, url: &str, version: &str) {
    // (CI: this build's installer, from a server on the test machine)
    let url = std::env::var("TURBODM_TEST_INSTALLER").unwrap_or_else(|_| url.to_string());
    let id = app.manager.add_unlisted(AddRequest {
        url,
        filename: Some(format!("TurboDM-{version}-setup.exe")),
        directory: Some(std::env::temp_dir()),
        ..Default::default()
    });
    *app.updating.borrow_mut() = Some(id.clone());
    progress::open(app, &id);
}

/// The update's installer is here: run it (it asks for admin rights) and quit, so it can replace
/// this version; downloads under way are paused and saved, and the installer reopens TurboDM.
/// No "Download complete" window, as in IDM.
pub fn downloaded(app: &Rc<App>, snap: &Snapshot) {
    app.updating.take();
    progress::close(app, &snap.id);
    app.on_done.borrow_mut().remove(&snap.id);
    match run_installer(&snap.path) {
        Ok(()) => app.quit(),
        Err(err) => dialogs::message(app.main.window.hwnd(), "Couldn't start the update", &err.to_string(), true),
    }
}
