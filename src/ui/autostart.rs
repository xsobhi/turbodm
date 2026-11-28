//! "Start when you log in" (Options → General, on by default), as "Start with Windows" there:
//! TurboDM waits in the notification area from login, with no window, like IDM (`turbodm
//! --tray`), through the desktop's autostart folder. Only the installed copy (the package's, in
//! /usr) signs itself in, so a test copy never takes the entry over; switching it off removes it.

use std::path::PathBuf;

fn entry() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("autostart").join("turbodm.desktop"))
}

/// The login entry, made to match the setting.
pub fn sync(on: bool) {
    let Some(file) = entry() else { return };
    let installed = std::env::current_exe().is_ok_and(|exe| exe.starts_with("/usr") || exe.starts_with("/opt"));
    if !on {
        let _ = std::fs::remove_file(&file);
        return;
    }
    if !installed {
        return;
    }
    let content = format!("[Desktop Entry]\nType=Application\nName={}\nComment=Waits in the notification area for \
                           downloads\nExec=turbodm --tray\nIcon=turbodm\nTerminal=false\nNoDisplay=true\n\
                           X-GNOME-Autostart-enabled=true\n", turbodm::config::APP_NAME);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(content.as_str()) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&file, content);
    }
}
