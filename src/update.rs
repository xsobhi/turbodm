//! Is there a newer TurboDM? Asks GitHub for the latest release (at start and once a day, unless
//! turned off in Preferences) and fetches the Windows installer when the user wants to update.

use crate::config::{own_user_agent, VERSION};
use serde::Deserialize;
use std::path::Path;

const LATEST: &str = "https://api.github.com/repos/xsobhi/turbodm/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/xsobhi/turbodm/releases/latest";
const INSTALLER_SUFFIX: &str = "-windows-x64-setup.exe";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub notes: String,             // what's new, as plain text
    pub installer: Option<String>, // download address of the Windows installer
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

fn numbers(version: &str) -> Option<Vec<u64>> {
    version.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect()
}

/// `candidate` is a later version than `current` ("1.10.0" > "1.9.2").
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((numbers(candidate), numbers(current)), (Some(a), Some(b)) if a > b)
}

/// The "What's new" part of release notes (before "## Install"), without Markdown.
fn whats_new(body: &str) -> String {
    body.lines()
        .take_while(|l| !l.starts_with("## ") || l.starts_with("## What"))
        .filter(|l| !l.starts_with("## "))
        .map(|l| l.replace("**", "").replace('`', ""))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

async fn get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, String> {
    client.get(url).header("User-Agent", own_user_agent()).header("Accept", "application/vnd.github+json")
        .send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())
}

/// The latest release, if it's newer than this TurboDM.
pub async fn check(client: &reqwest::Client) -> Result<Option<Release>, String> {
    let bytes = get(client, LATEST).await?.bytes().await.map_err(|e| e.to_string())?;
    let api: ApiRelease = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !is_newer(&api.tag_name, VERSION) {
        return Ok(None);
    }
    Ok(Some(Release {
        version: api.tag_name.trim_start_matches('v').to_string(),
        page: api.html_url,
        notes: whats_new(api.body.as_deref().unwrap_or_default()),
        installer: api.assets.into_iter().find(|a| a.name.ends_with(INSTALLER_SUFFIX))
            .map(|a| a.browser_download_url),
    }))
}

/// Save the installer to `dest`.
pub async fn download(client: &reqwest::Client, url: &str, dest: &Path) -> Result<(), String> {
    let bytes = get(client, url).await?.bytes().await.map_err(|e| e.to_string())?;
    std::fs::write(dest, &bytes).map_err(|e| format!("Cannot save the update: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_and_reads_notes() {
        assert!(is_newer("v1.10.0", "1.9.2") && is_newer("1.4.1", "1.4.0"));
        assert!(!is_newer("v1.4.0", "1.4.0") && !is_newer("1.3.9", "1.4.0") && !is_newer("nightly", "1.0.0"));
        let body = "## What's new\n\n- **Faster** `x`\n- More\n\n## Install\nstuff";
        assert_eq!(whats_new(body), "- Faster x\n- More");
        assert_eq!(whats_new("- Fixed\n\n## Install\nstuff"), "- Fixed");
    }
}
