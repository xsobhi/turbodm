//! IDM-style categories: choose a sub-folder and an icon from the file extension.

use std::path::{Path, PathBuf};

const CATEGORIES: &[(&str, &[&str])] = &[
    ("Compressed", &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "lz", "iso", "img"]),
    ("Documents", &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp",
                    "txt", "rtf", "epub", "csv", "md", "djvu"]),
    ("Music", &["mp3", "flac", "wav", "aac", "ogg", "opus", "m4a", "wma", "alac", "aiff"]),
    ("Video", &["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "ts", "mpg", "mpeg", "3gp"]),
    ("Programs", &["deb", "rpm", "appimage", "flatpakref", "snap", "exe", "msi", "sh", "run",
                   "apk", "dmg", "pkg", "jar"]),
    ("Images", &["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "tif", "tiff", "psd"]),
];

pub fn extension(filename: &str) -> String {
    filename.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default()
}

pub fn category_for(filename: &str) -> &'static str {
    let ext = extension(filename);
    CATEGORIES
        .iter()
        .find(|(_, exts)| exts.contains(&ext.as_str()))
        .map(|(name, _)| *name)
        .unwrap_or("General")
}

pub fn icon_for(filename: &str) -> &'static str {
    match category_for(filename) {
        "Compressed" => "package-x-generic",
        "Documents" => "x-office-document",
        "Music" => "audio-x-generic",
        "Video" => "video-x-generic",
        "Programs" => "application-x-executable",
        "Images" => "image-x-generic",
        _ => "text-x-generic",
    }
}

/// Extensions worth catching from the clipboard (IDM's file-type list; images excluded).
pub fn is_downloadable(filename: &str) -> bool {
    !matches!(category_for(filename), "General" | "Images")
}

/// base/<Category> when categories are enabled, else base.
pub fn target_dir(base: &Path, filename: &str, use_categories: bool) -> PathBuf {
    match category_for(filename) {
        "General" => base.to_path_buf(),
        _ if !use_categories => base.to_path_buf(),
        category => base.join(category),
    }
}
