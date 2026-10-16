//! Looking at a page with yt-dlp (-J): what video is there, the formats it comes in and where
//! each is. Also yt-dlp's errors in plain words (shared with its own downloads).

use super::args::common;
use super::{formats, is_youtube, process, sites, tools, VideoInfo};
use crate::engine::http::Headers;
use std::path::Path;
use std::time::Duration;

/// YouTube works best without the browser's cookies (with them yt-dlp must use pickier
/// players); they're kept for when it asks to sign in. Other sites get them from the start.
fn cookies_first(url: &str) -> bool {
    !is_youtube(url)
}

/// The site wants to know who's asking: the browser's cookies may get in.
fn wants_sign_in(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["sign in", "log in", "login", "logged", "cookies", "your age", "private", "members"].iter().any(|w| e.contains(w))
}

/// yt-dlp itself may be out of date (sites change; yt-dlp follows within days).
pub(super) fn looks_outdated(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["unable to extract", "failed to extract", "latest version", "signature", "nsig", "challenge", "precondition",
     "http error 400", "no video formats", "no such option"].iter().any(|w| e.contains(w))
}

/// yt-dlp's last "ERROR: …" line, in a few plain words.
pub(super) fn error_of(stderr: &str) -> Option<String> {
    // an option it doesn't know (an old yt-dlp, such as a distribution's) is said otherwise
    if let Some(usage) = stderr.lines().find(|l| l.contains("error: no such option")) {
        return Some(usage.trim().to_string());
    }
    let line = stderr.lines().rev().find(|l| l.starts_with("ERROR:"))?;
    let mut text = line.trim_start_matches("ERROR:").trim();
    // "[youtube] Z4C82eyhwgU: Sign in…" → "Sign in…"
    if text.starts_with('[')
        && let Some((_, rest)) = text.split_once("] ") {
            text = rest.split_once(": ").filter(|(id, _)| !id.contains(' ')).map_or(rest, |(_, r)| r);
        }
    // the advice for yt-dlp's own users (its options, links) goes
    for cut in [". Use --", "; please report", ". Please report", " See  http", ". See http", " (caused by"] {
        if let Some(i) = text.find(cut) {
            text = &text[..i];
        }
    }
    let text = text.trim().trim_end_matches('.');
    Some(if text.starts_with("Unsupported URL") { "No video found on this page".into() } else { text.to_string() })
}

/// Look at a page: its video's title and the qualities it comes in. Fetches yt-dlp first if
/// needed, retries with the browser's cookies when the site asks to sign in, and with the
/// latest yt-dlp when it looks out of date.
pub async fn probe(client: &reqwest::Client, url: &str, headers: &Headers, timeout: u64) -> Result<VideoInfo, String> {
    let mut exe = tools::yt_dlp(client).await?;
    let mut cookies = cookies_first(url) && headers.cookies.is_some();
    let (mut page, mut updated) = (url.to_string(), false);
    loop {
        match probe_once(&exe, &page, headers, cookies, timeout).await {
            Ok(mut info) => {
                info.with_cookies = cookies;
                return Ok(info);
            }
            Err(e) if !cookies && headers.cookies.is_some() && wants_sign_in(&e) => cookies = true,
            // (the same video elsewhere, when the page wants an account)
            Err(e) if page == url && wants_sign_in(&e) && let Some(other) = sites::alternative(url) => page = other,
            Err(e) if !updated && looks_outdated(&e) => {
                updated = true;
                if tools::update(client, true).await != Ok(true) {
                    return Err(e);
                }
                exe = tools::yt_dlp_now().ok_or(e)?;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn probe_once(exe: &Path, url: &str, headers: &Headers, cookies: bool, timeout: u64) -> Result<VideoInfo, String> {
    let mut command = process::command(exe);
    let _cookies = common(&mut command, url, headers, cookies, timeout);
    command.args(["-J", "--playlist-items", "1", "--"]).arg(url);
    let mut running = process::Running::spawn(command).map_err(|e| format!("yt-dlp doesn't start: {e}"))?;
    let (mut stdout, mut stderr) = (running.child.stdout.take(), running.child.stderr.take());
    let read = async {
        use tokio::io::AsyncReadExt;
        let (mut out, mut err) = (vec![], vec![]);
        let (a, b) = tokio::join!(async { if let Some(s) = &mut stdout { s.read_to_end(&mut out).await } else { Ok(0) } },
                                  async { if let Some(s) = &mut stderr { s.read_to_end(&mut err).await } else { Ok(0) } });
        a.and(b).map(|_| (out, err))
    };
    // a page that never answers: give up after a while
    let read = tokio::time::timeout(Duration::from_secs(timeout.max(30) * 4), read).await;
    let Ok(Ok((out, err))) = read else {
        running.stop().await;
        return Err("The page took too long to answer".into());
    };
    let status = running.child.wait().await.map_err(|e| e.to_string())?;
    let err = String::from_utf8_lossy(&err);
    if !status.success() || out.is_empty() {
        return Err(error_of(&err).unwrap_or_else(|| format!("yt-dlp couldn't read the page ({status})")));
    }
    formats::parse(&out, url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_yt_dlp_errors() {
        let bot = "WARNING: [youtube] No title found\nERROR: [youtube] Z4C82eyhwgU: Sign in to confirm you’re not \
                   a bot. Use --cookies-from-browser or --cookies for the authentication. See  https://github.com/x";
        let message = error_of(bot).unwrap();
        assert_eq!(message, "Sign in to confirm you’re not a bot");
        assert!(wants_sign_in(&message) && !looks_outdated(&message));
        assert_eq!(error_of("ERROR: Unsupported URL: https://example.org/").as_deref(), Some("No video found on this page"));
        let broken = "ERROR: [youtube] abc: Unable to extract yt initial data; please report this issue on  \
                      https://github.com/yt-dlp/yt-dlp/issues . Confirm you are on the latest version using yt-dlp -U";
        let message = error_of(broken).unwrap();
        assert_eq!(message, "Unable to extract yt initial data");
        assert!(looks_outdated(&message));
        assert_eq!(error_of("ERROR: unable to download video data: HTTP Error 403: Forbidden").as_deref(),
                   Some("unable to download video data: HTTP Error 403: Forbidden"));
        assert_eq!(error_of("nothing here"), None);
        // a distribution's old yt-dlp, which doesn't know an option of ours: a newer one is fetched
        let old = "Usage: yt-dlp [OPTIONS] URL [URL...]\n\nyt-dlp: error: no such option: --js-runtimes\n";
        assert!(looks_outdated(&error_of(old).unwrap()));
    }

    #[test]
    fn youtube_gets_the_browser_cookies_only_when_it_asks() {
        assert!(!cookies_first("https://www.youtube.com/watch?v=x") && cookies_first("https://vimeo.com/1"));
        assert!(wants_sign_in("Sign in to confirm your age") && !wants_sign_in("HTTP Error 404: Not Found"));
    }
}
