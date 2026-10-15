//! Reading yt-dlp's output: our progress lines (`--progress-template`), the finished file
//! (`--print after_move:`), and the moment it starts joining video and sound.

use std::path::PathBuf;

/// What yt-dlp prints for every progress update: "TDM-P status|done|total|estimate|format".
pub const TEMPLATE: &str = "download:TDM-P %(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|\
                            %(progress.total_bytes_estimate)s|%(info.format_id)s";
const PROGRESS: &str = "TDM-P ";
/// …and once the file is in its place: "TDM-F path".
pub const FILE_TEMPLATE: &str = "after_move:TDM-F %(filepath)s";
const FILE: &str = "TDM-F ";

#[derive(Debug, PartialEq)]
pub enum Line {
    Progress { format: String, done: Option<u64>, total: Option<u64>, finished: bool },
    File(PathBuf),
    Merging, // FFmpeg joins the video and the sound
    Other,
}

/// "1234", "1234.5" or "NA" (yt-dlp's word for unknown).
fn number(text: &str) -> Option<u64> {
    text.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0).map(|n| n as u64)
}

pub fn parse(line: &str) -> Line {
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some(rest) = line.strip_prefix(PROGRESS) {
        let fields: Vec<&str> = rest.split('|').collect();
        if let [status, done, total, estimate, format] = fields[..] {
            let total = number(total).or_else(|| number(estimate)).filter(|t| *t > 0);
            let finished = status == "finished";
            // a part found already downloaded comes as finished, with its size only
            let done = number(done).or(if finished { total } else { None });
            return Line::Progress { format: format.to_string(), done, total, finished };
        }
    }
    if let Some(path) = line.strip_prefix(FILE).filter(|p| !p.trim().is_empty()) {
        return Line::File(PathBuf::from(path.trim()));
    }
    if line.starts_with("[Merger] Merging formats into") {
        return Line::Merging;
    }
    Line::Other
}

/// The whole download's progress, over its parts (a video and its sound are two downloads).
#[derive(Clone, Debug, Default)]
pub struct Tally {
    parts: Vec<Part>,
}

#[derive(Clone, Debug)]
struct Part {
    format: String,
    done: u64,
    total: Option<u64>, // estimated until yt-dlp says
    seen: bool,         // yt-dlp reported on it (else an estimate from the page)
}

impl Tally {
    /// The parts expected (format id, estimated size), from the chosen quality.
    pub fn new(expected: &[(String, Option<u64>)]) -> Self {
        let parts = expected.iter().map(|(format, total)| Part { format: format.clone(), done: 0, total: *total, seen: false })
            .collect();
        Tally { parts }
    }

    pub fn update(&mut self, format: &str, done: Option<u64>, total: Option<u64>, finished: bool) {
        let index = match self.parts.iter().position(|p| p.format == format) {
            Some(i) => i,
            None => {
                // yt-dlp fell back to other formats: the estimates don't apply any more
                self.parts.retain(|p| p.seen);
                self.parts.push(Part { format: format.into(), done: 0, total: None, seen: true });
                self.parts.len() - 1
            }
        };
        let part = &mut self.parts[index];
        part.seen = true;
        if let Some(total) = total {
            part.total = Some(total);
        }
        if let Some(done) = done {
            part.done = done;
        }
        if finished {
            part.done = part.total.unwrap_or(part.done).max(part.done);
            part.total = Some(part.done);
        }
    }

    pub fn done(&self) -> u64 {
        self.parts.iter().map(|p| p.done).sum()
    }

    /// The total size, when every part's is known or estimated.
    pub fn total(&self) -> Option<u64> {
        self.parts.iter().map(|p| p.total.map(|t| t.max(p.done))).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_yt_dlp_lines() {
        // as yt-dlp 2026.08 prints them (one part downloading, one already there, the file)
        assert_eq!(parse("TDM-P downloading|3072|1502769|NA|160\n"),
                   Line::Progress { format: "160".into(), done: Some(3072), total: Some(1502769), finished: false });
        assert_eq!(parse("TDM-P finished|NA|891970|NA|139"),
                   Line::Progress { format: "139".into(), done: Some(891970), total: Some(891970), finished: true });
        // HLS: no total yet, an estimate
        assert_eq!(parse("TDM-P downloading|1048576|NA|8388608.5|230"),
                   Line::Progress { format: "230".into(), done: Some(1048576), total: Some(8388608), finished: false });
        assert_eq!(parse(r"TDM-F C:\Users\me\Downloads\Video\test 100% x.mp4"),
                   Line::File(PathBuf::from(r"C:\Users\me\Downloads\Video\test 100% x.mp4")));
        assert_eq!(parse(r#"[Merger] Merging formats into "C:\x\a.mp4""#), Line::Merging);
        assert_eq!(parse("[download] Resuming download at byte 2574371"), Line::Other);
        assert_eq!(parse("TDM-P downloading|1|2"), Line::Other); // not ours after all
    }

    #[test]
    fn adds_up_video_and_sound() {
        let mut tally = Tally::new(&[("137".into(), Some(50_000_000)), ("140".into(), Some(2_000_000))]);
        assert_eq!((tally.done(), tally.total()), (0, Some(52_000_000)));
        tally.update("137", Some(1_000), Some(52_740_343), false);
        assert_eq!((tally.done(), tally.total()), (1_000, Some(54_740_343)));
        tally.update("137", None, None, true);
        tally.update("140", Some(500), Some(2_365_262), false);
        assert_eq!((tally.done(), tally.total()), (52_740_843, Some(55_105_605)));
        // unknown sizes: no total
        let mut odd = Tally::new(&[("x".into(), None)]);
        odd.update("x", Some(10), None, false);
        assert_eq!((odd.done(), odd.total()), (10, None));
    }

    #[test]
    fn other_formats_than_planned_replace_the_estimates() {
        let mut tally = Tally::new(&[("137".into(), Some(50)), ("140".into(), Some(5))]);
        tally.update("18", Some(10), Some(100), false);
        assert_eq!((tally.done(), tally.total()), (10, Some(100)));
    }
}
