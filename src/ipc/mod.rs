//! Talking to the running app: browser extension -> native host -> Unix socket -> app.

pub mod client;
pub mod native_host;
pub mod server;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Messages from the browser extension (and from `turbodm URL` on the command line).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Message {
    Ping,
    Show,
    Quit,
    Download {
        url: String,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        directory: Option<PathBuf>, // where the browser was saving it (e.g. picked in "Save As")
        #[serde(default)]
        referrer: Option<String>,
        #[serde(default)]
        cookies: Option<String>,
        #[serde(default)]
        user_agent: Option<String>,
        #[serde(default)]
        file_size: Option<u64>,
        #[serde(default)]
        silent: bool, // skip the confirmation dialog
    },
    /// A video on a web page: the extension's "Download video with TurboDM". (Added in 1.8: an
    /// older TurboDM answers "bad message", and the extension says it needs updating.)
    Video {
        url: String, // the page (or the link to it)
        #[serde(default)]
        referrer: Option<String>,
        #[serde(default)]
        cookies: Option<String>,
        #[serde(default)]
        user_agent: Option<String>,
        #[serde(default)]
        silent: bool, // no dialog: the usual quality
    },
    /// A video playing in the browser: look at its page now, so its qualities are ready when
    /// it's asked for (1.9.1; only for a TurboDM already running, which an older one ignores).
    VideoPeek {
        url: String,
        #[serde(default)]
        referrer: Option<String>,
        #[serde(default)]
        cookies: Option<String>,
        #[serde(default)]
        user_agent: Option<String>,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    pub fn ok() -> Self {
        Reply { ok: true, running: true, version: crate::config::VERSION.into(), error: None }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Reply { ok: false, error: Some(msg.into()), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extension_json() {
        let json = r#"{"type":"download","url":"https://x/y.zip","userAgent":"UA","cookies":"a=1"}"#;
        match serde_json::from_str::<Message>(json).unwrap() {
            Message::Download { url, user_agent, cookies, silent, directory, .. } => {
                assert_eq!((url.as_str(), user_agent.as_deref(), cookies.as_deref(), silent),
                           ("https://x/y.zip", Some("UA"), Some("a=1"), false));
                assert_eq!(directory, None);
            }
            other => panic!("wrong message {other:?}"),
        }
        let json = r#"{"type":"download","url":"https://x/c.jpg","directory":"/home/me/Pictures"}"#;
        match serde_json::from_str::<Message>(json).unwrap() {
            Message::Download { directory, .. } => assert_eq!(directory, Some("/home/me/Pictures".into())),
            other => panic!("wrong message {other:?}"),
        }
        assert!(matches!(serde_json::from_str::<Message>(r#"{"type":"ping"}"#), Ok(Message::Ping)));
        let json = r#"{"type":"video","url":"https://youtu.be/x","cookies":"a=1","userAgent":"UA"}"#;
        match serde_json::from_str::<Message>(json).unwrap() {
            Message::Video { url, cookies, user_agent, referrer, silent } => {
                assert_eq!((url.as_str(), cookies.as_deref(), user_agent.as_deref(), referrer, silent),
                           ("https://youtu.be/x", Some("a=1"), Some("UA"), None, false));
            }
            other => panic!("wrong message {other:?}"),
        }
        // a video playing: the extension's early look
        let json = r#"{"type":"videoPeek","url":"https://youtu.be/x","referrer":null,"userAgent":"UA"}"#;
        match serde_json::from_str::<Message>(json).unwrap() {
            Message::VideoPeek { url, user_agent, cookies, .. } => {
                assert_eq!((url.as_str(), user_agent.as_deref(), cookies), ("https://youtu.be/x", Some("UA"), None));
            }
            other => panic!("wrong message {other:?}"),
        }
    }
}
