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
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::{HKEY, RegKey};

    /// Registry keys browsers look up hosts under (in HKCU or HKLM), and whether it's Firefox.
    const KEYS: [(&str, bool); 6] = [
        (r"Software\Mozilla\NativeMessagingHosts", true),
        (r"Software\Google\Chrome\NativeMessagingHosts", false),
        (r"Software\Chromium\NativeMessagingHosts", false),
        (r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts", false),
        (r"Software\Microsoft\Edge\NativeMessagingHosts", false),
        (r"Software\Vivaldi\NativeMessagingHosts", false),
    ];

    /// Where the registration lives: for this user, or (installer, as admin) for everyone,
    /// with the manifests next to the installed program.
    struct Scope {
        hive: HKEY,
        dir: PathBuf,
    }

    fn user() -> Scope {
        Scope { hive: HKEY_CURRENT_USER, dir: dirs::data_local_dir().unwrap_or_default().join("TurboDM") }
    }

    fn system() -> std::io::Result<Scope> {
        let program = program()?;
        let install = program.parent().and_then(Path::parent).unwrap_or(Path::new("."));
        Ok(Scope { hive: HKEY_LOCAL_MACHINE, dir: install.join("native-messaging") })
    }

    fn manifest_file(scope: &Scope, firefox: bool) -> PathBuf {
        scope.dir.join(if firefox { "native-host-firefox.json" } else { "native-host-chrome.json" })
    }

    fn add(scope: &Scope) -> std::io::Result<()> {
        let program = program()?;
        for firefox in [true, false] {
            write_if_changed(&manifest_file(scope, firefox), &manifest(&program, firefox))?;
        }
        let hive = RegKey::predef(scope.hive);
        for (key, firefox) in KEYS {
            let (host, _) = hive.create_subkey(format!(r"{key}\{NATIVE_HOST}"))?;
            let file = manifest_file(scope, firefox).to_string_lossy().into_owned();
            if host.get_value::<String, _>("").ok().as_deref() != Some(file.as_str()) {
                host.set_value("", &file)?;
            }
        }
        Ok(())
    }

    fn remove(scope: &Scope) {
        let hive = RegKey::predef(scope.hive);
        for (key, _) in KEYS {
            let _ = hive.delete_subkey_all(format!(r"{key}\{NATIVE_HOST}"));
        }
        for firefox in [true, false] {
            let _ = std::fs::remove_file(manifest_file(scope, firefox));
        }
    }

    /// Installed for everyone (by the installer) as this very program?
    fn installed_for_everyone() -> bool {
        let Ok(scope) = system() else { return false };
        let key = format!(r"{}\{NATIVE_HOST}", KEYS[0].0);
        let registered = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(key)
            .and_then(|k| k.get_value::<String, _>("")).ok();
        registered.is_some_and(|file| Path::new(&file) == manifest_file(&scope, true) && Path::new(&file).exists())
    }

    pub fn register() -> std::io::Result<()> {
        if installed_for_everyone() {
            remove(&user()); // the installer's registration covers everyone: nothing per user
            return Ok(());
        }
        add(&user())
    }

    pub fn unregister() -> std::io::Result<()> {
        remove(&user());
        Ok(())
    }

    pub fn register_system() -> std::io::Result<()> {
        add(&system()?)
    }

    pub fn unregister_system() -> std::io::Result<()> {
        remove(&system()?);
        Ok(())
    }
}

/// Linux reads per-user manifests (packages install system-wide ones themselves).
#[cfg(not(windows))]
pub use platform::{register as register_system, unregister as unregister_system};
pub use platform::{register, unregister};
#[cfg(windows)]
pub use platform::{register_system, unregister_system};

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
