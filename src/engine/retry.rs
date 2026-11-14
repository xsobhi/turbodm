//! Waiting between tries: growing waits with a little randomness (so connections refused together
//! don't all come back in the same instant), never shorter than a server's Retry-After.

use super::http::{self, FetchError, Headers, ProbeInfo};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// How long to wait before try `failures + 1`: 1 s, 2 s, 4 s… up to 30 s, plus up to a quarter
/// more at random; at least what the server asked for.
pub fn delay(failures: u32, asked: Option<Duration>) -> Duration {
    let base = (0.5 * 2f64.powi(failures.min(16) as i32)).min(30.0);
    let jittered = Duration::from_secs_f64(base * (1.0 + 0.25 * random()));
    asked.map_or(jittered, |asked| jittered.max(asked))
}

/// 0 to 1, different every call (good enough to spread retries; not for anything secret).
fn random() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    (h.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// Worth another try: the network, or a server that's busy or briefly broken. (Not a refusal
/// like 403/404: trying again gets the same answer.)
pub fn transient(err: &FetchError) -> bool {
    match err {
        FetchError::Network(_) | FetchError::Busy(..) => true,
        FetchError::Http(code, _) => matches!(code, 408 | 425 | 500 | 502 | 504 | 520..=524),
        FetchError::RangeNotSupported => false,
    }
}

/// Sleep, or less if cancelled. False: cancelled.
pub async fn sleep(wait: Duration, cancel: &CancellationToken) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(wait) => true,
        _ = cancel.cancelled() => false,
    }
}

/// `http::probe`, tried again (up to `retries` times) while the failure is a passing one: a
/// download shouldn't fail outright because its very first request met a dropped connection or a
/// moment of 503. Ok(None): cancelled meanwhile.
pub async fn probe(client: &reqwest::Client, url: &str, headers: &Headers, retries: u32,
                   cancel: &CancellationToken) -> Result<Option<ProbeInfo>, FetchError> {
    let mut failures = 0;
    loop {
        let result = tokio::select! {
            r = http::probe(client, url, headers) => r,
            _ = cancel.cancelled() => return Ok(None),
        };
        match result {
            Ok(info) => return Ok(Some(info)),
            Err(err) if transient(&err) && failures < retries => {
                let asked = match &err { FetchError::Busy(_, _, wait) => *wait, _ => None };
                if !sleep(delay(failures, asked), cancel).await {
                    return Ok(None);
                }
                failures += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_grow_and_respect_the_server() {
        let first = delay(0, None).as_secs_f64();
        assert!((0.5..=0.625).contains(&first), "{first}");
        let later = delay(10, None).as_secs_f64();
        assert!((30.0..=37.5).contains(&later), "{later}");
        assert_eq!(delay(0, Some(Duration::from_secs(90))), Duration::from_secs(90));
        assert!(transient(&FetchError::Busy(503, String::new(), None)));
        assert!(!transient(&FetchError::Http(404, String::new())));
    }
}
