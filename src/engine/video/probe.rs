//! Looking at a page with yt-dlp (-J): what video is there, the formats it comes in and where
//! each is. With the browser's cookies or without, whichever gets it (said.rs: why not).

use super::args::common;
use super::said::{cookies_may_be_why, error_of, looks_outdated, wants_sign_in, why};
use super::{formats, is_youtube, process, sites, tools, VideoInfo};
use crate::engine::http::Headers;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Pages looked at lately, and looks under way: a look the browser started early (while the
/// video played) is what the dialog gets when asked, at once or as soon as it's done. Kept 10
/// minutes (the addresses inside last hours); failures aren't kept.
type Look = Arc<tokio::sync::OnceCell<Result<VideoInfo, String>>>;
static LOOKS: LazyLock<Mutex<HashMap<String, (Instant, Look)>>> = LazyLock::new(Default::default);
const KEEP: Duration = Duration::from_secs(600);

/// YouTube works best without the browser's cookies (with them yt-dlp must use pickier
/// players); they're kept for when it asks to sign in. Other sites get them from the start.
fn cookies_first(url: &str) -> bool {
    !is_youtube(url)
}

/// Look at a page: its video's title and the qualities it comes in. Fetches yt-dlp first if
/// needed, retries with the browser's cookies when the site asks to sign in (without them when
/// they didn't do), and with the latest yt-dlp when it looks out of date.
pub async fn probe(client: &reqwest::Client, url: &str, headers: &Headers, timeout: u64) -> Result<VideoInfo, String> {
    let url = &sites::clean(url);
    let look = {
        let mut looks = LOOKS.lock().unwrap();
        looks.retain(|_, (at, _)| at.elapsed() < KEEP);
        looks.entry(url.to_string()).or_insert_with(|| (Instant::now(), Look::default())).1.clone()
    };
    let found = look.get_or_init(|| async {
        let started = Instant::now();
        let found = look_at(client, url, headers, timeout).await;
        let said = found.as_ref().map(|info| format!("{} ({} formats)", info.title, info.formats.len())).map_err(Clone::clone);
        super::log::look(url, started.elapsed(), &said); // (video.log: what came of it)
        found
    }).await.clone();
    if found.is_err() {
        forget(url); // (the next ask tries again)
    }
    found
}

/// A video playing in the browser: look at its page now, in the background, so the dialog has
/// it at once when it's asked for. Only once yt-dlp is here (it isn't fetched for this); the same
/// page isn't looked at twice (`probe` keeps it).
pub fn look_ahead(shared: &crate::engine::Shared, rt: &tokio::runtime::Handle, url: String, headers: Headers) {
    if tools::yt_dlp_now().is_none() {
        return;
    }
    let (client, timeout) = (shared.client(), shared.settings().timeout_secs);
    rt.spawn(async move {
        let _ = probe(&client, &url, &headers, timeout).await;
    });
}

/// A page is being looked at.
pub(super) fn looking() -> bool {
    LOOKS.lock().unwrap().values().any(|(_, look)| !look.initialized())
}

/// Look at the page afresh next time (its addresses expired).
pub fn forget(url: &str) {
    LOOKS.lock().unwrap().remove(&sites::clean(url));
}

async fn look_at(client: &reqwest::Client, url: &str, headers: &Headers, timeout: u64) -> Result<VideoInfo, String> {
    let mut exe = tools::yt_dlp(client).await?;
    let has_cookies = headers.cookies.is_some();
    let mut cookies = cookies_first(url) && has_cookies;
    let (mut with, mut without) = (false, false); // tried with the browser's cookies, without them
    let (mut page, mut updated) = (url.to_string(), false);
    let failed = |e: String| Err(why(url, &e, headers.cookies.as_deref()));
    loop {
        *(if cookies { &mut with } else { &mut without }) = true;
        match probe_once(&exe, &page, headers, cookies, timeout).await {
            Ok(mut info) => {
                info.with_cookies = cookies;
                return Ok(info);
            }
            Err(e) if has_cookies && !with && wants_sign_in(&e) => cookies = true,
            // (as anyone would see it: the browser's sign-in may be what the site turned down)
            Err(e) if cookies && !without && cookies_may_be_why(&e) => cookies = false,
            // (the same video elsewhere, when the page wants an account)
            Err(e) if page == url && wants_sign_in(&e) && let Some(other) = sites::alternative(url) => page = other,
            Err(e) if !updated && looks_outdated(&e) => {
                updated = true;
                (with, without) = (false, false); // (each way again, with the new one)
                if tools::update(client, true).await != Ok(true) {
                    return failed(e);
                }
                let Some(new) = tools::yt_dlp_now() else { return failed(e) };
                exe = new;
            }
            Err(e) => return failed(e),
        }
    }
}

async fn probe_once(exe: &Path, url: &str, headers: &Headers, cookies: bool, timeout: u64) -> Result<VideoInfo, String> {
    let mut command = process::command(exe);
    let _cookies = common(&mut command, url, headers, cookies, timeout);
    command.args(["-J", "--playlist-items", "1", "--"]).arg(url);
    let mut running = process::Running::spawn(command).map_err(|e| format!("yt-dlp doesn't start: {e}"))?;
    let (mut stdout, mut stderr) = (running.child.stdout.take(), running.child.stderr.take());
    // (what yt-dlp said so far stays, to tell why if it never finishes)
    let (mut out, mut err) = (vec![], String::new());
    let read = async {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
        let output = async { if let Some(s) = &mut stdout { s.read_to_end(&mut out).await.map(|_| ()) } else { Ok(()) } };
        let said = async {
            if let Some(s) = &mut stderr {
                let mut lines = BufReader::new(s).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    err.push_str(&line);
                    err.push('\n');
                }
            }
        };
        tokio::join!(output, said).0
    };
    // a page that never answers: give up after a while
    let read = tokio::time::timeout(Duration::from_secs(timeout.max(30) * 4), read).await;
    if !matches!(read, Ok(Ok(()))) {
        running.stop().await;
        let last = err.lines().rev().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(140).collect::<String>());
        return Err(match last {
            Some(last) => format!("The page took too long to answer (yt-dlp's last words: {last})"),
            None => "The page took too long to answer".into(),
        });
    }
    let status = running.child.wait().await.map_err(|e| e.to_string())?;
    if !status.success() || out.is_empty() {
        return Err(error_of(&err).unwrap_or_else(|| format!("yt-dlp couldn't read the page ({status})")));
    }
    formats::parse(&out, url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_gets_the_browser_cookies_only_when_it_asks() {
        assert!(!cookies_first("https://www.youtube.com/watch?v=x") && cookies_first("https://vimeo.com/1"));
    }
}
