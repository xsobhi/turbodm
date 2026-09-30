//! Talking to the running app: browser extension -> native host -> Unix socket -> app.

pub mod client;
pub mod native_host;
pub mod server;

use serde::{Deserialize, Serialize};

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
            Message::Download { url, user_agent, cookies, silent, .. } => {
                assert_eq!((url.as_str(), user_agent.as_deref(), cookies.as_deref(), silent),
                           ("https://x/y.zip", Some("UA"), Some("a=1"), false));
            }
            other => panic!("wrong message {other:?}"),
        }
        assert!(matches!(serde_json::from_str::<Message>(r#"{"type":"ping"}"#), Ok(Message::Ping)));
    }
}
