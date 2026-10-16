//! Names for new downloads: not one a file already has, nor one another download will have
//! (yt-dlp's part files don't carry the final name, so a file check alone misses a video still
//! downloading under the same title; and two videos named alike, "a.mp4" and "a.webm", would
//! share their parts' names). Also the name a download confirmed in its dialog moves to.

use super::Manager;
use crate::engine::task::Task;
use crate::util::unique_path;
use std::path::Path;
use std::sync::Arc;

impl Manager {
    /// `name` in `dir`, or "name (1).ext"… if that file exists or another download will be it
    /// (for a `video`: or another video download there has its stem, which its parts carry).
    pub(super) fn unused_name(&self, dir: &Path, name: String, video: bool) -> String {
        free_name(&self.all(), dir, name, video)
    }

    /// The name `task` can move to in `dir`: `name`, unless another file or download has it.
    pub(super) fn name_to_move_to(&self, task: &Task, dir: &Path, name: String) -> String {
        let (own, video) = {
            let info = task.info.lock().unwrap();
            (info.path(), info.video.is_some())
        };
        if dir.join(&name) == own {
            return name;
        }
        let others: Vec<_> = self.all().into_iter().filter(|t| t.id != task.id).collect();
        free_name(&others, dir, name, video)
    }
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn free_name(tasks: &[Arc<Task>], dir: &Path, name: String, video: bool) -> String {
    let taken = |path: &Path| tasks.iter().any(|t| {
        let info = t.info.lock().unwrap();
        let same_stem = || info.directory == dir && info.filename.as_deref().map(stem)
            == path.file_name().map(|n| stem(&n.to_string_lossy()).to_string()).as_deref();
        info.path() == path || (video && info.video.is_some() && same_stem())
    });
    let mut path = unique_path(&dir.join(&name));
    let mut n = 1;
    while taken(&path) {
        let (stem, ext) = name.rsplit_once('.').map_or((name.as_str(), String::new()), |(s, e)| (s, format!(".{e}")));
        path = unique_path(&dir.join(format!("{stem} ({n}){ext}")));
        n += 1;
    }
    path.file_name().unwrap().to_string_lossy().into_owned()
}
