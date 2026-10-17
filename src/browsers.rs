//! The browsers installed here, and the extension TurboDM ships for them.

use crate::config::{chrome_extension_ids, CHROME_EXTENSION_ID, CHROME_STORE_ID, EDGE_STORE_ID, FIREFOX_EXTENSION_ID};
use std::path::{Path, PathBuf};

pub struct Browser {
    pub name: &'static str,
    pub program: PathBuf,
    pub extensions_page: Option<&'static str>, // Chromium-based: where "Load unpacked" is; None: Firefox
    data: PathBuf,                            // its profiles' folder
}

/// A browser TurboDM knows: its name, programs on Linux, program on Windows, extensions page,
/// and its profiles' folder (Windows: in local app data, Firefox's in roaming; Linux: in home).
struct Known {
    name: &'static str,
    linux: &'static [&'static str],
    exe: &'static str,
    extensions_page: Option<&'static str>,
    windows_data: &'static str,
    linux_data: &'static str,
}

const KNOWN: [Known; 6] = [
    Known { name: "Firefox", linux: &["firefox"], exe: "firefox.exe", extensions_page: None,
            windows_data: r"Mozilla\Firefox\Profiles", linux_data: ".mozilla/firefox" },
    Known { name: "Google Chrome", linux: &["google-chrome", "google-chrome-stable"], exe: "chrome.exe",
            extensions_page: Some("chrome://extensions"), windows_data: r"Google\Chrome\User Data", linux_data: ".config/google-chrome" },
    Known { name: "Microsoft Edge", linux: &["microsoft-edge", "microsoft-edge-stable"], exe: "msedge.exe",
            extensions_page: Some("edge://extensions"), windows_data: r"Microsoft\Edge\User Data", linux_data: ".config/microsoft-edge" },
    Known { name: "Brave", linux: &["brave-browser", "brave"], exe: "brave.exe", extensions_page: Some("brave://extensions"),
            windows_data: r"BraveSoftware\Brave-Browser\User Data", linux_data: ".config/BraveSoftware/Brave-Browser" },
    Known { name: "Chromium", linux: &["chromium", "chromium-browser"], exe: "", extensions_page: Some("chrome://extensions"),
            windows_data: r"Chromium\User Data", linux_data: ".config/chromium" },
    Known { name: "Vivaldi", linux: &["vivaldi", "vivaldi-stable"], exe: "vivaldi.exe", extensions_page: Some("vivaldi://extensions"),
            windows_data: r"Vivaldi\User Data", linux_data: ".config/vivaldi" },
];

pub fn installed() -> Vec<Browser> {
    KNOWN.iter().filter_map(|known| {
        let program = if cfg!(windows) { registered(known.exe) } else { known.linux.iter().find_map(|c| in_path(c)) };
        let data = if cfg!(windows) {
            let base = if known.extensions_page.is_none() { dirs::data_dir() } else { dirs::data_local_dir() };
            base.unwrap_or_default().join(known.windows_data)
        } else {
            dirs::home_dir().unwrap_or_default().join(known.linux_data)
        };
        Some(Browser { name: known.name, program: program?, extensions_page: known.extensions_page, data })
    }).collect()
}

impl Browser {
    pub fn is_firefox(&self) -> bool {
        self.extensions_page.is_none()
    }

    /// TurboDM's page in the store this browser adds extensions from, once it's listed there.
    pub fn store_page(&self) -> Option<String> {
        match (self.is_firefox(), self.name == "Microsoft Edge" && EDGE_STORE_ID.is_some()) {
            (true, _) => None,
            (_, true) => EDGE_STORE_ID.map(|id| format!("https://microsoftedge.microsoft.com/addons/detail/{id}")),
            _ => CHROME_STORE_ID.map(|id| format!("https://chromewebstore.google.com/detail/{id}")),
        }
    }

    /// Is TurboDM's extension in one of its profiles, and on?
    pub fn has_extension(&self) -> bool {
        self.has_one_of(&chrome_extension_ids())
    }

    /// Chromium-based: is the full extension (added by hand: it has YouTube, which the stores'
    /// copy leaves out) in one of its profiles, and on?
    pub fn has_full_extension(&self) -> bool {
        !self.is_firefox() && self.has_one_of(&[CHROME_EXTENSION_ID])
    }

