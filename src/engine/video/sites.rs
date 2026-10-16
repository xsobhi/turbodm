//! Which addresses are a video site's pages, and which site a page's cookies belong to.

use crate::categories::is_downloadable;
use crate::util::filename_from_url;

/// Sites known for their videos (and their short links): their pages are looked at for a video
/// straight away, not downloaded as a file first.
const SITES: [&str; 26] = ["youtube.com", "youtu.be", "youtube-nocookie.com", "vimeo.com", "dailymotion.com", "dai.ly",
    "twitch.tv", "tiktok.com", "instagram.com", "facebook.com", "fb.watch", "fb.com", "x.com", "twitter.com",
    "reddit.com", "redd.it", "soundcloud.com", "bilibili.com", "bilibili.tv", "b23.tv", "rumble.com",
    "streamable.com", "threads.net", "pinterest.com", "pin.it", "ok.ru"];

/// The address's host, lower case, without its user and port.
fn host(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

/// Which of `sites` the address is on (it, or one of its subdomains).
fn site_of<'a>(url: &str, sites: &[&'a str]) -> Option<&'a str> {
    let host = host(url);
    sites.iter().copied().find(|s| host == *s || host.ends_with(&format!(".{s}")))
}

pub fn is_youtube(url: &str) -> bool {
    site_of(url, &["youtube.com", "youtu.be", "youtube-nocookie.com"]).is_some()
}

/// A page of a site known for its videos. Any other page is looked at once its server says
/// it's a web page (the Add dialog, link.rs).
pub fn is_video_site(url: &str) -> bool {
    url.starts_with("http") && site_of(url, &SITES).is_some() && !is_downloadable(&filename_from_url(url))
}

/// A web page, by its content type, rather than a file.
pub fn is_web_page(content_type: &str) -> bool {
    matches!(content_type, "text/html" | "application/xhtml+xml")
}

/// Another address of the same video, for when its page wants an account: a Vimeo page's player
/// (yt-dlp needs an account for Vimeo's pages, not for its player).
pub(super) fn alternative(url: &str) -> Option<String> {
    if !matches!(host(url).as_str(), "vimeo.com" | "www.vimeo.com") {
        return None;
    }
    let path = url.split("://").nth(1)?.split(['?', '#']).next()?;
    let id = path.rsplit('/').find(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))?;
    Some(format!("https://player.vimeo.com/video/{id}"))
}

/// The domain a page's cookies are for: the whole site (Instagram's and Facebook's pages call
/// their other hosts, which need them too), else the page's host. ".instagram.com".
pub(super) fn cookie_domain(url: &str) -> String {
    match site_of(url, &SITES) {
        Some(site) => format!(".{site}"),
        None => format!(".{}", host(url).trim_start_matches("www.")),
    }
}
