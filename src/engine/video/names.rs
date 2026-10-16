//! A video's file names: the video's own (from its title), its parts' while they download, and
//! what's left beside it to delete.

use crate::engine::task::{Status, TaskInfo};
use crate::util::sanitize_filename;
use std::path::{Path, PathBuf};

/// The file name for a video: its title (kept short: parts add to it while downloading, and
/// Windows paths are limited), then the extension.
pub fn filename(title: &str, ext: &str) -> String {
    // titles are full of what file names can't have: "Part 2: the end" → "Part 2 - the end"
    let mut plain = String::with_capacity(title.len());
    for c in title.chars() {
        match c {
            '"' => plain.push('\''),
            ':' => plain.push_str(" -"),
            '|' | '/' | '\\' => plain.push('-'),
            '?' | '*' | '<' | '>' => plain.push(' '),
            c => plain.push(c),
        }
    }
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = sanitize_filename(&plain).chars().take(120).collect();
    format!("{}.{ext}", name.trim().trim_end_matches('.'))
}

/// "name" of "name.mp4".
pub(super) fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// Where a part goes once downloaded, beside the video: "name.f137.mp4" (as yt-dlp names them).
pub(super) fn part_file(dir: &Path, stem: &str, format_id: &str, ext: &str) -> PathBuf {
    let id: String = format_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    dir.join(format!("{stem}.f{id}.{ext}"))
}

/// What a video leaves beside it while it downloads ("a.f137.mp4", "a.f137.mp4.tdmpart",
/// yt-dlp's "a.mp4.part", "a.mp4.part-Frag3", "a.mp4.ytdl", and "a.temp.mp4" while it's made),
/// for a file named `stem` + extension.
pub(super) fn is_leftover(name: &str, stem: &str) -> bool {
    let Some(rest) = name.strip_prefix(stem).and_then(|r| r.strip_prefix('.')) else { return false };
    let format_part = rest.split_once('.').is_some_and(|(first, ext)| {
        first.len() > 1 && first.starts_with('f') && !ext.contains('.')
            && first[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    });
    rest.contains(".part") || rest.ends_with(".tdmpart") || rest.ends_with(".ytdl") || rest.starts_with("temp.")
        || format_part
}

/// Delete what's left of a video's download beside it (and the video too once it's finished).
pub fn remove_files(info: &TaskInfo) {
    let Some(name) = info.filename.as_deref() else { return };
    if info.status == Status::Completed {
        let _ = std::fs::remove_file(info.path());
    }
    remove_leftovers(&info.directory, stem(name));
}

pub(super) fn remove_leftovers(dir: &Path, stem: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if is_leftover(&entry.file_name().to_string_lossy(), stem) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
