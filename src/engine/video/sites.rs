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

/// Short links, and the site they're of (whose cookies they take).
const SHORT: [(&str, &str); 8] = [("fb.watch", "facebook.com"), ("fb.com", "facebook.com"), ("youtu.be", "youtube.com"),
    ("redd.it", "reddit.com"), ("dai.ly", "dailymotion.com"), ("b23.tv", "bilibili.com"), ("pin.it", "pinterest.com"),
    ("instagr.am", "instagram.com")];

/// The domain a page's cookies are for: the whole site (Instagram's and Facebook's pages call
/// their other hosts, which need them too; a short link's are its site's), else the page's
/// host. ".instagram.com".
pub(super) fn cookie_domain(url: &str) -> String {
    match site_of(url, &SITES) {
        Some(site) => format!(".{}", SHORT.iter().find(|(short, _)| *short == site).map_or(site, |(_, of)| of)),
        None => format!(".{}", host(url).trim_start_matches("www.")),
    }
}

/// The site as people call it: "Instagram", "Facebook"; else its address ("example.org").
pub(super) fn site_name(url: &str) -> String {
    const NAMES: [(&str, &str); 16] = [("youtube.com", "YouTube"), ("youtu.be", "YouTube"), ("instagram.com", "Instagram"),
        ("facebook.com", "Facebook"), ("fb.watch", "Facebook"), ("fb.com", "Facebook"), ("tiktok.com", "TikTok"),
        ("x.com", "X"), ("twitter.com", "X"), ("vimeo.com", "Vimeo"), ("dailymotion.com", "Dailymotion"),
        ("twitch.tv", "Twitch"), ("reddit.com", "Reddit"), ("bilibili.com", "Bilibili"), ("threads.net", "Threads"),
        ("pinterest.com", "Pinterest")];
    let sites: Vec<&str> = NAMES.iter().map(|(site, _)| *site).collect();
    match site_of(url, &sites) {
        Some(site) => NAMES.iter().find(|(s, _)| *s == site).map_or(site, |(_, name)| name).to_string(),
        None => host(url).trim_start_matches("www.").to_string(),
    }
}

/// The browser's cookies have a sign-in in them (the session cookie of a site TurboDM knows).
pub(super) fn signed_in(cookies: &str) -> bool {
    const SESSIONS: [&str; 8] = ["sessionid", "c_user", "auth_token", "SID", "__Secure-3PSID", "vimeo", "SESSDATA",
                                 "reddit_session"];
    cookies.split(';').filter_map(|pair| pair.split_once('=')).any(|(name, _)| SESSIONS.contains(&name.trim()))
}

/// The address without what only says who shared it, where (Facebook's "?__cft__[0]=…&__tn__=…",
/// Instagram's "?igsh=…", "utm_…") and its "#…": one video, one address, also when it's peeked
/// at from one link and asked for from another.
pub fn clean(url: &str) -> String {
    const TRACKING: [&str; 13] = ["__cft__", "__tn__", "__xts__", "utm_", "igsh", "igshid", "mibextid", "rdid",
                                  "share_url", "fbclid", "sfnsn", "_rdr", "ref"];
    if site_of(url, &["facebook.com", "fb.watch", "fb.com", "instagram.com", "threads.net"]).is_none() {
        return url.to_string();
    }
    let address = url.split('#').next().unwrap_or(url);
    let Some((path, query)) = address.split_once('?') else { return address.to_string() };
    let kept: Vec<&str> = query.split('&').filter(|pair| {
        let name = pair.split('=').next().unwrap_or("");
        !name.is_empty() && !TRACKING.iter().any(|t| if t.ends_with('_') { name.starts_with(t) } else { name == *t })
    }).collect();
    if kept.is_empty() { path.to_string() } else { format!("{path}?{}", kept.join("&")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instagram_and_facebook_addresses() {
        // what the browser's button sends: posts' addresses, with what tracks the sharing
        for (sent, cleaned) in [
            ("https://www.facebook.com/reel/1195289147628387?__cft__[0]=AZWxyz&__tn__=%2CO%2CP-R",
             "https://www.facebook.com/reel/1195289147628387"),
            ("https://www.facebook.com/watch/?__cft__%5B0%5D=AZ&v=647537299265662&ref=sharing",
             "https://www.facebook.com/watch/?v=647537299265662"),
            ("https://www.facebook.com/page/posts/pfbid0j1C?__cft__[0]=x#comments", "https://www.facebook.com/page/posts/pfbid0j1C"),
            ("https://www.facebook.com/story.php?story_fbid=123&id=456&mibextid=abc",
             "https://www.facebook.com/story.php?story_fbid=123&id=456"),
            ("https://www.instagram.com/reel/Chunk8-jurw/?igsh=MWQ1ZGUx&utm_source=ig_web",
             "https://www.instagram.com/reel/Chunk8-jurw/"),
            ("https://www.instagram.com/p/BQ0eAlwhDrw/?img_index=2", "https://www.instagram.com/p/BQ0eAlwhDrw/?img_index=2"),
            ("https://www.youtube.com/watch?v=x&utm_source=y#t=3", "https://www.youtube.com/watch?v=x&utm_source=y#t=3"),
        ] {
            assert_eq!(clean(sent), cleaned, "{sent}");
        }
        assert_eq!(cookie_domain("https://fb.watch/abc/"), ".facebook.com"); // (redirected there)
        assert_eq!(cookie_domain("https://m.facebook.com/watch/?v=1"), ".facebook.com");
        assert_eq!(cookie_domain("https://www.facebook.com/share/v/1AbC/"), ".facebook.com");
        assert_eq!((site_name("https://fb.watch/x"), site_name("https://www.instagram.com/reels/x/")),
                   ("Facebook".to_string(), "Instagram".to_string()));
        assert_eq!(site_name("https://www.example.org/v"), "example.org");
        assert!(signed_in("csrftoken=a; sessionid=1%3Ab; mid=c") && signed_in("datr=a; c_user=100; xs=1"));
        assert!(!signed_in("csrftoken=a; mid=c; ig_did=d") && !signed_in(""));
    }
}
