//! Running a video download: yt-dlp with the chosen quality, its progress into the download's
//! (so the list, progress window and notifications work as for any file). Pausing stops it,
//! keeping its partial files; resuming runs it again and it carries on from them.

use super::progress::{self, Line, Tally};
use super::{common, filename, process, tools, VideoJob};
use crate::engine::task::{unix_now, Status, Task};
use crate::engine::Shared;
use crate::util::unique_path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

/// How many times yt-dlp is run again after it failed (a site's links fail now and then:
/// YouTube's answer 403 to one try and work on the next).
const TRIES: u32 = 5;

/// Drive a video download until it completes, is paused, or fails.
pub async fn run(task: Arc<Task>, shared: Arc<Shared>, cancel: CancellationToken) {
    let outcome = drive(&task, &shared, &cancel).await;
    task.set_activity(None);
    task.video_parts.lock().unwrap().iter_mut().for_each(|p| p.active = false);
    let mut info = task.info.lock().unwrap();
    match outcome {
        Ok(true) => {}
        Ok(false) => info.status = if info.error.is_some() { Status::Error } else { Status::Paused },
        Err(msg) => {
            info.error.get_or_insert(msg);
            info.status = Status::Error;
        }
    }
}

/// Run `f` unless the download is paused first (None).
async fn unless_paused<T>(cancel: &CancellationToken, f: impl Future<Output = T>) -> Option<T> {
    tokio::select! {
        result = f => Some(result),
        _ = cancel.cancelled() => None,
    }
}

async fn drive(task: &Arc<Task>, shared: &Shared, cancel: &CancellationToken) -> Result<bool, String> {
    let client = shared.client();
    if tools::yt_dlp_now().is_none() {
        task.set_activity(Some("Getting yt-dlp…"));
    }
    let Some(exe) = unless_paused(cancel, tools::yt_dlp(&client)).await else { return Ok(false) };
    let mut exe = exe?;
    let picked = task.info.lock().unwrap().video.as_ref().is_some_and(|v| !v.format.is_empty());
    if !picked && !choose(task, shared, cancel, &client).await? {
        return Ok(false);
    }
    if task.info.lock().unwrap().video.as_ref().is_some_and(|v| v.merge) && tools::ffmpeg().is_none() {
        return Err(if tools::CAN_FETCH_FFMPEG { "This quality needs FFmpeg: add the video again to get it".into() }
                   else { tools::NO_FFMPEG.into() });
    }
    task.set_status(Status::Downloading);
    let mut failures = 0;
    let mut updated = false;
    loop {
        task.set_activity(None);
        let error = match once(task, shared, &exe, cancel).await {
            Ok(Some(path)) => return finish(task, path).map(|_| true),
            Ok(None) => return Ok(false), // paused
            Err(_) if cancel.is_cancelled() => return Ok(false),
            Err(error) => error,
        };
        if !updated && super::looks_outdated(&error) {
            updated = true;
            if let Ok(true) = tools::update(&client, true).await {
                exe = tools::yt_dlp_now().unwrap_or(exe);
                continue;
            }
        }
        failures += 1;
        if failures >= TRIES.min(shared.settings().retries.max(1)) || !worth_retrying(&error) {
            return Err(error);
        }
        task.set_activity(Some("Trying again…"));
        let wait = Duration::from_secs(1u64 << failures.min(4)); // 2, 4, 8, 16 s
        if unless_paused(cancel, tokio::time::sleep(wait)).await.is_none() {
            return Ok(false);
        }
    }
}

/// Not a lasting refusal (the video is gone, private, the disk is full…): try again.
fn worth_retrying(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    !["no video found", "sign in", "private", "unavailable", "removed", "not available", "copyright", "members",
      "ffmpeg", "no space", "permission denied", "file name too long", "invalid argument"].iter().any(|w| e.contains(w))
}

/// A video added without its dialog (from the browser with the dialog turned off, or on Linux):
/// look at the page and take the usual quality. False if paused meanwhile.
async fn choose(task: &Task, shared: &Shared, cancel: &CancellationToken, client: &reqwest::Client) -> Result<bool, String> {
    task.set_activity(Some("Looking for the video…"));
    let (url, headers) = {
        let info = task.info.lock().unwrap();
        (info.url.clone(), info.headers())
    };
    let settings = shared.settings();
    let probe = super::probe(client, &url, &headers, settings.timeout_secs);
    let Some(found) = unless_paused(cancel, probe).await else { return Ok(false) };
    let found = found?;
    // no FFmpeg: only what needs none (FFmpeg isn't fetched without asking)
    let ffmpeg = tools::ffmpeg().is_some();
    let usable = |i: &usize| ffmpeg || !found.choices[*i].merge;
    let index = found.choice_like(&settings.video_quality).filter(usable)
        .or_else(|| Some(found.default_choice(ffmpeg)).filter(usable))
        .ok_or(if tools::CAN_FETCH_FFMPEG { "This video needs FFmpeg: add it with Add URL, which offers to get it" }
               else { tools::NO_FFMPEG })?;
    let choice = &found.choices[index];
    let mut info = task.info.lock().unwrap();
    let name = filename(&found.title, &choice.ext);
    if info.category_base.take().is_some() {
        info.directory = settings.folder_for(&name);
    }
    let unique = unique_path(&info.directory.join(&name));
    info.filename = unique.file_name().map(|n| n.to_string_lossy().into_owned());
    info.url = found.url.clone();
    info.size = choice.size;
    info.video = Some(VideoJob::new(&found, choice));
    Ok(true)
}

/// Run yt-dlp once: Some(the file) when it's done, None when paused.
async fn once(task: &Task, shared: &Shared, exe: &std::path::Path, cancel: &CancellationToken)
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
        command.arg("--limit-rate").arg((kib * 1024).to_string());
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
                        *task.video_parts.lock().unwrap() = tally.segments();
                        let mut info = task.info.lock().unwrap();
                        if let Some(video) = info.video.as_mut() {
                            video.done = tally.done();
                        }
                        if let Some(total) = tally.total() {
                            info.size = Some(total);
                        }
                    }
                    Line::Merging => task.set_activity(Some("Joining video and sound…")),
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
        return Err(super::error_of(&errors).unwrap_or_else(|| format!("yt-dlp stopped ({status})")));
    }
    let path = file.unwrap_or_else(|| dir.join(&name));
    if !path.is_file() {
        return Err("yt-dlp finished, but the video isn't there".into());
    }
    Ok(Some(path))
}

/// Done: its real name and size (the joined file is a little bigger than its parts).
fn finish(task: &Task, path: PathBuf) -> Result<(), String> {
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
