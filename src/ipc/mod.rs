//! Talking to the running app: browser extension -> native host -> Unix socket -> app.

pub mod client;
pub mod native_host;
mod qualities;
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
        /// A quality picked in the browser's menu (a `VideoChoice` id, 1.10): its dialog opens
        /// with it, and it's downloading meanwhile; without the dialog, it's what's downloaded.
        #[serde(default)]
        choice: Option<String>,
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
    /// The browser's menu of a video's qualities (1.10): the answer, in the reply, is its title
    /// and `choices` (or `error`). Starts TurboDM if needed, and waits for the page's look
    /// (shared with a `VideoPeek` under way).
    VideoQualities {
        url: String,
        #[serde(default)]
        referrer: Option<String>,
        #[serde(default)]
        cookies: Option<String>,
        #[serde(default)]
        user_agent: Option<String>,
    },
}

/// One line of the browser's quality menu: "1080p MP4", "2160p WebM", "MP3".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoChoice {
    pub id: String, // sent back in `Video { choice }`: "1080p/mp4", "video/mp4", "audio/mp3"
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>, // estimated, in bytes
    #[serde(default)]
    pub audio: bool, // the sound alone
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
    /// `VideoQualities`' answer: the video's title and the qualities it comes in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<VideoChoice>,
}

impl Reply {
    pub fn ok() -> Self {
        Reply { ok: true, running: true, version: crate::config::VERSION.into(), ..Default::default() }
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
            Message::Video { url, cookies, user_agent, referrer, silent, choice } => {
                assert_eq!((url.as_str(), cookies.as_deref(), user_agent.as_deref(), referrer, silent, choice),
                           ("https://youtu.be/x", Some("a=1"), Some("UA"), None, false, None));
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

    #[test]
    fn video_quality_menu_json() {
        let json = r#"{"type":"videoQualities","url":"https://youtu.be/x","referrer":"https://youtu.be/x","cookies":"a=1","userAgent":"UA"}"#;
        match serde_json::from_str::<Message>(json).unwrap() {
            Message::VideoQualities { url, referrer, cookies, user_agent } => {
                assert_eq!((url.as_str(), referrer.as_deref(), cookies.as_deref(), user_agent.as_deref()),
                           ("https://youtu.be/x", Some("https://youtu.be/x"), Some("a=1"), Some("UA")));
            }
            other => panic!("wrong message {other:?}"),
        }
        let json = r#"{"type":"video","url":"https://youtu.be/x","choice":"1080p/mp4"}"#;
        match serde_json::from_str::<Message>(json).unwrap() {
            Message::Video { choice, silent, .. } => assert_eq!((choice.as_deref(), silent), (Some("1080p/mp4"), false)),
            other => panic!("wrong message {other:?}"),
        }
        // the answer: a plain reply leaves the menu's fields out
        let plain = serde_json::to_string(&Reply::ok()).unwrap();
        assert!(!plain.contains("title") && !plain.contains("choices") && !plain.contains("error"), "{plain}");
        let reply = Reply {
            title: Some("A video".into()),
            choices: vec![
                VideoChoice { id: "1080p/mp4".into(), label: "1080p MP4".into(), size: Some(52_428_800), audio: false },
                VideoChoice { id: "audio/mp3".into(), label: "MP3".into(), size: None, audio: true },
            ],
            ..Reply::ok()
        };
        let json = serde_json::to_value(&reply).unwrap();
        assert_eq!(json, serde_json::json!({
            "ok": true, "running": true, "version": crate::config::VERSION, "title": "A video",
            "choices": [{"id": "1080p/mp4", "label": "1080p MP4", "size": 52_428_800, "audio": false},
                        {"id": "audio/mp3", "label": "MP3", "audio": true}],
        }));
        assert_eq!(serde_json::from_value::<Reply>(json).unwrap().choices, reply.choices);
        let failed = serde_json::to_value(Reply::error("No video found on this page")).unwrap();
        assert_eq!(failed, serde_json::json!({"ok": false, "running": false, "version": "", "error": "No video found on this page"}));
    }
}
