//! The browsers installed here, and the extension TurboDM ships for them.

use std::path::{Path, PathBuf};

pub struct Browser {
    pub name: &'static str,
    pub program: PathBuf,
    pub extensions_page: Option<&'static str>, // Chromium-based: where "Load unpacked" is; None: Firefox
}

/// (name, programs on Linux, program on Windows, extensions page)
const KNOWN: [(&str, &[&str], &str, Option<&str>); 6] = [
    ("Firefox", &["firefox"], "firefox.exe", None),
    ("Google Chrome", &["google-chrome", "google-chrome-stable"], "chrome.exe", Some("chrome://extensions")),
    ("Microsoft Edge", &["microsoft-edge", "microsoft-edge-stable"], "msedge.exe", Some("edge://extensions")),
    ("Brave", &["brave-browser", "brave"], "brave.exe", Some("brave://extensions")),
    ("Chromium", &["chromium", "chromium-browser"], "", Some("chrome://extensions")),
    ("Vivaldi", &["vivaldi", "vivaldi-stable"], "vivaldi.exe", Some("vivaldi://extensions")),
];

pub fn installed() -> Vec<Browser> {
    KNOWN.iter().filter_map(|&(name, commands, exe, extensions_page)| {
        let program = if cfg!(windows) { registered(exe) } else { commands.iter().find_map(|c| in_path(c)) };
        Some(Browser { name, program: program?, extensions_page })
    }).collect()
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
