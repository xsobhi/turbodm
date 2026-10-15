//! Words shown for downloads, the same in the Linux and Windows interfaces.

use crate::categories::{category_for, is_downloadable};
use crate::engine::segments::Segment;
use crate::engine::torrent::{is_magnet, is_torrent};
use crate::engine::{Snapshot, Status};
use crate::util::{filename_from_url, human_eta, human_size, human_speed};

/// A download's status column: progress while it runs, why it stopped otherwise.
pub fn status_text(snap: &Snapshot) -> String {
    let pct = snap.progress.map(|p| format!("{:.1}%", p * 100.0));
    match snap.status {
        Status::Downloading => pct.unwrap_or_else(|| human_size(Some(snap.downloaded))),
        Status::Paused => pct.map_or("Paused".into(), |p| format!("Paused · {p}")),
        Status::Error => format!("Error: {}", snap.error.as_deref().unwrap_or("unknown")),
        Status::Completed if snap.torrent.as_ref().is_some_and(|t| t.seeding) => "Complete · sharing".into(),
        Status::Connecting if snap.torrent.is_some() => torrent_stage(snap).into(),
        other => other.label().into(),
    }
}

/// The progress window's "Status" line.
pub fn status_line(s: &Snapshot) -> String {
    match (s.status, &s.error) {
        (Status::Error, Some(e)) => format!("Error: {e}"),
        (Status::Downloading, _) if s.torrent.is_some() => "Receiving data from peers…".into(),
        (Status::Downloading, _) => "Receiving data…".into(),
        (Status::Connecting, _) if s.torrent.is_some() => torrent_stage(s).into(),
        (status, _) => status.label().into(),
    }
}

/// What a torrent that isn't downloading yet is busy with.
fn torrent_stage(s: &Snapshot) -> &'static str {
    match (&s.torrent, s.size) {
        (Some(t), _) if t.checking => "Checking what's on disk…",
        (_, None) if is_magnet(&s.url) => "Getting the torrent's details from peers…",
        (_, None) => "Reading the torrent file…",
        _ => "Looking for peers…",
    }
}

/// A torrent's peers, while it runs: "12 connected (of 85 found)".
pub fn peers_text(s: &Snapshot) -> String {
    match &s.torrent {
        Some(t) if s.status.is_active() => format!("{} connected (of {} found)", t.peers, t.known_peers),
        Some(t) if t.seeding => "Sharing with others".into(),
        _ => String::new(),
    }
}

/// The add dialog's line about a torrent it looked up: what's in it, or what it's waiting for.
pub fn torrent_info(s: &Snapshot) -> String {
    match (s.status, s.size, &s.torrent) {
        (Status::Error, _, _) => format!("Couldn't open the torrent: {}", s.error.as_deref().unwrap_or("unknown error")),
        (_, Some(size), Some(t)) => {
            let files = if t.file_count == 1 { "1 file".to_string() } else { format!("{} files", t.file_count) };
            format!("Torrent: {}   ·   {files}", human_size(Some(size)))
        }
        _ => status_line(s),
    }
}

/// The transfer rate. While downloading it always says something: "0 B/s" when nothing is
/// arriving at the moment, instead of a blank that looks like the download is stuck.
pub fn rate_text(s: &Snapshot) -> String {
    match (s.status, human_speed(s.speed)) {
        (Status::Downloading, speed) if speed.is_empty() => "0 B/s".into(),
        (_, speed) => speed,
    }
}

/// Time left; while downloading, "Unknown" until it can be worked out.
pub fn eta_text(s: &Snapshot) -> String {
    match (s.status, human_eta(s.eta)) {
        (Status::Downloading, eta) if eta.is_empty() => "Unknown".into(),
        (_, eta) => eta,
    }
}

/// What one connection (segment) is doing, for the connections list.
pub fn connection_info(seg: &Segment, status: Status) -> &'static str {
    if seg.finished() {
        "Complete"
    } else if seg.active {
        if status == Status::Connecting { "Connecting…" } else { "Receiving data…" }
    } else if status.is_active() {
        "Waiting for a free connection"
    } else {
        status.label()
    }
}

/// "documents", "videos"…: the kind of file, for "Always save documents to this folder".
pub fn kind_words(filename: &str) -> &'static str {
    match category_for(filename) {
        "Video" => "videos",
        "Music" => "music",
        "Documents" => "documents",
        "Compressed" => "archives",
        "Programs" => "programs",
        "Images" => "images",
        _ => "other files",
    }
}

/// Under a finished download's name: "1.20 MB · Video · took 4s".
pub fn finished_line(s: &Snapshot) -> String {
    let kind = if s.category == "General" { "Other" } else { s.category };
    let took = s.completed.map(|done| human_eta(Some(done.saturating_sub(s.added) as f64))).filter(|t| !t.is_empty());
    let mut parts = vec![human_size(s.size), kind.to_string()];
    parts.extend(took.map(|t| format!("took {t}")));
    parts.join("  ·  ")
}

/// A copied link worth offering to download (clipboard monitoring): to a file, to a .torrent
/// file, or a magnet link.
pub fn looks_like_file_link(text: &str) -> bool {
    let web = (text.starts_with("http://") || text.starts_with("https://")) && !text.contains(char::is_whitespace);
    (web && (is_downloadable(&filename_from_url(text)) || is_torrent(text, None))) || is_magnet(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn detects_file_links() {
        assert!(super::looks_like_file_link("https://x.org/a/file.zip"));
        assert!(super::looks_like_file_link("http://x.org/movie.mkv?token=1"));
        assert!(!super::looks_like_file_link("https://x.org/page.html"));
        assert!(!super::looks_like_file_link("not a link.zip"));
        assert!(super::looks_like_file_link("https://x.org/ubuntu.iso.torrent"));
        assert!(super::looks_like_file_link("magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Sintel"));
        assert!(!super::looks_like_file_link(r"C:\Users\me\sintel.torrent")); // a copied path isn't a link
    }
}
