//! Moving a video download to the name and folder its dialog settled on (`Manager::confirm`),
//! downloading or not: its parts and what's half-downloaded of them, or the video once made.

use super::names::{is_leftover, stem};
use crate::engine::task::{Status, Task, TaskInfo};
use std::path::{Path, PathBuf};

/// Move it as it downloads (an open part follows a rename; the next one opens where it's
/// named then). False: not now (its file is being made, yt-dlp has it, another disk…): stop
/// it, then `move_stopped`.
pub fn move_running(task: &Task, name: &str, dir: &Path) -> bool {
    let mut info = task.info.lock().unwrap();
    let ytdlp = info.video.as_ref().is_some_and(|v| v.by_ytdlp) && info.status.is_active();
    if task.video.making() || ytdlp {
        return false;
    }
    let (moves, name) = moves(&info, name, dir);
    if !moves.is_empty() {
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
        for (done, (from, to)) in moves.iter().enumerate() {
            if std::fs::rename(from, to).is_err() {
                // (all or nothing: what moved goes back, and it's done stopped)
                moves[..done].iter().for_each(|(from, to)| { let _ = std::fs::rename(to, from); });
                return false;
            }
        }
    }
    set_place(&mut info, name, dir);
    true
}

/// Move it once it's stopped: renamed, else copied (another disk). Not locked meanwhile (a
/// copy takes a while): nothing else touches a stopped download's files.
pub fn move_stopped(task: &Task, name: &str, dir: &Path) -> std::io::Result<()> {
    let (moves, name) = moves(&task.info.lock().unwrap(), name, dir);
    if !moves.is_empty() {
        std::fs::create_dir_all(dir)?;
    }
    for (from, to) in moves {
        if std::fs::rename(&from, &to).is_err() {
            std::fs::copy(&from, &to)?;
            std::fs::remove_file(&from)?;
        }
    }
    set_place(&mut task.info.lock().unwrap(), name, dir);
    Ok(())
}

/// Its files (the video once made; else its parts and partial files), each with where it goes.
fn moves(info: &TaskInfo, name: &str, dir: &Path) -> (Vec<(PathBuf, PathBuf)>, String) {
    let old_name = info.filename.clone().unwrap_or_else(|| name.to_string());
    if info.status == Status::Completed {
        let (from, to) = (info.path(), dir.join(name));
        return (if from == to || !from.exists() { vec![] } else { vec![(from, to)] }, name.into());
    }
    let (old, new) = (stem(&old_name), stem(name));
    if info.directory == dir && old == new {
        return (vec![], name.into());
    }
    let Ok(entries) = std::fs::read_dir(&info.directory) else { return (vec![], name.into()) };
    let moves = entries.flatten().filter_map(|entry| {
        let file = entry.file_name().to_string_lossy().into_owned();
        is_leftover(&file, old).then(|| (entry.path(), dir.join(format!("{new}{}", &file[old.len()..]))))
    }).collect();
    (moves, name.into())
}

fn set_place(info: &mut TaskInfo, name: String, dir: &Path) {
    info.directory = dir.to_path_buf();
    info.category_base = None; // the dialog chose the folder
    info.filename = Some(name);
}
