//! Running a video download: its parts downloaded by TurboDM itself (direct.rs), then the file
//! made of them (make.rs); yt-dlp downloads it instead only where TurboDM can't (ytdlp.rs).
//! Pausing keeps what's downloaded; resuming, also after a restart, carries on from it.

use super::direct::{self, Ending};
use super::names::{remove_leftovers, stem};
use super::{filename, make, probe, tools, ytdlp, Container, VideoJob, Want};
use crate::engine::task::{unix_now, Status, Task};
use crate::engine::Shared;
use crate::util::unique_path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Drive a video download until it completes, is paused, or fails.
pub async fn run(task: Arc<Task>, shared: Arc<Shared>, cancel: CancellationToken) {
    let outcome = drive(&task, &shared, &cancel).await;
    task.video.stopped();
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
    let picked = task.info.lock().unwrap().video.as_ref().is_some_and(|v| !v.format.is_empty());
    if !picked && !choose(task, shared, cancel).await? {
        return Ok(false);
    }
    let job = task.info.lock().unwrap().video.clone().unwrap_or_default();
    if job.output().needs_ffmpeg() && tools::ffmpeg().is_none() {
        return Err(if tools::CAN_FETCH_FFMPEG { "This needs FFmpeg: Tasks → Download video… offers to get it".into() }
                   else { tools::NO_FFMPEG.into() });
    }
    // half-downloaded by TurboDM 1.8, which left videos to yt-dlp: its partial files are yt-dlp's
    if job.sources.is_empty() && job.done > 0 && let Some(job) = task.info.lock().unwrap().video.as_mut() {
        job.by_ytdlp = true;
    }
    if !task.info.lock().unwrap().video.as_ref().is_some_and(|v| v.by_ytdlp) {
        match direct::run(task, shared, cancel).await? {
            Ending::Done => {
                let Some(path) = make::make(task, cancel).await? else { return Ok(false) };
                return finish(task, path).map(|_| true);
            }
            Ending::Paused => return Ok(false),
            Ending::Elsewhere(why) => leave_to_ytdlp(task, &why),
        }
    }
    if tools::yt_dlp_now().is_none() {
        task.video.set_activity(Some("Getting yt-dlp…")); // (Windows asks for it before: Linux)
    }
    let Some(exe) = unless_paused(cancel, tools::yt_dlp(&shared.client())).await else { return Ok(false) };
    match ytdlp::run(task, shared, cancel, exe?).await? {
        Some(path) => finish(task, path).map(|_| true),
        None => Ok(false),
    }
}

/// TurboDM can't download it itself: yt-dlp downloads it from the start (what TurboDM got of it
/// isn't in a form yt-dlp can carry on from).
fn leave_to_ytdlp(task: &Task, why: &str) {
    if std::env::var_os("TURBODM_DEBUG").is_some() {
        eprintln!("video: yt-dlp downloads it itself: {why}");
    }
    *task.segments.lock().unwrap() = None;
    let mut info = task.info.lock().unwrap();
    if let Some(name) = info.filename.clone() {
        remove_leftovers(&info.directory, stem(&name));
    }
    let estimate = info.video.as_ref().map(|v| v.parts.iter().map(|(_, size)| *size).sum::<Option<u64>>());
    if let Some(job) = info.video.as_mut() {
        job.by_ytdlp = true;
        job.done = 0;
        job.sources.iter_mut().for_each(|s| s.done = false);
    }
    info.size = estimate.flatten();
}

/// A video added without its dialog (from the browser with the dialog turned off, or on Linux):
/// look at the page and take the quality picked in the browser's menu, else what was picked
/// last, or the usual. False if paused meanwhile.
async fn choose(task: &Task, shared: &Shared, cancel: &CancellationToken) -> Result<bool, String> {
    task.video.set_activity(Some("Looking for the video…"));
    let (url, headers, choice) = {
        let info = task.info.lock().unwrap();
        (info.url.clone(), info.headers(), info.video.as_ref().map(|v| v.choice.clone()).unwrap_or_default())
    };
    let settings = shared.settings();
    let client = shared.client();
    let looked = probe(&client, &url, &headers, settings.timeout_secs);
    let Some(found) = unless_paused(cancel, looked).await else { return Ok(false) };
    let found = found?;
    // no FFmpeg: only what needs none (FFmpeg isn't fetched without asking)
    let ffmpeg = tools::ffmpeg().is_some();
    let mut plan = found.chosen(&choice).unwrap_or_else(|| found.first(&settings.video_quality, &settings.video_format, ffmpeg));
    if plan.needs_ffmpeg() && !ffmpeg {
        plan = found.plans(Container::Mp4).into_iter().find(|p| !p.needs_ffmpeg() && matches!(p.want, Want::Video(..)))
            .ok_or(if tools::CAN_FETCH_FFMPEG { "This video needs FFmpeg: Tasks → Download video… offers to get it" }
                   else { tools::NO_FFMPEG })?;
    }
    let mut info = task.info.lock().unwrap();
    let name = filename(&found.title, &plan.ext);
    if info.category_base.take().is_some() {
        info.directory = settings.folder_for(&name);
    }
    let unique = unique_path(&info.directory.join(&name));
    info.filename = unique.file_name().map(|n| n.to_string_lossy().into_owned());
    info.url = found.url.clone();
    info.size = plan.size;
    info.video = Some(VideoJob::new(&found, &plan));
    Ok(true)
}

/// Done: its real name and size (a joined file is a little bigger than its parts).
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