    fn has_one_of(&self, ids: &[&str]) -> bool {
        let Ok(profiles) = std::fs::read_dir(&self.data) else { return false };
        let profiles: Vec<PathBuf> = profiles.flatten().map(|entry| entry.path()).filter(|p| p.is_dir()).collect();
        if self.is_firefox() {
            return profiles.iter().any(|p| p.join("extensions").join(format!("{FIREFOX_EXTENSION_ID}.xpi")).is_file());
        }
        profiles.iter().any(|profile| ["Secure Preferences", "Preferences"].iter().any(|file| {
            std::fs::read_to_string(profile.join(file)).is_ok_and(|text| enabled_in(&text, ids))
        }))
    }
}

/// Does a Chromium profile's preferences have one of these extensions, not turned off?
fn enabled_in(preferences: &str, ids: &[&str]) -> bool {
    let Ok(prefs) = serde_json::from_str::<serde_json::Value>(preferences) else { return false };
    ids.iter().any(|id| {
        let Some(extension) = prefs.pointer(&format!("/extensions/settings/{id}")) else { return false };
        // (why it's off: a number once, a list now; added from outside and not yet turned on, too)
        match extension.get("disable_reasons") {
            Some(serde_json::Value::Array(reasons)) => reasons.is_empty(),
            Some(serde_json::Value::Number(reasons)) => reasons.as_u64() == Some(0),
            _ => extension.get("state").and_then(|s| s.as_u64()) != Some(0),
        }
    })
}

fn in_path(command: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(command)).find(|p| p.is_file())
}

/// Windows: browsers register their program under "App Paths".
#[cfg(windows)]
fn registered(exe: &str) -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    if exe.is_empty() {
        return None;
    }
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER].into_iter().find_map(|hive| {
        let path: String = RegKey::predef(hive).open_subkey(&key).ok()?.get_value("").ok()?;
        Some(PathBuf::from(path.trim_matches('"'))).filter(|p| p.is_file())
    })
}

#[cfg(not(windows))]
fn registered(_exe: &str) -> Option<PathBuf> {
    None
}

/// The folder with the extension (chrome/, firefox/, turbodm-firefox.xpi): next to the
/// program on Windows, in share/turbodm on Linux.
pub fn extension_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let prefix = exe.parent()?.parent()?;
    [prefix.join("extension"), prefix.join("share/turbodm/extension"), crate::config::data_dir().join("extension")]
        .into_iter().find(|dir| dir.join("chrome").is_dir())
}

/// The Mozilla-signed Firefox extension, which Firefox offers to add when it's opened.
pub fn firefox_extension(dir: &Path) -> Option<PathBuf> {
    Some(dir.join("turbodm-firefox.xpi")).filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_whether_the_extension_is_on() {
        let prefs = |settings: &str| format!(r#"{{"extensions":{{"settings":{{{settings}}}}}}}"#);
        let id = CHROME_EXTENSION_ID;
        assert!(enabled_in(&prefs(&format!(r#""{id}":{{"location":4}}"#)), &chrome_extension_ids()));
        assert!(enabled_in(&prefs(&format!(r#""{id}":{{"disable_reasons":[]}}"#)), &chrome_extension_ids()));
        assert!(!enabled_in(&prefs(&format!(r#""{id}":{{"disable_reasons":[8192]}}"#)), &chrome_extension_ids()));
        assert!(!enabled_in(&prefs(&format!(r#""{id}":{{"disable_reasons":1}}"#)), &chrome_extension_ids()));
        assert!(!enabled_in(&prefs(&format!(r#""{id}":{{"state":0}}"#)), &chrome_extension_ids()));
        assert!(!enabled_in(&prefs(r#""ngpampappnmepgilojfohadhhmbhlaek":{}"#), &chrome_extension_ids()));
        assert!(!enabled_in("not json", &chrome_extension_ids()));
        // the full extension, told apart from the stores' copy
        let store = crate::config::CHROME_STORE_ID.unwrap();
        assert!(!enabled_in(&prefs(&format!(r#""{store}":{{"location":1}}"#)), &[CHROME_EXTENSION_ID]));
        assert!(enabled_in(&prefs(&format!(r#""{store}":{{"location":1}}"#)), &chrome_extension_ids()));
    }
}
