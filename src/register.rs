//! Registering TurboDM as the browsers' native-messaging host, so the extension can reach it.
//! Done on every start (cheap: files are only written when they change), so it keeps working
//! whichever way TurboDM was installed or moved — a package, install.sh, an AppImage, the
//! Windows installer — and by `turbodm --register` / `--unregister` from installers.

use crate::config::{CHROME_EXTENSION_ID, FIREFOX_EXTENSION_ID, NATIVE_HOST};
use serde_json::json;
use std::path::{Path, PathBuf};

/// The program browsers should start: this one (or the AppImage it runs from).
fn program() -> std::io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(appimage) => Ok(PathBuf::from(appimage)),
        None => std::env::current_exe(),
    }
}

fn manifest(path: &Path, firefox: bool) -> String {
    let mut value = json!({
        "name": NATIVE_HOST,
        "description": "TurboDM",
        "path": path,
        "type": "stdio",
    });
    if firefox {
        value["allowed_extensions"] = json!([FIREFOX_EXTENSION_ID]);
    } else {
        value["allowed_origins"] = json!([format!("chrome-extension://{CHROME_EXTENSION_ID}/")]);
    }
    serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
}

/// Write `content` unless the file already holds exactly that.
fn write_if_changed(file: &Path, content: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(file).is_ok_and(|old| old == content) {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, content)
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    /// (manifest directory, is Firefox) for every browser that's set up for this user.
    fn targets() -> Vec<(PathBuf, bool)> {
        let home = dirs::home_dir().unwrap_or_default();
        let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        let mut out = vec![(home.join(".mozilla/native-messaging-hosts"), true)];
        for dir in ["google-chrome", "google-chrome-beta", "google-chrome-unstable", "chromium",
                    "BraveSoftware/Brave-Browser", "microsoft-edge", "vivaldi", "opera"] {
            if config.join(dir).is_dir() {
                out.push((config.join(dir).join("NativeMessagingHosts"), false));
            }
        }
        out
    }

    pub fn register() -> std::io::Result<()> {
        let program = program()?;
        for (dir, firefox) in targets() {
            write_if_changed(&dir.join(format!("{NATIVE_HOST}.json")), &manifest(&program, firefox))?;
        }
        Ok(())
    }

    pub fn unregister() -> std::io::Result<()> {
        for (dir, _) in targets() {
            let _ = std::fs::remove_file(dir.join(format!("{NATIVE_HOST}.json")));
        }
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    /// Registry keys browsers look up hosts under (HKCU), and whether it's Firefox.
    const KEYS: [(&str, bool); 6] = [
        (r"Software\Mozilla\NativeMessagingHosts", true),
        (r"Software\Google\Chrome\NativeMessagingHosts", false),
        (r"Software\Chromium\NativeMessagingHosts", false),
        (r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts", false),
        (r"Software\Microsoft\Edge\NativeMessagingHosts", false),
        (r"Software\Vivaldi\NativeMessagingHosts", false),
    ];

    fn manifest_file(firefox: bool) -> PathBuf {
        let dir = dirs::data_local_dir().unwrap_or_default().join("TurboDM");
        dir.join(if firefox { "native-host-firefox.json" } else { "native-host-chrome.json" })
    }

    pub fn register() -> std::io::Result<()> {
        let program = program()?;
        for firefox in [true, false] {
            write_if_changed(&manifest_file(firefox), &manifest(&program, firefox))?;
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for (key, firefox) in KEYS {
            let (host, _) = hkcu.create_subkey(format!(r"{key}\{NATIVE_HOST}"))?;
            let file = manifest_file(firefox).to_string_lossy().into_owned();
            if host.get_value::<String, _>("").ok().as_deref() != Some(file.as_str()) {
                host.set_value("", &file)?;
            }
        }
        Ok(())
    }

    pub fn unregister() -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for (key, _) in KEYS {
            let _ = hkcu.delete_subkey_all(format!(r"{key}\{NATIVE_HOST}"));
        }
        for firefox in [true, false] {
            let _ = std::fs::remove_file(manifest_file(firefox));
        }
        Ok(())
    }
}

pub use platform::{register, unregister};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_name_the_extension() {
        let firefox: serde_json::Value = serde_json::from_str(&manifest(Path::new("/usr/bin/turbodm"), true)).unwrap();
        assert_eq!(firefox["allowed_extensions"][0], FIREFOX_EXTENSION_ID);
        assert_eq!(firefox["path"], "/usr/bin/turbodm");
        let chrome: serde_json::Value = serde_json::from_str(&manifest(Path::new("/x"), false)).unwrap();
        assert_eq!(chrome["allowed_origins"][0], format!("chrome-extension://{CHROME_EXTENSION_ID}/"));
    }
}
