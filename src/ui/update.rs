//! "A new version is available", like IDM: checked at start and once a day. On Windows the new
//! installer is downloaded and run; Linux packages update through apt instead.

use super::{center, Ctx};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use turbodm::update::{self, RELEASES_PAGE};

/// Installed from the .deb, which added the apt repository: updates come with the system's.
fn updated_by_apt() -> bool {
    cfg!(target_os = "linux") && std::env::current_exe().is_ok_and(|p| p.starts_with("/usr"))
        && std::path::Path::new("/etc/apt/sources.list.d/turbodm.sources").exists()
}

/// Installed by our Windows installer (not the portable zip): it can install over itself.
fn installed_by_setup() -> bool {
    cfg!(windows) && std::env::current_exe().ok().and_then(|p| Some(p.parent()?.parent()?.join("unins000.exe")))
        .is_some_and(|p| p.exists())
}

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
            Ok(_) if manual => tell(&ctx, "TurboDM is up to date", &format!("You have the latest version, {}.", turbodm::config::VERSION)),
            Err(err) if manual => tell(&ctx, "Couldn't check for updates", &err),
            _ => {}
        }
    });
}

fn tell(ctx: &Ctx, message: &str, detail: &str) {
    let dialog = gtk::AlertDialog::builder().message(message).detail(detail).build();
    dialog.show(Some(&ctx.win.window).filter(|w| w.is_visible()));
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
    let heading = gtk::Label::builder().label(format!("TurboDM {} is available", release.version))
        .xalign(0.0).css_classes(["title-3"]).build();
    let body = gtk::Label::builder().label(&text).xalign(0.0).wrap(true).max_width_chars(60).build();
    let status = gtk::Label::builder().xalign(0.0).css_classes(["dim-label"]).build();
    let skip = gtk::Button::with_label("Skip this version");
    let later = gtk::Button::with_label("Later");
    let go = gtk::Button::builder().label(if can_install { "Update now" } else { "Download" })
        .css_classes(["suggested-action"]).build();
    let buttons = gtk::Box::builder().spacing(8).halign(gtk::Align::End).build();
    for b in [&skip, &later, &go] {
        buttons.append(b);
    }
    let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12)
        .margin_top(18).margin_bottom(18).margin_start(18).margin_end(18).build();
    for w in [heading.upcast_ref::<gtk::Widget>(), body.upcast_ref(), status.upcast_ref(), buttons.upcast_ref()] {
        content.append(w);
    }
    let window = gtk::Window::builder().title("TurboDM update").default_width(460).child(&content).build();
    center::on_screen(&window, false);
    window.set_default_widget(Some(&go));

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
    window.present();
}

/// Windows: fetch the installer, run it (it asks for admin rights, replaces this version and
/// opens the new one), and quit so it can.
fn install(ctx: &Rc<Ctx>, url: &str, version: &str, button: &gtk::Button, status: &gtk::Label) {
    button.set_sensitive(false);
    status.set_text("Downloading the update…");
    let dest: PathBuf = std::env::temp_dir().join(format!("TurboDM-{version}-setup.exe"));
    let (client, url, file) = (ctx.manager.shared.client(), url.to_string(), dest.clone());
    let job = ctx.rt.spawn(async move { update::download(&client, &url, &file).await });
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

/// The installer needs admin rights: `start` goes through the shell, which shows the prompt.
fn run_installer(setup: &std::path::Path) -> std::io::Result<()> {
    let mut command = std::process::Command::new("cmd");
    command.args(["/C", "start", ""]).arg(setup).args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000); // no console
    command.spawn().map(|_| ())
}
