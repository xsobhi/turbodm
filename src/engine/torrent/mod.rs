//! Torrents (magnet links and .torrent files) as one more kind of download, on librqbit.
//!
//! A torrent is a `Task` like any other: listed and saved with the others, paused, resumed and
//! removed the same way, with its runner (`run.rs`) keeping the task's numbers up to date. One
//! librqbit session serves them all (`session.rs`), started the first time a torrent needs it.
//! It keeps its own record of its torrents and of the pieces already on disk ("fast resume") in
//! the `torrents` folder beside the download list, so an unfinished torrent carries on after a
//! restart without checking every piece again.

mod run;
mod session;
mod source;

pub use run::{remove, run, share};
pub use session::Torrents;

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// What's saved about a torrent, besides what every download has.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TorrentMeta {
    /// Its info hash (hex), once known: how the session knows it.
    #[serde(default)]
    pub info_hash: Option<String>,
    /// How many files it has (0 until known). With more than one, the download's name is the
    /// folder they go in; with one, it's that file.
    #[serde(default)]
    pub files: usize,
    /// Given to the session at least once: what's at its path is its own data.
    #[serde(default)]
    pub added: bool,
    /// Bytes downloaded and checked, shown while the session isn't running.
    #[serde(default)]
    pub have: u64,
}

impl TorrentMeta {
    pub fn multi_file(&self) -> bool {
        self.files > 1
    }
}

/// One file of a torrent, for the progress window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TorrentFile {
    pub name: String, // inside the torrent's folder
    pub offset: u64,  // where it starts, counting the torrent as one run of bytes
    pub size: u64,
    pub have: u64,
}

/// A torrent's live numbers, besides what every download has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TorrentView {
    pub peers: usize,       // connected now
    pub known_peers: usize, // found so far (trackers, DHT, peers' tips)
    pub uploaded: u64,      // shared with others while it ran
    pub checking: bool,     // checking the pieces already on disk
    pub seeding: bool,      // complete and still shared ("seed" setting)
    pub files: Arc<Vec<TorrentFile>>,
    /// Bytes received from peers while it runs (unchecked): what the speed goes by, as the
    /// checked total moves in whole pieces.
    pub fetched: u64,
}

/// A magnet link, a link to a .torrent file, or a .torrent file on this computer.
/// `filename`: the name a browser gave the download, if any (links to .torrent files don't
/// always end in .torrent).
pub fn is_torrent(url: &str, filename: Option<&str>) -> bool {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let torrent_name = |name: &str| name.to_ascii_lowercase().ends_with(".torrent");
    if magnet_query(url).is_some() {
        return true;
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let path = lower.split(['?', '#']).next().unwrap_or("");
        return path.ends_with(".torrent") || filename.is_some_and(torrent_name);
    }
    !lower.contains("://") && torrent_name(url)
}

/// A magnet link (for the clipboard monitor: a .torrent link is caught as a file link).
pub fn is_magnet(text: &str) -> bool {
    magnet_query(text).is_some() && !text.contains(char::is_whitespace)
}

/// What follows "magnet:?" (in any case).
fn magnet_query(url: &str) -> Option<&str> {
    url.get(..8).filter(|start| start.eq_ignore_ascii_case("magnet:?")).map(|_| &url[8..])
}

/// The name a magnet link gives its torrent (`dn=`), to show until the torrent's own arrives.
pub fn name_hint(url: &str) -> Option<String> {
    let value = magnet_query(url)?.split('&').find_map(|pair| pair.strip_prefix("dn="))?;
    let name = crate::util::percent_decode(&value.replace('+', " "));
    let name = crate::util::sanitize_filename(&name);
    (name != "download").then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_torrents_from_other_links() {
        assert!(is_torrent("magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c", None));
        assert!(is_torrent("https://x.org/a/ubuntu.iso.torrent?key=1", None));
        assert!(is_torrent("https://x.org/download.php?id=7", Some("Big Buck Bunny.torrent")));
        assert!(is_torrent(r"C:\Users\me\Downloads\sintel.torrent", None));
        assert!(is_torrent("/home/me/sintel.TORRENT", None));
        assert!(!is_torrent("https://x.org/file.zip", Some("file.zip")));
        assert!(!is_torrent("ftp://x.org/a.torrent", None));
        assert!(is_magnet("magnet:?xt=urn:btih:abc"));
        assert!(!is_magnet("magnet:?xt=urn:btih:abc and more"));
    }

    #[test]
    fn magnet_names() {
        let url = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel&tr=udp%3A%2F%2Fx";
        assert_eq!(name_hint(url).as_deref(), Some("Sintel"));
        assert_eq!(name_hint("magnet:?dn=Big+Buck%20Bunny&xt=urn:btih:abc").as_deref(), Some("Big Buck Bunny"));
        assert_eq!(name_hint("magnet:?dn=a%2Fb").as_deref(), Some("a_b"));
        assert_eq!(name_hint("magnet:?xt=urn:btih:abc"), None);
        assert_eq!(name_hint("https://x.org/a.torrent"), None);
    }
}
