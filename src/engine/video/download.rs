//! One run of yt-dlp for a download: its arguments, and its output read into the download's
//! progress as it comes (its parts, its size, what it's doing).

use super::probe::{common, error_of};
use super::progress::{self, Line, Tally};
use super::process;
use crate::engine::task::{unix_now, Status, Task};
use crate::engine::Shared;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

/// Run yt-dlp once: Some(the file) when it's done, None when paused.
pub(super) async fn run_once(task: &Task, shared: &Shared, exe: &Path, cancel: &CancellationToken)
                             -> Result<Option<PathBuf>, String> {
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
    common(&mut command, &headers, job.with_cookies, settings.timeout_secs);
    command.arg("-f").arg(&job.format);
    if job.merge {
        command.args(["--merge-output-format", "mp4"]);
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

/// Done: its real name and size (the joined file is a little bigger than its parts).
pub(super) fn finish(task: &Task, path: PathBuf) -> Result<(), String> {
    let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    let mut info = task.info.lock().unwrap();
    if let Some(dir) = path.parent() {
        info.directory = dir.to_path_buf();
    }
    info.filename = path.file_name().map(|n| n.to_string_lossy().into_owned());
    info.size = Some(size);
    if let Some(video) = info.video.as_mut() {
        video.done = size;
    }
    info.status = Status::Completed;
    info.completed = Some(unix_now());
    Ok(())
}
