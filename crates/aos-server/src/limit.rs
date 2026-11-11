//! In-memory rate limits for pairing claims and failed authentication.
//!
//! One guard wraps the whole app (API, Work, Nora, UI server functions). Behind the Cloudflare
//! tunnel every connection comes from 127.0.0.1, so when the peer is loopback the client IP is
//! taken from `CF-Connecting-IP`. The server listens on loopback only, so only local processes
//! (cloudflared) can set that header.
//!
//! - `POST /api/pair/claim`: at most `claim_per_ip` per window per IP and `claim_global` overall.
//! - Any response with status 401 counts as a failure for that IP. At `fail_per_ip` failures in a
//!   window the IP gets 429 on everything (except `/api/health`) until the window drains.
//! - Counts live in memory and reset on restart.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};

use crate::auth::ApiError;

#[derive(Debug, Clone)]
pub struct Limits {
    pub window: Duration,
    pub claim_per_ip: usize,
    pub claim_global: usize,
    pub fail_per_ip: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            window: Duration::from_secs(60),
            claim_per_ip: 5,
            claim_global: 20,
            fail_per_ip: 30,
        }
    }
}

#[derive(Default)]
struct Hits(HashMap<String, VecDeque<Instant>>);

impl Hits {
    fn prune(q: &mut VecDeque<Instant>, now: Instant, window: Duration) {
        while q.front().is_some_and(|t| now.duration_since(*t) >= window) {
            q.pop_front();
        }
    }

    fn count(&mut self, key: &str, now: Instant, window: Duration) -> usize {
        match self.0.get_mut(key) {
            Some(q) => {
                Self::prune(q, now, window);
                q.len()
            }
            None => 0,
        }
    }

    /// Seconds until the oldest hit in `key` leaves the window.
    fn retry_after(&self, key: &str, now: Instant, window: Duration) -> u64 {
        self.0
            .get(key)
            .and_then(|q| q.front())
            .map(|t| window.saturating_sub(now.duration_since(*t)).as_secs() + 1)
            .unwrap_or(1)
    }

    fn add(&mut self, key: &str, now: Instant, window: Duration) {
        if self.0.len() > 10_000 {
            self.0.retain(|_, q| {
                Self::prune(q, now, window);
                !q.is_empty()
            });
        }
        self.0.entry(key.to_string()).or_default().push_back(now);
    }
}

#[derive(Clone)]
struct Guard {
    limits: Arc<Limits>,
    hits: Arc<Mutex<Hits>>,
}

impl Guard {
    fn new(limits: Limits) -> Self {
        Guard { limits: Arc::new(limits), hits: Arc::default() }
    }

    /// `Err(seconds)` when the request must wait. Records the claim attempt when allowed.
    fn admit(&self, ip: &str, is_claim: bool) -> Result<(), u64> {
        let (l, now) = (&self.limits, Instant::now());
        let mut h = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let fail_key = format!("fail:{ip}");
        if h.count(&fail_key, now, l.window) >= l.fail_per_ip {
            return Err(h.retry_after(&fail_key, now, l.window));
        }
        if is_claim {
            let (ip_key, global_key) = (format!("claim:{ip}"), "claim:*".to_string());
            for (key, max) in [(&ip_key, l.claim_per_ip), (&global_key, l.claim_global)] {
                if h.count(key, now, l.window) >= max {
                    return Err(h.retry_after(key, now, l.window));
                }
            }
            h.add(&ip_key, now, l.window);
            h.add(&global_key, now, l.window);
        }
        Ok(())
    }

    fn record_failure(&self, ip: &str) {
        let mut h = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        h.add(&format!("fail:{ip}"), Instant::now(), self.limits.window);
    }
}

/// The client IP: the peer, or `CF-Connecting-IP` when the peer is loopback (the tunnel).
fn client_ip(req: &Request) -> String {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let forwarded = req
        .headers()
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok());
    match (peer, forwarded) {
        (Some(p), Some(f)) if p.is_loopback() => f.to_string(),
        (Some(p), _) => p.to_string(),
        (None, Some(f)) => f.to_string(),
        (None, None) => "unknown".into(),
    }
}

async fn guard_mw(State(g): State<Guard>, req: Request, next: Next) -> Response {
    if req.uri().path() == "/api/health" {
        return next.run(req).await;
    }
    let ip = client_ip(&req);
    let is_claim = req.method() == Method::POST && req.uri().path() == "/api/pair/claim";
    if let Err(secs) = g.admit(&ip, is_claim) {
        tracing::warn!(%ip, retry_after = secs, "rate limited");
        let mut resp = ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            format!("too many attempts; try again in {secs} s"),
        )
        .into_response();
        if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
            resp.headers_mut().insert(header::RETRY_AFTER, v);
        }
        return resp;
    }
    let mut resp = next.run(req).await;
    if resp.status() == StatusCode::UNAUTHORIZED {
        g.record_failure(&ip);
    }
    // Event streams (SSE) must not be buffered, cached or transformed by a proxy or the tunnel.
    let is_sse = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    if is_sse {
        let h = resp.headers_mut();
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache, no-transform"));
        h.insert("x-accel-buffering", HeaderValue::from_static("no"));
    }
    resp
}

/// Wrap a whole app. Serve it with `into_make_service_with_connect_info::<SocketAddr>()`.
pub fn guard(app: Router, limits: Limits) -> Router {
    app.layer(middleware::from_fn_with_state(Guard::new(limits), guard_mw))
}
