//! A short record of the pages looked at for videos (video.log in TurboDM's data folder): when,
//! which page, how long it took and what came of it, so a look that hangs or fails can be told
//! apart afterwards. Kept small: the older half goes when it grows past 256 KB.

use crate::config::data_dir;
use std::io::Write;
use std::time::Duration;

const MOST: u64 = 256 * 1024;

/// Note one look at `url`: how long it took, and what it found (or why not).
pub(super) fn look(url: &str, took: Duration, outcome: &Result<String, String>) {
    let when = chrono_like_now();
    let (how, what) = match outcome {
        Ok(found) => ("found", found.as_str()),
        Err(why) => ("failed", why.as_str()),
    };
    let line = format!("{when}  {:.1} s  {how}  {url}  {}\n", took.as_secs_f64(), what.replace('\n', " "));
    let path = data_dir().join("video.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MOST) {
        // keep the newer half
        if let Ok(text) = std::fs::read_to_string(&path) {
            let half = text.len() / 2;
            let cut = text[half..].find('\n').map_or(half, |i| half + i + 1);
            let _ = std::fs::write(&path, &text[cut..]);
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Now, as "2026-10-16 15:42:07" (UTC: the log is read by people, the hour doesn't matter much).
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        let now = super::chrono_like_now();
        assert_eq!(now.len(), 19);
        assert!(now.starts_with("20"), "{now}");
    }
}
