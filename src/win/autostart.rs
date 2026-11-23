//! "Start with Windows" (Options, on by default): TurboDM waits in the notification area from
//! sign-in, with no window, like IDM (`turbodm --tray`). Only the installed copy signs itself in,
//! so a portable or test copy never takes the entry over; switching it off removes it from any copy.

use winreg::{enums::HKEY_CURRENT_USER, RegKey};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "TurboDM";

/// The sign-in entry, made to match the setting (and this copy's path, after an update or a move).
pub fn sync(on: bool) {
    if on && !turbodm::update::installed_by_setup() {
        return;
    }
    let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN) else { return };
    let _ = match std::env::current_exe() {
        Ok(exe) if on => key.set_value(NAME, &format!("\"{}\" --tray", exe.display())),
        _ => key.delete_value(NAME),
    };
}
