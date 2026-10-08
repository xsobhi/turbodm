//! "A new version is available", like IDM: checked at start and once a day. The new installer
//! is downloaded and run; it replaces this version and opens the new one.

use super::wnd::{self, enable, set_text, Rows, Window, LINE};
use super::{post, App};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use turbodm::update::{self, installed_by_setup, run_installer, RELEASES_PAGE};
use windows::Win32::Foundation::HWND;
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
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let window = Window::new("TurboDM update", style, WINDOW_EX_STYLE(0), None, (480, 200));
    let heading = window.heading(&format!("TurboDM {} is available", release.version));
    let text = window.note(&body.replace('\n', "\r\n"));
    let status = window.label("");
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
    let (a, w, button) = (app.clone(), window.clone(), Rc::new(std::cell::Cell::new(HWND::default())));
    let b = button.clone();
    let go = window.default_button(if can_install { "Update now" } else { "Download" }, move || {
        match release.installer.clone().filter(|_| can_install) {
            Some(url) => install(&a, &url, &release.version, b.get(), status),
            None => {
                let page = if release.page.is_empty() { RELEASES_PAGE } else { &release.page };
                super::shell::open(std::path::Path::new(page));
                w.destroy();
            }
        }
    });
    button.set(go);
    let mut rows = Rows::new(16, 14, 0, 448);
    rows.full(heading, 24);
    rows.full(text, lines * LINE);
    rows.full(status, LINE);
    let bottom = rows.buttons(&[skip, later, go]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}

/// Fetch the installer, run it (it asks for admin rights), and quit so it can replace us.
fn install(app: &Rc<App>, url: &str, version: &str, button: HWND, status: HWND) {
    set_text(status, "Downloading the update…");
    enable(button, false);
    let dest: PathBuf = std::env::temp_dir().join(format!("TurboDM-{version}-setup.exe"));
    let (client, url) = (app.manager.shared.client(), url.to_string());
    let local = std::env::var_os("TURBODM_TEST_INSTALLER"); // tests the update with a fresh installer (CI)
    let (status_id, button_id) = (status.0 as isize, button.0 as isize);
    app.rt.spawn(async move {
        let fetched = match local {
            Some(path) => std::fs::copy(path, &dest).map(|_| ()).map_err(|e| e.to_string()),
            None => update::download(&client, &url, &dest).await,
        };
        post(move |app| {
            let status = HWND(status_id as *mut _);
            match fetched.map_err(|e| format!("Couldn't download the update: {e}"))
                .and_then(|_| run_installer(&dest).map_err(|e| format!("Couldn't start the update: {e}"))) {
                Ok(()) => app.quit(), // downloads are paused and saved; the installer reopens TurboDM
                Err(err) => {
                    set_text(status, &err);
                    enable(HWND(button_id as *mut _), true);
                }
            }
        });
    });
}
