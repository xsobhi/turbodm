//! Formatting and file-name helpers.

use std::path::{Path, PathBuf};

pub const PART_SUFFIX: &str = ".tdmpart";

pub fn human_size(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else { return "Unknown".into() };
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{size:.2} {}", UNITS[unit]) }
}

pub fn human_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec <= 0.0 { String::new() } else { format!("{}/s", human_size(Some(bytes_per_sec as u64))) }
}

pub fn human_eta(seconds: Option<f64>) -> String {
    let Some(s) = seconds.filter(|s| s.is_finite() && *s >= 0.0) else { return String::new() };
    let s = s as u64;
    match (s / 3600, (s % 3600) / 60, s % 60) {
        (0, 0, sec) => format!("{sec}s"),
        (0, min, sec) => format!("{min}m {sec:02}s"),
        (h, min, _) => format!("{h}h {min:02}m"),
    }
}

pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.');
    let cleaned: String = cleaned.chars().take(240).collect();
    if cleaned.is_empty() { "download".into() } else { cleaned }
}

/// Decode %XX escapes (UTF-8, lossy).
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn filename_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let path = path.splitn(4, '/').nth(3).unwrap_or("");     // drop scheme://host/
    let last = path.rsplit('/').next().unwrap_or("");
    sanitize_filename(&percent_decode(last))
}

/// File name from a Content-Disposition header (RFC 6266, incl. filename*=UTF-8'').
pub fn filename_from_disposition(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if let Some(pos) = lower.find("filename*=") {
        let rest = &value[pos + 10..];
        let rest = rest.split(';').next().unwrap_or("").trim().trim_matches('"');
        if let Some(idx) = rest.rfind('\'') {
            return Some(sanitize_filename(&percent_decode(&rest[idx + 1..])));
        }
    }
    let pos = lower.find("filename=")?;
    let rest = value[pos + 9..].trim_start();
    let name = if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next().unwrap_or("")
    } else {
        rest.split(';').next().unwrap_or("").trim()
    };
    Some(sanitize_filename(name))
}

/// 'file' + 'application/pdf' -> 'file.pdf' (only when the name has no extension).
pub fn add_extension(name: String, content_type: &str) -> String {
    if name.contains('.') {
        return name;
    }
    let ext = match content_type {
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "application/x-7z-compressed" => "7z",
        "application/vnd.rar" | "application/x-rar-compressed" => "rar",
        "application/x-tar" => "tar",
        "application/gzip" => "gz",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "audio/mpeg" => "mp3",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "application/vnd.debian.binary-package" => "deb",
        "application/x-iso9660-image" => "iso",
        _ => return name,
    };
    format!("{name}.{ext}")
}

fn taken(path: &Path) -> bool {
    path.exists() || part_path(path).exists()
}

pub fn part_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(PART_SUFFIX);
    PathBuf::from(os)
}

/// path, or 'name (1).ext', 'name (2).ext'... if it (or its .part file) exists.
pub fn unique_path(path: &Path) -> PathBuf {
    if !taken(path) {
        return path.to_path_buf();
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = match name.strip_suffix(".tar.gz").map(|s| (s, ".tar.gz")) {
        Some(split) => (split.0.to_string(), split.1.to_string()),
        None => match name.rfind('.').filter(|&i| i > 0) {
            Some(i) => (name[..i].to_string(), name[i..].to_string()),
            None => (name.clone(), String::new()),
        },
    };
    (1..)
        .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|candidate| !taken(candidate))
        .expect("unbounded search")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_names() {
        assert_eq!(human_size(Some(512)), "512 B");
        assert_eq!(human_size(Some(1536)), "1.50 KB");
        assert_eq!(filename_from_disposition("attachment; filename=\"a b.zip\""), Some("a b.zip".into()));
        assert_eq!(filename_from_disposition("attachment; filename*=UTF-8''%D9%85.pdf"), Some("م.pdf".into()));
        assert_eq!(filename_from_url("https://x.com/dir/My%20File.iso?x=1"), "My File.iso");
        assert_eq!(filename_from_url("https://x.com/"), "download");
        assert_eq!(sanitize_filename("a/b:c"), "a_b_c");
    }
}
