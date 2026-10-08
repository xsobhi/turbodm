//! Words shown for downloads, the same in the Linux and Windows interfaces.

use crate::categories::{category_for, is_downloadable};
use crate::engine::segments::Segment;
use crate::engine::{Snapshot, Status};
use crate::util::{filename_from_url, human_size};

/// A download's status column: progress while it runs, why it stopped otherwise.
pub fn status_text(snap: &Snapshot) -> String {
    let pct = snap.progress.map(|p| format!("{:.1}%", p * 100.0));
    match snap.status {
        Status::Downloading => pct.unwrap_or_else(|| human_size(Some(snap.downloaded))),
        Status::Paused => pct.map_or("Paused".into(), |p| format!("Paused · {p}")),
        Status::Error => format!("Error: {}", snap.error.as_deref().unwrap_or("unknown")),
        other => other.label().into(),
    }
}

/// The progress window's "Status" line.
pub fn status_line(s: &Snapshot) -> String {
    match (s.status, &s.error) {
        (Status::Error, Some(e)) => format!("Error: {e}"),
        (Status::Downloading, _) => "Receiving data…".into(),
        (status, _) => status.label().into(),
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

/// A copied link worth offering to download (clipboard monitoring).
pub fn looks_like_file_link(text: &str) -> bool {
    (text.starts_with("http://") || text.starts_with("https://"))
        && !text.contains(char::is_whitespace)
        && is_downloadable(&filename_from_url(text))
}

#[cfg(test)]
mod tests {
    #[test]
    fn detects_file_links() {
        assert!(super::looks_like_file_link("https://x.org/a/file.zip"));
        assert!(super::looks_like_file_link("http://x.org/movie.mkv?token=1"));
        assert!(!super::looks_like_file_link("https://x.org/page.html"));
        assert!(!super::looks_like_file_link("not a link.zip"));
    }
}
