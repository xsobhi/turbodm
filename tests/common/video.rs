//! What the video tests share: a data folder of their own, a video's parts on local servers,
//! and the dialog's early download of them.

use super::harness::Env;
use super::{start, Options, TestServer};
use std::path::Path;
use std::sync::{Arc, Once};
use turbodm::engine::manager::Manager;
use turbodm::engine::video::{Output, Source, VideoJob};
use turbodm::engine::AddRequest;

pub const PAGE: &str = "https://example.org/watch?v=clip";

/// A data folder of this test's own (its tools: none, unless FFmpeg is on PATH). Each test
/// calls it first, before any thread of its own reads the environment.
pub fn sandbox() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let home = std::env::temp_dir().join(format!("turbodm-test-video-home-{}", std::process::id()));
        // SAFETY: once, before this process's tests start their threads (they all wait here)
        unsafe { std::env::set_var("TURBODM_HOME", home) };
    });
}

pub fn serve(env: &Env, data: Arc<Vec<u8>>, delay_ms: u64) -> TestServer {
    start(&env.rt, data, Options { ranges: true, no_length: false, max_conns: 0 }, delay_ms)
}

pub fn part(id: &str, server: &TestServer, ext: &str, size: usize) -> Source {
    Source { format_id: id.into(), url: server.url.clone(), protocol: "http".into(), ext: ext.into(),
             size: Some(size as u64), ..Default::default() }
}

/// A video download of these parts (the page, what's made of them, where it goes).
pub fn video(dir: &Path, name: &str, output: Output, sources: Vec<Source>) -> AddRequest {
    let job = VideoJob { format: "v+a/b".into(), quality: "720p MKV".into(), title: "clip".into(), output,
                         parts: sources.iter().map(|s| (s.format_id.clone(), s.size)).collect(), sources,
                         choice: "720p/mkv".into(), ..Default::default() };
    AddRequest { url: PAGE.into(), filename: Some(name.into()), directory: Some(dir.to_path_buf()), video: Some(job),
                 ..Default::default() }
}

/// What the dialog starts for a quality, behind it.
pub fn prefetch_video(manager: &Arc<Manager>, dir: &Path, name: &str, output: Output, sources: Vec<Source>) -> String {
    manager.prefetch(video(dir, name, output, sources))
}

pub fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).map(|d| d.flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}
