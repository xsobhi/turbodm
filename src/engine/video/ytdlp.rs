//! yt-dlp downloading a video itself: what TurboDM can't download directly (streams in
//! fragments, HLS and the like; or a site that turned TurboDM's connections down). Its output is
//! read into the download's progress as it comes (its parts, its size, what it's doing). A
//! failed run is run again: sites' links fail now and then (YouTube answers 403 once, then not).

use super::args::common;
use super::said::{error_of, looks_outdated};
use super::progress::{self, Line, Tally};
use super::{process, tools, Output};
use crate::engine::task::{Status, Task};
use crate::engine::Shared;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

/// How many times yt-dlp is run again after it failed.
const TRIES: u32 = 5;

/// Download it with yt-dlp, running it again when it fails: Some(the file), None when paused.
pub(super) async fn run(task: &Task, shared: &Shared, cancel: &CancellationToken, mut exe: PathBuf)
                        -> Result<Option<PathBuf>, String> {
    task.set_status(Status::Downloading);
    let (mut failures, mut updated) = (0, false);
    loop {
        task.video.set_activity(None);
        let error = match run_once(task, shared, &exe, cancel).await {
            Ok(done) => return Ok(done),
            Err(_) if cancel.is_cancelled() => return Ok(None),
            Err(error) => error,
        };
        if !updated && looks_outdated(&error) {
            updated = true;
            if let Ok(true) = tools::update(&shared.client(), true).await {
                exe = tools::yt_dlp_now().unwrap_or(exe);
                continue;
            }
        }
        failures += 1;
        if failures >= TRIES.min(shared.settings().retries.max(1)) || !worth_retrying(&error) {
            return Err(error);
        }
        task.video.set_activity(Some("Trying again…"));
        let wait = Duration::from_secs(1u64 << failures.min(4)); // 2, 4, 8, 16 s
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = cancel.cancelled() => return Ok(None),
        }
    }
}

/// Not a lasting refusal (the video is gone, private, the disk is full…): try again.
fn worth_retrying(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    !["no video found", "sign in", "private", "unavailable", "removed", "not available", "copyright", "members",
      "ffmpeg", "no space", "permission denied", "file name too long", "invalid argument"].iter().any(|w| e.contains(w))
}

/// Run yt-dlp once: Some(the file) when it's done, None when paused.
async fn run_once(task: &Task, shared: &Shared, exe: &Path, cancel: &CancellationToken) -> Result<Option<PathBuf>, String> {
    let settings = shared.settings();
    let (url, dir, name, job, headers, limit) = {
        let info = task.info.lock().unwrap();
        let limit = [settings.speed_limit_kib, info.speed_limit_kib].into_iter().filter(|k| *k > 0).min();
        (info.url.clone(), info.directory.clone(), info.filename.clone().unwrap_or_else(|| "video.mp4".into()),
         info.video.clone().unwrap_or_default(), info.headers(), limit)
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
    let mut command = process::command(exe);
    let _cookies = common(&mut command, &url, &headers, job.with_cookies, settings.timeout_secs);
    command.arg("-f").arg(&job.format);
    match job.output() {
        Output::Single => {}
        Output::Mp3 => {
            command.args(["-x", "--audio-format", "mp3", "--audio-quality", "2"]);
        }
        joined => {
            command.arg("--merge-output-format").arg(joined.ext().unwrap_or("mp4"));
        }
    }
    // the name is ours (yt-dlp's templates take % as theirs: doubled), the extension yt-dlp's
    command.arg("-P").arg(format!("home:{}", dir.display())).arg("-o").arg(format!("{}.%(ext)s", stem.replace('%', "%%")))
        .args(["--newline", "--progress", "--no-quiet", "--no-mtime", "--continue", "--progress-template", progress::TEMPLATE,
               "--print", progress::FILE_TEMPLATE, "--retries", "10", "--fragment-retries", "10"]);
    if let Some(kib) = limit {
        command.arg("--limit-rate").arg((kib * 1024).to_string()); // (applies from the next start)
    }
    command.arg("--").arg(&url);
    let mut running = process::Running::spawn(command).map_err(|e| format!("yt-dlp doesn't start: {e}"))?;
    let mut out = BufReader::new(running.child.stdout.take().expect("piped")).lines();
    let mut err = BufReader::new(running.child.stderr.take().expect("piped")).lines();
    let mut tally = Tally::new(&job.parts);
    let (mut file, mut errors, mut err_open) = (None, String::new(), true);
    loop {
        tokio::select! {
            line = out.next_line() => match line {
                Ok(Some(line)) => match progress::parse(&line) {
                    Line::Progress { format, done, total, finished } => {
                        tally.update(&format, done, total, finished);
                        task.video.set_parts(tally.segments());
                        let mut info = task.info.lock().unwrap();
                        if let Some(video) = info.video.as_mut() {
                            video.done = tally.done();
                        }
                        if let Some(total) = tally.total() {
                            info.size = Some(total);
                        }
                    }
                    Line::Merging => task.video.set_activity(Some("Joining video and sound…")),
                    Line::File(path) => file = Some(path),
                    Line::Other => {}
                },
                _ => break, // yt-dlp is done
            },
            line = err.next_line(), if err_open => match line {
                Ok(Some(line)) => {
                    errors.push_str(&line);
                    errors.push('\n');
                }
                _ => err_open = false,
            },
            _ = cancel.cancelled() => {
                running.stop().await;
                return Ok(None);
            }
        }
    }
    // the last of its complaints, then how it ended
    while let Ok(Ok(Some(line))) = tokio::time::timeout(Duration::from_secs(2), err.next_line()).await {
        errors.push_str(&line);
        errors.push('\n');
    }
    let status = tokio::select! {
        status = running.child.wait() => status.map_err(|e| e.to_string())?,
        _ = cancel.cancelled() => {
            running.stop().await;
            return Ok(None);
        }
    };
    if !status.success() {
        return Err(error_of(&errors).unwrap_or_else(|| format!("yt-dlp stopped ({status})")));
    }
    let path = file.unwrap_or_else(|| dir.join(&name));
    if !path.is_file() {
        return Err("yt-dlp finished, but the video isn't there".into());
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    #[test]
    fn retries_what_may_work_next_time() {
        assert!(super::worth_retrying("unable to download video data: HTTP Error 403: Forbidden"));
        assert!(super::worth_retrying("The read operation timed out"));
        assert!(!super::worth_retrying("Video unavailable. This video is private"));
        assert!(!super::worth_retrying("No video found on this page"));
    }
}
