//! HTTP layer: one shared client, probing a URL, and byte-range requests.

use crate::config::own_user_agent;
use crate::util::{add_extension, filename_from_disposition, filename_from_url};
use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Client, Response, StatusCode};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Headers {
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub cookies: Option<String>, // "a=1; b=2", as sent by the browser extension
}

#[derive(Debug)]
pub enum FetchError {
    RangeNotSupported,
    Http(u16, String),
    Network(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::RangeNotSupported => write!(f, "the server doesn't support resuming"),
            FetchError::Http(code, reason) => write!(f, "HTTP {code} {reason}"),
            FetchError::Network(msg) => write!(f, "network error: {msg}"),
        }
    }
}

impl From<reqwest::Error> for FetchError {
    fn from(err: reqwest::Error) -> Self {
        let mut msg = err.to_string();
        let mut source = std::error::Error::source(&err);
        while let Some(inner) = source {
            msg = format!("{msg}: {inner}");
            source = inner.source();
        }
        FetchError::Network(msg)
    }
}

#[derive(Clone, Debug)]
pub struct ProbeInfo {
    pub final_url: String,
    pub size: Option<u64>,
    pub resumable: bool,
    pub filename: String,
    pub content_type: String,
    pub etag: Option<String>,
    pub user_agent: Option<String>, // set when we fell back to our own user-agent
}

/// HTTP/1.1 only and no connection reuse: every download connection is its own TCP
/// stream (HTTP/2 would multiplex all of them into a single connection).
pub fn build_client(timeout: Duration) -> Client {
    Client::builder()
        .http1_only()
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("HTTP client")
}

fn header_map(h: &Headers, range: Option<(u64, Option<u64>)>) -> HeaderMap {
    let mut map = HeaderMap::new();
    let ua = h.user_agent.clone().unwrap_or_else(own_user_agent);
    let pairs = [
        (header::USER_AGENT, Some(ua)),
        (header::REFERER, h.referrer.clone()),
        (header::COOKIE, h.cookies.clone()),
        (header::ACCEPT_ENCODING, Some("identity".into())),
        (header::RANGE, range.map(|(start, end)| match end {
            Some(end) => format!("bytes={start}-{}", end - 1),
            None => format!("bytes={start}-"),
        })),
    ];
    for (name, value) in pairs {
        if let Some(v) = value.and_then(|v| HeaderValue::from_str(&v).ok()) {
            map.insert(name, v);
        }
    }
    map
}

fn content_range(resp: &Response) -> Option<(u64, Option<u64>)> {
    let value = resp.headers().get(header::CONTENT_RANGE)?.to_str().ok()?;
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let start = range.split('-').next()?.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

/// GET, optionally for bytes [start, end). A range request must come back as a
/// matching 206 Partial Content, otherwise it's RangeNotSupported.
pub async fn open(client: &Client, url: &str, h: &Headers, range: Option<(u64, Option<u64>)>)
    -> Result<Response, FetchError> {
    let resp = client.get(url).headers(header_map(h, range)).send().await?;
    let status = resp.status();
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        return Err(FetchError::RangeNotSupported);
    }
    if !status.is_success() {
        let reason = status.canonical_reason().unwrap_or("").to_string();
        return Err(FetchError::Http(status.as_u16(), reason));
    }
    if let Some((start, _)) = range {
        let ok = status == StatusCode::PARTIAL_CONTENT
            && content_range(&resp).is_some_and(|(s, _)| s == start);
        if !ok {
            return Err(FetchError::RangeNotSupported);
        }
    }
    Ok(resp)
}

/// Learn size, resume support, final URL and file name by asking for byte 0 only.
/// If a server rejects the browser's user-agent (anti-bot checks compare it with the
/// TLS fingerprint), retry once with TurboDM's own honest user-agent.
pub async fn probe(client: &Client, url: &str, h: &Headers) -> Result<ProbeInfo, FetchError> {
    match probe_once(client, url, h).await {
        Err(FetchError::Network(_) | FetchError::Http(403 | 406, _)) if h.user_agent.is_some() => {
            let own = Headers { user_agent: None, ..h.clone() };
            let mut info = probe_once(client, url, &own).await?;
            info.user_agent = Some(own_user_agent());
            Ok(info)
        }
        other => other,
    }
}

async fn probe_once(client: &Client, url: &str, h: &Headers) -> Result<ProbeInfo, FetchError> {
    let resp = match open(client, url, h, Some((0, Some(1)))).await {
        Ok(resp) => resp,
        Err(FetchError::RangeNotSupported) => open(client, url, h, None).await?,
        Err(err) => return Err(err),
    };
    let headers = resp.headers();
    let text = |name| headers.get(name).and_then(|v: &HeaderValue| v.to_str().ok()).map(String::from);
    let range = content_range(&resp);
    let resumable = resp.status() == StatusCode::PARTIAL_CONTENT && range.is_some_and(|(_, t)| t.is_some());
    let size = if resumable { range.and_then(|(_, total)| total) } else { resp.content_length() };
    let final_url = resp.url().to_string();
    let content_type = text(header::CONTENT_TYPE)
        .map(|t| t.split(';').next().unwrap_or("").trim().to_string())
        .unwrap_or_default();
    let name = text(header::CONTENT_DISPOSITION)
        .and_then(|d| filename_from_disposition(&d))
        .unwrap_or_else(|| filename_from_url(&final_url));
    Ok(ProbeInfo {
        filename: add_extension(name, &content_type),
        etag: text(header::ETAG),
        final_url,
        size,
        resumable,
        content_type,
        user_agent: None,
    })
}
