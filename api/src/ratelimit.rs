//! In-memory sliding-window limiter for issue submissions (per client IP).
use std::{collections::HashMap, net::SocketAddr, sync::Mutex, time::{Duration, Instant}};

use axum::http::HeaderMap;

pub struct RateLimiter {
    per_hour: u32,
    hits: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new(per_hour: u32) -> Self {
        Self { per_hour, hits: Mutex::new(HashMap::new()) }
    }

    /// Returns true if the request is allowed (and records it).
    pub fn check(&self, key: &str) -> bool {
        if self.per_hour == 0 {
            return true;
        }
        let now = Instant::now();
        let window = Duration::from_secs(3600);
        let mut map = self.hits.lock().unwrap();
        if map.len() > 10_000 {
            map.retain(|_, v| v.iter().any(|t| now.duration_since(*t) < window));
        }
        let v = map.entry(key.to_string()).or_default();
        v.retain(|t| now.duration_since(*t) < window);
        if v.len() as u32 >= self.per_hour {
            return false;
        }
        v.push(now);
        true
    }
}

/// Client IP: Cloudflare's header first, then the proxy-appended X-Forwarded-For, then the socket.
pub fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> String {
    if let Some(v) = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok()) {
        return v.trim().to_string();
    }
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(last) = v.split(',').map(str::trim).filter(|s| !s.is_empty()).last() {
            return last.to_string();
        }
    }
    peer.ip().to_string()
}
