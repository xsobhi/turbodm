//! Download speed limit shared by connections (token bucket): one for all downloads, one per download.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    state: Mutex<State>,
}

struct State {
    rate: f64,       // bytes per second, 0 = unlimited
    allowance: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        let state = State { rate: bytes_per_sec as f64, allowance: 0.0, last: Instant::now() };
        RateLimiter { state: Mutex::new(state) }
    }

    pub fn set_rate(&self, bytes_per_sec: u64) {
        let mut s = self.state.lock().unwrap();
        s.rate = bytes_per_sec as f64;
        s.allowance = s.allowance.min(s.rate);
        s.last = Instant::now();
    }

    /// Account for `n` bytes; waits when the connections run ahead of the limit.
    pub async fn consume(&self, n: usize) {
        let wait = {
            let mut s = self.state.lock().unwrap();
            if s.rate <= 0.0 {
                return;
            }
            let now = Instant::now();
            s.allowance = (s.allowance + now.duration_since(s.last).as_secs_f64() * s.rate).min(s.rate);
            s.last = now;
            s.allowance -= n as f64;
            if s.allowance < 0.0 { -s.allowance / s.rate } else { 0.0 }
        };
        if wait > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(wait.min(5.0))).await;
        }
    }
}
