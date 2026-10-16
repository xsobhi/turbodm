//! What yt-dlp said, in plain words: its last error, whether the site wants an account or
//! yt-dlp looks out of date, and, for a page it found nothing on, why, said so that the user
//! can tell ("Instagram wants you signed in…", "No video found at this address (…)").

use super::sites::{signed_in, site_name};

/// What `error_of` makes of yt-dlp's "Unsupported URL".
const NOTHING_HERE: &str = "No video found on this page";

/// The site wants to know who's asking: the browser's cookies may get in.
pub(super) fn wants_sign_in(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["sign in", "log in", "login", "logged", "cookies", "your age", "private", "members", "registered users", "rate-limit"]
        .iter().any(|w| e.contains(w))
}

/// yt-dlp itself may be out of date (sites change; yt-dlp follows within days).
pub(super) fn looks_outdated(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["unable to extract", "failed to extract", "latest version", "signature", "nsig", "challenge", "precondition",
     "http error 400", "no video formats", "no such option", "cannot parse data", "failed to parse json"]
        .iter().any(|w| e.contains(w))
}

/// It failed with the browser's cookies in a way their sign-in may be why (one the site won't
/// serve to yt-dlp: Instagram's "Failed to parse JSON"): worth a try without them. Not when it
/// said nothing's there, couldn't be reached, or wants an account (that's what they're for).
pub(super) fn cookies_may_be_why(error: &str) -> bool {
    let unrelated = [NOTHING_HERE, "Couldn't reach the site", "The site didn't answer", "The page took too long"];
    !wants_sign_in(error) && !unrelated.iter().any(|start| error.starts_with(start))
}

/// The site sent a page without the video in it: private, removed, for its members only, or
/// changed in a way yt-dlp can't read yet.
fn hidden(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["cannot parse data", "no video formats", "failed to parse json", "empty media response", "isn't available"]
        .iter().any(|w| e.contains(w))
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
    let lower = text.to_ascii_lowercase();
    // (Python's words for a site it can't reach)
    let unreachable = ["failed to resolve", "getaddrinfo failed", "name or service not known",
                       "temporary failure in name resolution", "nodename nor servname"];
    Some(if text.starts_with("Unsupported URL") {
        NOTHING_HERE.into()
    } else if unreachable.iter().any(|w| lower.contains(w)) {
        "Couldn't reach the site: no connection, or the address is wrong".into()
    } else if lower.contains("timed out") {
        "The site didn't answer in time".into()
    } else {
        text.to_string()
    })
}

/// Why nothing was found at `url`, from what yt-dlp said, and whether the browser's `cookies`
/// were there to try (and had a sign-in in them).
pub(super) fn why(url: &str, error: &str, cookies: Option<&str>) -> String {
    let site = site_name(url);
    if error == NOTHING_HERE {
        return format!("No video found at this address ({url})");
    }
    if wants_sign_in(error) {
        return match cookies {
            // (an address typed or pasted in TurboDM: the browser's cookies come with its button)
            None => format!("{site} wants you signed in: open the video in your browser, signed in to {site}, and \
                             download it with TurboDM's button there"),
            Some(c) if !signed_in(c) => format!("{site} wants you signed in: sign in to {site} in your browser, \
                                                 then try again"),
            Some(_) => format!("{site} wants you signed in, and didn't accept your browser's sign-in for this \
                                video ({error})"),
        };
    }
    if hidden(error) {
        return format!("{site} didn't show this video to yt-dlp: it may be private, removed, or only for people \
                        signed in ({error})");
    }
    error.to_string()
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
        assert_eq!(error_of("ERROR: Unsupported URL: https://example.org/").as_deref(), Some(NOTHING_HERE));
        let broken = "ERROR: [youtube] abc: Unable to extract yt initial data; please report this issue on  \
                      https://github.com/yt-dlp/yt-dlp/issues . Confirm you are on the latest version using yt-dlp -U";
        let message = error_of(broken).unwrap();
        assert_eq!(message, "Unable to extract yt initial data");
        assert!(looks_outdated(&message));
        assert_eq!(error_of("ERROR: unable to download video data: HTTP Error 403: Forbidden").as_deref(),
                   Some("unable to download video data: HTTP Error 403: Forbidden"));
        assert_eq!(error_of("nothing here"), None);
        let offline = "ERROR: [generic] Unable to download webpage: HTTPSConnection(host='example.org', port=443): \
                       Failed to resolve 'example.org' ([Errno 11001] getaddrinfo failed) (caused by TransportError(\"x\"))";
        assert_eq!(error_of(offline).as_deref(), Some("Couldn't reach the site: no connection, or the address is wrong"));
        assert_eq!(error_of("ERROR: [vimeo] 1: Unable to download JSON metadata: The read operation timed out").as_deref(),
                   Some("The site didn't answer in time"));
        // a distribution's old yt-dlp, which doesn't know an option of ours: a newer one is fetched
        let old = "Usage: yt-dlp [OPTIONS] URL [URL...]\n\nyt-dlp: error: no such option: --js-runtimes\n";
        assert!(looks_outdated(&error_of(old).unwrap()));
        assert!(wants_sign_in("Sign in to confirm your age") && !wants_sign_in("HTTP Error 404: Not Found"));
    }

    #[test]
    fn says_why_a_page_has_nothing() {
        // what yt-dlp 2026.08 said about Instagram and Facebook pages
        let feed = error_of("ERROR: Unsupported URL: https://www.instagram.com/").unwrap();
        assert_eq!(why("https://www.instagram.com/", &feed, None), "No video found at this address (https://www.instagram.com/)");
        let login = error_of("ERROR: [Instagram] DQ1x: The webpage request was redirected to the login page. You have \
                              exceeded the rate-limit for accessing posts anonymously. Use --cookies-from-browser or \
                              --cookies for the authentication. See  https://github.com/yt-dlp/yt-dlp/wiki/FAQ").unwrap();
        let reel = "https://www.instagram.com/reel/DQ1x/";
        assert_eq!(why(reel, &login, Some("csrftoken=x; mid=y")),
                   "Instagram wants you signed in: sign in to Instagram in your browser, then try again");
        assert!(why(reel, &login, Some("csrftoken=x; sessionid=1%3Aa")).starts_with(
            "Instagram wants you signed in, and didn't accept your browser's sign-in for this video (The webpage"));
        assert!(why(reel, &login, None).starts_with("Instagram wants you signed in: open the video in your browser"));
        let parse = error_of("ERROR: [facebook] 818149149511725: Cannot parse data; please report this issue on  \
                              https://github.com/yt-dlp/yt-dlp/issues?q= , filling out the appropriate issue template. \
                              Confirm you are on the latest version using  yt-dlp -U").unwrap();
        assert!(looks_outdated(&parse) && !wants_sign_in(&parse));
        assert_eq!(why("https://fb.watch/abc/", &parse, Some("c_user=1; xs=2")),
                   "Facebook didn't show this video to yt-dlp: it may be private, removed, or only for people signed \
                    in (Cannot parse data)");
        assert_eq!(why("https://vimeo.com/1", "HTTP Error 404: Not Found", None), "HTTP Error 404: Not Found");
    }
}
