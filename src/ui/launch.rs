//! Handing files to the desktop: open, open with…, show in folder.

use super::Ctx;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::path::Path;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub enum Launch {
    Open,
    OpenWith, // ask which app
    ShowInFolder,
}

/// Hand a file to the desktop. Counted in `ctx.launching` until the desktop has it: quitting
/// earlier drops the request (and closes an "Open with" chooser that's still up).
pub fn launch(ctx: &Rc<Ctx>, path: &Path, how: Launch) {
    let (c, file) = (ctx.clone(), gio::File::for_path(path));
    ctx.launching.set(ctx.launching.get() + 1);
    glib::spawn_future_local(async move {
        let result = match how {
            Launch::Open => open_default(&file).await,
            Launch::ShowInFolder => match show_in_folder(&file).await {
                // no file manager answers ShowItems: just open the folder
                Err(_) => open_default(&file.parent().unwrap_or_else(|| file.clone())).await,
                ok => ok,
            },
            Launch::OpenWith => open_with(&c, &file).await,
        };
        c.launching.set(c.launching.get() - 1);
        match result {
            Err(err) if !err.matches(gtk::DialogError::Dismissed) => {
                eprintln!("turbodm: open {}: {err}", file.parse_name())
            }
            _ => {}
        }
    });
}

/// With the default app, like a double-click in the file manager. (GtkFileLauncher goes through
/// the desktop portal, which keeps asking which app to use for types with several apps.)
async fn open_default(file: &gio::File) -> Result<(), glib::Error> {
    let context = gtk::gdk::Display::default().map(|d| d.app_launch_context());
    gio::AppInfo::launch_default_for_uri_future(&file.uri(), context.as_ref()).await
}

/// Open Explorer with the file selected.
#[cfg(windows)]
async fn show_in_folder(file: &gio::File) -> Result<(), glib::Error> {
    let path = file.path().unwrap_or_default();
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(path.as_os_str());
    std::process::Command::new("explorer.exe").arg(arg).spawn().map(|_| ())
        .map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e.to_string()))
}

/// Windows' own "Open with" chooser.
#[cfg(windows)]
async fn open_with(_ctx: &Ctx, file: &gio::File) -> Result<(), glib::Error> {
    let path = file.path().unwrap_or_default();
    std::process::Command::new("rundll32.exe").arg("shell32.dll,OpenAs_RunDLL").arg(path).spawn()
        .map(|_| ()).map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e.to_string()))
}

/// The desktop portal's app chooser.
#[cfg(not(windows))]
async fn open_with(ctx: &Ctx, file: &gio::File) -> Result<(), glib::Error> {
    let launcher = gtk::FileLauncher::new(Some(file));
    launcher.set_always_ask(true);
    // GTK 4.14 crashes on a parent that was never shown, like the main window when
    // the browser started TurboDM in the background
    launcher.launch_future(Some(&ctx.win.window).filter(|w| w.is_realized())).await
}

/// Open the folder with the file selected (Nemo, Nautilus, Dolphin, Caja, Thunar…).
#[cfg(not(windows))]
async fn show_in_folder(file: &gio::File) -> Result<(), glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::Session).await?;
    let args = (vec![file.uri().to_string()], "").to_variant(); // (uris, startup id)
    bus.call_future(Some("org.freedesktop.FileManager1"), "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1", "ShowItems", Some(&args), None,
                    gio::DBusCallFlags::NONE, -1).await.map(|_| ())
}
