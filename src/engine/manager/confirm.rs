//! Confirming a download the add dialog started early (`Manager::prefetch`): moving it to the
//! chosen name and folder and giving it all its connections.

use super::Manager;
use crate::engine::task::{Status, Task, TaskInfo};
use crate::util::{part_path, sanitize_filename, unique_path};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// What the add dialog settled on for a download it started early (`Manager::prefetch`).
#[derive(Clone, Debug)]
pub struct Confirm {
    pub filename: Option<String>,
    pub directory: PathBuf,
    pub connections: usize,
    pub start: bool, // false: "Download later" (pause it, keep what's downloaded)
}

impl Manager {
    /// The add dialog's Start / Download later for a download it started early: move what's
    /// downloaded to the chosen name and folder, then list it like any other download.
    pub fn confirm(self: &Arc<Self>, id: &str, choice: Confirm) {
        let Some(task) = self.find(id) else { return };
        // from the early single connection to all of them, without reconnecting (set right
        // away: the progress window opening next shows that many)
        task.info.lock().unwrap().connections = choice.connections;
        task.grow.notify_one();
        let me = self.clone();
        self.rt.spawn(async move {
            let name = choice.filename.as_deref().filter(|n| !n.trim().is_empty()).map(sanitize_filename);
            if move_files(&task, name.clone(), &choice.directory).is_err() {
                // another disk: stop, then move the file the slow way
                me.stop_and_wait(&task).await;
                let (task2, dir) = (task.clone(), choice.directory.clone());
                let moved = tokio::task::spawn_blocking(move || copy_files(&task2, name, &dir)).await;
                if let Ok(Err(err)) = moved {
                    task.info.lock().unwrap().error = Some(format!("Cannot move the file: {err}"));
                }
            }
            if choice.start {
                me.start_task(&task); // no-op if still running or already complete
            } else {
                me.stop_and_wait(&task).await;
            }
            task.pending.store(false, Ordering::Relaxed); // listed, saved, events from here on
            me.dirty.store(true, Ordering::Relaxed);
        });
    }
}

/// The file (or its unfinished part) now, and as it would be named with this name and folder.
fn target(info: &TaskInfo, name: Option<String>, dir: &Path) -> (PathBuf, PathBuf, Option<String>) {
    let completed = info.status == Status::Completed;
    let file = |p: PathBuf| if completed { p } else { part_path(&p) };
    let name = name.or_else(|| info.filename.clone());
    let old = info.path();
    let mut new = dir.join(name.as_deref().unwrap_or("download"));
    if new != old && (new.exists() || part_path(&new).exists()) {
        new = unique_path(&new);
    }
    let name = name.map(|_| new.file_name().unwrap().to_string_lossy().into_owned());
    (file(old), file(new), name)
}

fn set_place(info: &mut TaskInfo, name: Option<String>, dir: &Path) {
    info.directory = dir.to_path_buf();
    info.category_base = None; // the dialog chose the folder
    if name.is_some() {
        info.filename = name;
    }
}

/// Rename in place, locked so the download can't open the file meanwhile. Fine while
/// downloading (open files follow a rename); fails across disks.
fn move_files(task: &Task, name: Option<String>, dir: &Path) -> std::io::Result<()> {
    let mut info = task.info.lock().unwrap();
    let (old, new, name) = target(&info, name, dir);
    if old != new {
        std::fs::create_dir_all(dir)?;
        match std::fs::rename(&old, &new) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    set_place(&mut info, name, dir);
    Ok(())
}

/// Copy then delete, for a download that is stopped (not locked: the copy can take a while).
fn copy_files(task: &Task, name: Option<String>, dir: &Path) -> std::io::Result<()> {
    let (old, new, name) = target(&task.info.lock().unwrap(), name, dir);
    if old != new && old.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::copy(&old, &new)?;
        std::fs::remove_file(&old)?;
    }
    set_place(&mut task.info.lock().unwrap(), name, dir);
    Ok(())
}
