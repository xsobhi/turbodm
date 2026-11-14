//! What the server says about a file, read once and trusted only as far as it deserves: how long
//! to wait when it's busy (Retry-After), whether a file being resumed is still the same one
//! (ETag, Last-Modified), and what the finished file's checksum must be (Repr-Digest, Digest).

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The longest a server's Retry-After is waited for: past that, the usual retries go on.
pub const LONGEST_WAIT: Duration = Duration::from_secs(300);

/// `Retry-After: 120` or `Retry-After: Wed, 21 Oct 2026 07:28:00 GMT`: how long from now.
pub fn retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();
    let wait = match value.parse::<u64>() {
        Ok(secs) => Duration::from_secs(secs),
        Err(_) => {
            let at = UNIX_EPOCH + Duration::from_secs(http_date(value)?);
            at.duration_since(SystemTime::now()).unwrap_or_default()
        }
    };
    Some(wait.min(LONGEST_WAIT))
}

/// An HTTP date (RFC 9110's IMF-fixdate, "Sun, 06 Nov 1994 08:49:37 GMT") as seconds since 1970.
pub fn http_date(value: &str) -> Option<u64> {
    let mut parts = value.split_once(',')?.1.split_whitespace();
    let day: u64 = parts.next()?.parse().ok()?;
    let month = parts.next()?;
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter().position(|m| m.eq_ignore_ascii_case(month))? as u64 + 1;
    let year: u64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':').map(|n| n.parse::<u64>().ok());
    let (h, m, s) = (clock.next()??, clock.next()??, clock.next()??);
    if !(1..=31).contains(&day) || year < 1970 || h > 23 || m > 59 || s > 60 {
        return None;
    }
    // days from 1970-01-01 to the date (Howard Hinnant's days_from_civil)
    let (y, mo) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let (era, yoe) = (y / 400, y % 400);
    let doy = (153 * mo + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(days * 86_400 + h * 3600 + m * 60 + s)
}

/// What says which file a server is sending, to tell on resuming whether it's still the same.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Validators {
    /// A strong ETag (a weak one, `W/"…"`, may stay the same while the bytes change).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// Last-Modified, only when strong (RFC 9110 8.8.2.2: sent at least a second after it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

impl Validators {
    pub fn from_headers(etag: Option<&str>, modified: Option<&str>, date: Option<&str>) -> Self {
        let etag = etag.map(str::trim).filter(|e| !e.is_empty() && !e.starts_with("W/")).map(String::from);
        let strong = |lm: &&str| match (http_date(lm), date.and_then(http_date)) {
            (Some(lm), Some(date)) => date > lm,
            _ => false,
        };
        let modified = modified.map(str::trim).filter(strong).map(String::from);
        Validators { etag, modified }
    }

    /// The file changed on the server: a validator both have, different. (Neither, or only one
    /// side: no way to tell, so the same, as before; the bytes before the resume point decide.)
    pub fn changed(&self, now: &Validators) -> bool {
        let differs = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if a != b);
        differs(&self.etag, &now.etag) || (self.etag.is_none() && differs(&self.modified, &now.modified))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Algorithm {
    Sha256,
    Sha512,
}

impl Algorithm {
    pub fn name(self) -> &'static str {
        match self {
            Algorithm::Sha256 => "SHA-256",
            Algorithm::Sha512 => "SHA-512",
        }
    }
}

/// The checksum a finished file must have: the server's, or one the user gave.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checksum {
    pub algorithm: Algorithm,
    pub hex: String, // lower case
}

impl Checksum {
    /// "sha256:…", "sha512:…", or bare hex (its length says which).
    pub fn parse(text: &str) -> Option<Checksum> {
        let text = text.trim();
        let (named, hex) = match text.split_once([':', '=']) {
            Some((name, hex)) => (Some(name.to_ascii_lowercase().replace('-', "")), hex.trim()),
            None => (None, text),
        };
        let hex = hex.to_ascii_lowercase();
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let algorithm = match (named.as_deref(), hex.len()) {
            (None | Some("sha256"), 64) => Algorithm::Sha256,
            (None | Some("sha512"), 128) => Algorithm::Sha512,
            _ => return None,
        };
        Some(Checksum { algorithm, hex })
    }

    /// From the response's headers: `Repr-Digest: sha-256=:BASE64:` (RFC 9530), or the older
    /// `Digest: SHA-256=BASE64` (RFC 3230). Both are the whole file's, even on a range's answer
    /// (unlike Content-Digest, the bytes sent, which isn't read).
    pub fn from_headers(repr_digest: Option<&str>, digest: Option<&str>) -> Option<Checksum> {
        let entries = repr_digest.into_iter().chain(digest).flat_map(|v| v.split(','));
        let mut best: Option<Checksum> = None;
        for entry in entries {
            let Some((name, value)) = entry.split_once('=') else { continue };
            let algorithm = match name.trim().to_ascii_lowercase().as_str() {
                "sha-256" => Algorithm::Sha256,
                "sha-512" => Algorithm::Sha512,
                _ => continue,
            };
            let value = value.trim().trim_matches(':');
            let Some(bytes) = base64_decode(value) else { continue };
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            if Checksum::parse(&hex).is_some_and(|c| c.algorithm == algorithm) && best.is_none() {
                best = Some(Checksum { algorithm, hex });
            }
        }
        best
    }

    /// Hash `file` (calling `progress` with how far, 0 to 1) and compare: Err says how it differs.
    pub fn verify(&self, file: &Path, size: u64, progress: impl Fn(f64)) -> Result<(), String> {
        let got = match self.algorithm {
            Algorithm::Sha256 => hash::<sha2::Sha256>(file, size, progress),
            Algorithm::Sha512 => hash::<sha2::Sha512>(file, size, progress),
        }
        .map_err(|e| format!("Cannot read the file to check it: {e}"))?;
        if got == self.hex {
            return Ok(());
        }
        Err(format!("The file is damaged: its {} is {got}, not {} as expected. Resume downloads it again.",
                    self.algorithm.name(), self.hex))
    }
}

fn hash<D: sha2::Digest>(file: &Path, size: u64, progress: impl Fn(f64)) -> std::io::Result<String> {
    let mut file = std::fs::File::open(file)?;
    let mut hasher = D::new();
    let (mut buf, mut read) = (vec![0u8; 1 << 20], 0u64);
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        read += n as u64;
        progress(if size > 0 { read as f64 / size as f64 } else { 1.0 });
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Standard base64 (with or without padding).
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in text.trim_end_matches('=').bytes() {
        acc = (acc << 6) | value(c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests;
