//! Per-IP token-bucket rate limiter.
//!
//! Each IP gets a bucket of `capacity` tokens that refills at `capacity` tokens/min.
//! A request consumes 1 token; if the bucket is empty, the request is rejected
//! with 429 Too Many Requests.
//!
//! Old buckets are pruned opportunistically on each call to keep memory bounded
//! under sustained traffic from many distinct IPs.
//!
//! Client IP resolution: `ConnectInfo` returns the TCP peer, which is the
//! reverse-proxy's IP behind Fly/Cloudflare/nginx — that would collapse the
//! limiter into a single shared bucket. When `TRUST_PROXY=1` is set, we read
//! the leftmost `X-Forwarded-For` entry instead. We do NOT trust the header by
//! default because it's trivially spoofable on direct binds.

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<Inner>>,
    capacity: f64,
    refill_per_sec: f64,
    trust_proxy: bool,
}

struct Inner {
    buckets: HashMap<IpAddr, Bucket>,
    last_prune: Instant,
}

impl RateLimiter {
    /// `per_minute` requests allowed per IP, with a burst capacity equal to the same number.
    /// `trust_proxy=true` enables `X-Forwarded-For` parsing — set this only when behind a
    /// trusted reverse proxy (Fly, Cloudflare, nginx) since the header is spoofable otherwise.
    pub fn new(per_minute: u32, trust_proxy: bool) -> Self {
        let capacity = per_minute.max(1) as f64;
        Self {
            inner: Arc::new(Mutex::new(Inner {
                buckets: HashMap::new(),
                last_prune: Instant::now(),
            })),
            capacity,
            refill_per_sec: capacity / 60.0,
            trust_proxy,
        }
    }

    fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut g = self.inner.lock().unwrap();

        if now.duration_since(g.last_prune) > Duration::from_secs(300) {
            g.buckets.retain(|_, b| now.duration_since(b.last_refill) < Duration::from_secs(600));
            g.last_prune = now;
        }

        let bucket = g.buckets.entry(ip).or_insert(Bucket {
            tokens: self.capacity,
            last_refill: now,
        });

        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub async fn middleware(
    State(limiter): State<RateLimiter>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let ip = client_ip(req.headers(), addr, limiter.trust_proxy);
    if limiter.check(ip) {
        next.run(req).await
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "60")],
            "rate limit exceeded — try again in a minute",
        )
            .into_response()
    }
}

/// Resolve the real client IP. Defaults to the TCP peer; when `trust_proxy`
/// is true, prefers the leftmost `X-Forwarded-For` entry (the original client
/// in a chain `client, proxy1, proxy2`).
fn client_ip(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        if let Some(forwarded_ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(str::trim)
            .and_then(|s| s.parse().ok())
        {
            return forwarded_ip;
        }
    }
    peer.ip()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn allows_up_to_capacity_then_rejects() {
        let limiter = RateLimiter::new(3, false);
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));
        assert!(!limiter.check(ip));
    }

    #[test]
    fn separate_ips_have_separate_buckets() {
        let limiter = RateLimiter::new(1, false);
        let a: IpAddr = "1.2.3.4".parse().unwrap();
        let b: IpAddr = "5.6.7.8".parse().unwrap();
        assert!(limiter.check(a));
        assert!(limiter.check(b));
        assert!(!limiter.check(a));
        assert!(!limiter.check(b));
    }

    #[test]
    fn ignores_xff_when_trust_proxy_false() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("9.9.9.9"));
        let peer: SocketAddr = "1.2.3.4:5000".parse().unwrap();
        assert_eq!(client_ip(&headers, peer, false), "1.2.3.4".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn uses_xff_first_entry_when_trust_proxy_true() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("9.9.9.9, 10.0.0.1"));
        let peer: SocketAddr = "10.0.0.1:5000".parse().unwrap();
        assert_eq!(client_ip(&headers, peer, true), "9.9.9.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn falls_back_to_peer_when_xff_missing_and_trusting() {
        let headers = HeaderMap::new();
        let peer: SocketAddr = "10.0.0.1:5000".parse().unwrap();
        assert_eq!(client_ip(&headers, peer, true), "10.0.0.1".parse::<IpAddr>().unwrap());
    }
}
