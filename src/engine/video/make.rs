//! The parts are in: the file asked for. One part as it comes: moved into place. Video and sound:
//! joined by FFmpeg into the container (copied as they are, not re-encoded: seconds). MP3: the
//! sound converted by FFmpeg.

use super::names::{part_file, stem};
use super::{process, tools, Output};
use crate::engine::task::Task;
use crate::util::unique_path;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

/// Make the file from the parts. None: paused meanwhile (the parts stay; it's made on resume).
pub(super) async fn make(task: &Task, cancel: &CancellationToken) -> Result<Option<PathBuf>, String> {
    let (dir, name, job) = {
        let info = task.info.lock().unwrap();
        task.video.set_making(); // (locked: the add dialog doesn't move the parts from here on)
        (info.directory.clone(), info.filename.clone().unwrap_or_else(|| "video.mp4".into()), info.video.clone().unwrap_or_default())
    };
    let parts: Vec<PathBuf> = job.sources.iter().map(|s| part_file(&dir, stem(&name), &s.format_id, &s.ext)).collect();
    let target = dir.join(&name);
    let target = if target.exists() { unique_path(&target) } else { target };
    let output = job.output();
    let Some(ext) = output.ext() else {
        let part = parts.first().ok_or("Nothing was downloaded")?;
        std::fs::rename(part, &target).map_err(|e| format!("Cannot rename the file: {e}"))?;
        return Ok(Some(target));
    };
    let ffmpeg = tools::ffmpeg_exe().ok_or("FFmpeg is missing: it's needed to make this file")?;
    let temp = dir.join(format!("{}.temp.{ext}", stem(&name)));
    let mut command = process::command(&ffmpeg);
    command.stdout(Stdio::null()).args(["-y", "-hide_banner", "-loglevel", "error", "-nostdin"]);
    for part in &parts {
        command.arg("-i").arg(part);
    }
    if output == Output::Mp3 {
        task.video.set_activity(Some("Converting to MP3…"));
        command.args(["-vn", "-c:a", "libmp3lame", "-q:a", "2"]); // VBR, ~190 kbit/s
    } else {
        task.video.set_activity(Some("Joining video and sound…"));
        if parts.len() > 1 {
            command.args(["-map", "0:v:0", "-map", "1:a:0"]);
        }
        command.args(["-c", "copy"]);
        if output == Output::Mp4 {
            // (Opus sound in MP4 was "experimental" in older FFmpegs, such as a Linux system's)
            command.args(["-movflags", "+faststart", "-strict", "experimental"]);
        }
    }
    command.arg(&temp);
    let mut running = process::Running::spawn(command).map_err(|e| format!("FFmpeg doesn't start: {e}"))?;
    let mut stderr = running.child.stderr.take();
    let complaints = tokio::spawn(async move {
        let mut text = vec![];
        if let Some(stderr) = &mut stderr {
            let _ = stderr.read_to_end(&mut text).await;
        }
        String::from_utf8_lossy(&text).into_owned()
    });
    let status = tokio::select! {
        status = running.child.wait() => status.map_err(|e| e.to_string())?,
        _ = cancel.cancelled() => {
            running.stop().await;
            let _ = std::fs::remove_file(&temp);
            return Ok(None);
        }
    };
    task.video.set_activity(None);
    if !status.success() {
        let _ = std::fs::remove_file(&temp);
        let said = complaints.await.unwrap_or_default();
        let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it stopped").trim().to_string();
        return Err(format!("FFmpeg couldn't make the file: {last}"));
    }
    std::fs::rename(&temp, &target).map_err(|e| format!("Cannot rename the file: {e}"))?;
    for part in parts {
        let _ = std::fs::remove_file(part);
    }
    Ok(Some(target))
}
