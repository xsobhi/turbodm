//! "A new version is available", like IDM: checked at start and once a day. On Windows the new
//! installer is downloaded and run; Linux packages update through apt instead.

use super::Ctx;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use turbodm::update::{self, installed_by_setup, run_installer, updated_by_apt, RELEASES_PAGE};

pub fn start(ctx: &Rc<Ctx>) {
    let weak = Rc::downgrade(ctx);
    glib::timeout_add_seconds_local(3, move || {
        let Some(ctx) = weak.upgrade() else { return glib::ControlFlow::Break };
        check(&ctx, false);
        let weak = Rc::downgrade(&ctx);
        glib::timeout_add_local(Duration::from_secs(24 * 3600), move || match weak.upgrade() {
            Some(ctx) => { check(&ctx, false); glib::ControlFlow::Continue }
            None => glib::ControlFlow::Break,
        });
        glib::ControlFlow::Break
    });
}

/// Ask GitHub. `manual`: from the menu, so also say when there's nothing new.
pub fn check(ctx: &Rc<Ctx>, manual: bool) {
    let settings = ctx.manager.settings();
    if !manual && (!settings.check_updates || updated_by_apt()) {
        return;
    }
    let client = ctx.manager.shared.client();
    let job = ctx.rt.spawn(async move { update::check(&client).await });
    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        let found = job.await.unwrap_or_else(|e| Err(e.to_string()));
        match found {
            Ok(Some(release)) if manual || release.version != settings.skipped_update => {
                *ctx.update.borrow_mut() = Some(release);
                if manual || ctx.win.window.is_visible() {
                    show_pending(&ctx);
                }
            }
            Ok(_) if manual => tell(&ctx, "TurboDM is up to date", &format!("You have the latest version, {}.", turbodm::config::VERSION), false),
            Err(err) if manual => tell(&ctx, "Couldn't check for updates", &err, true),
            _ => {}
        }
    });
}

fn tell(ctx: &Ctx, title: &str, detail: &str, error: bool) {
    super::frame::message(Some(ctx.win.window.upcast_ref()), title, detail, error);
}

/// Show the update found earlier (it waits while the window is hidden in the tray).
pub fn show_pending(ctx: &Rc<Ctx>) {
    let Some(release) = ctx.update.borrow_mut().take() else { return };
    let can_install = installed_by_setup() && release.installer.is_some();
    let mut text = format!("You have version {}.", turbodm::config::VERSION);
    if !release.notes.is_empty() {
        text.push_str(&format!("\n\nWhat's new:\n{}", release.notes));
    }
    if updated_by_apt() {
        text.push_str("\n\nIt will arrive with your system updates.");
    }
    let d = super::frame::dialog_with(&format!("{} update", turbodm::config::APP_NAME), None, 480, ":close", (14, 16));
    d.body.set_margin_bottom(14);
    d.body.append(&super::frame::heading(&format!("{} {} is available", turbodm::config::APP_NAME, release.version)));
    let body = super::frame::note(&text);
    body.set_margin_top(super::frame::LINE_GAP);
    d.body.append(&body);
    let status = gtk::Label::builder().xalign(0.0).css_classes(["dim-label"]).build();
    d.body.append(&status);
    let skip = super::frame::button("Skip this version");
    let later = super::frame::button("Later");
    let go = super::frame::default_button(&d.window, if can_install { "Update now" } else { "Download" });
    super::frame::buttons(&d.body, &[&skip, &later, &go], 12);
    let window = d.window.clone();
    let w = window.clone();
    later.connect_clicked(move |_| w.close());
    let (c, w, version) = (ctx.clone(), window.clone(), release.version.clone());
    skip.connect_clicked(move |_| {
        let mut s = c.manager.settings();
        s.skipped_update = version.clone();
        c.manager.update_settings(s);
        w.close();
    });
    let (c, w) = (ctx.clone(), window.clone());
    go.connect_clicked(move |button| {
        match release.installer.clone().filter(|_| can_install) {
            Some(url) => install(&c, &url, &release.version, button, &status),
            None => {
                let page = if release.page.is_empty() { RELEASES_PAGE } else { &release.page };
                let _ = gio::AppInfo::launch_default_for_uri(page, None::<&gio::AppLaunchContext>);
                w.close();
            }
        }
    });
    window.present(); // (Enter: the main action, not "Skip this version")
}

/// Windows: fetch the installer, run it (it asks for admin rights, replaces this version and
/// opens the new one), and quit so it can.
fn install(ctx: &Rc<Ctx>, url: &str, version: &str, button: &gtk::Button, status: &gtk::Label) {
    button.set_sensitive(false);
    status.set_text("Downloading the update…");
    let dest: PathBuf = std::env::temp_dir().join(format!("TurboDM-{version}-setup.exe"));
    let (client, url, file) = (ctx.manager.shared.client(), url.to_string(), dest.clone());
    // TURBODM_TEST_INSTALLER: tests the update with a freshly built installer (CI)
    let local = std::env::var_os("TURBODM_TEST_INSTALLER");
    let job = ctx.rt.spawn(async move {
        match local {
            Some(path) => std::fs::copy(path, &file).map(|_| ()).map_err(|e| e.to_string()),
            None => update::download(&client, &url, &file).await,
        }
    });
    let (ctx, button, status) = (ctx.clone(), button.clone(), status.clone());
    glib::spawn_future_local(async move {
        if let Err(err) = job.await.unwrap_or_else(|e| Err(e.to_string())) {
            status.set_text(&format!("Couldn't download the update: {err}"));
            button.set_sensitive(true);
            return;
        }
        if let Err(err) = run_installer(&dest) {
            status.set_text(&format!("Couldn't start the update: {err}"));
            button.set_sensitive(true);
            return;
        }
        ctx.app.quit(); // downloads are paused and saved; the installer reopens TurboDM
    });
}
