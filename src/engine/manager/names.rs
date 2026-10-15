//! Names for new downloads: not one a file already has, nor one another download will have
//! (yt-dlp's part files don't carry the final name, so a file check alone misses a video still
//! downloading under the same title).

use super::Manager;
use crate::util::unique_path;
use std::path::Path;

impl Manager {
    /// `name` in `dir`, or "name (1).ext"… if that file exists or another download will be it.
    pub(super) fn unused_name(&self, dir: &Path, name: String) -> String {
        let tasks = self.all();
        let taken = |path: &Path| tasks.iter().any(|t| t.info.lock().unwrap().path() == path);
        let mut path = unique_path(&dir.join(&name));
        let mut n = 1;
        while taken(&path) {
            let (stem, ext) = name.rsplit_once('.').map_or((name.as_str(), String::new()), |(s, e)| (s, format!(".{e}")));
            path = unique_path(&dir.join(format!("{stem} ({n}){ext}")));
            n += 1;
        }
        path.file_name().unwrap().to_string_lossy().into_owned()
    }
}
