//! Rate limit and route-auth tests (requests go into the router with `oneshot`).

use std::net::SocketAddr;
use std::sync::Arc;

use aos_core::Store;
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use crate::limit::{Limits, guard};
use crate::{AppState, api_router};

fn state() -> AppState {
    AppState::new(Arc::new(Store::open_temporary().unwrap()))
}

fn app(s: &AppState, limits: Limits) -> Router {
    guard(api_router(s.clone()), limits)
}

async fn hit(
    app: &Router,
    method: &str,
    uri: &str,
    peer: &str,
    cf: Option<&str>,
    token: Option<&str>,
) -> StatusCode {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(c) = cf {
        req = req.header("cf-connecting-ip", c);
    }
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = if method == "POST" {
        Body::from(r#"{"code":"000000","name":"x"}"#)
    } else {
        Body::empty()
    };
    let mut req = req.body(body).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn claims_are_limited_per_ip_and_globally() {
    let s = state();
    let a = app(&s, Limits { claim_per_ip: 3, claim_global: 5, ..Limits::default() });
    for _ in 0..3 {
        assert_eq!(hit(&a, "POST", "/api/pair/claim", "127.0.0.1:1", Some("1.1.1.1"), None).await, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(hit(&a, "POST", "/api/pair/claim", "127.0.0.1:1", Some("1.1.1.1"), None).await, StatusCode::TOO_MANY_REQUESTS);
    // Another IP behind the tunnel has its own budget, until the global cap.
    for _ in 0..2 {
        assert_eq!(hit(&a, "POST", "/api/pair/claim", "127.0.0.1:2", Some("2.2.2.2"), None).await, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(hit(&a, "POST", "/api/pair/claim", "127.0.0.1:3", Some("3.3.3.3"), None).await, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn repeated_auth_failures_block_the_ip_but_not_health_or_others() {
    let s = state();
    let good = s.store.create_device_token("test", None).unwrap().token;
    let a = app(&s, Limits { fail_per_ip: 4, ..Limits::default() });
    for _ in 0..4 {
        assert_eq!(hit(&a, "GET", "/api/tools", "127.0.0.1:1", Some("9.9.9.9"), Some("aos_bad")).await, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(hit(&a, "GET", "/api/tools", "127.0.0.1:1", Some("9.9.9.9"), Some(&good)).await, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(hit(&a, "GET", "/api/health", "127.0.0.1:1", Some("9.9.9.9"), None).await, StatusCode::OK);
    assert_eq!(hit(&a, "GET", "/api/tools", "127.0.0.1:1", Some("8.8.8.8"), Some(&good)).await, StatusCode::OK);
}

#[tokio::test]
async fn cf_header_is_ignored_from_a_non_loopback_peer() {
    let s = state();
    let a = app(&s, Limits { claim_per_ip: 1, ..Limits::default() });
    assert_eq!(hit(&a, "POST", "/api/pair/claim", "10.0.0.5:1", Some("1.1.1.1"), None).await, StatusCode::UNAUTHORIZED);
    // Spoofing another CF header does not buy a new budget.
    assert_eq!(hit(&a, "POST", "/api/pair/claim", "10.0.0.5:1", Some("2.2.2.2"), None).await, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn only_health_and_claim_are_public() {
    let s = state();
    let a = app(&s, Limits::default());
    let routes = [
        ("GET", "/api/tools"),
        ("POST", "/api/tools/today"),
        ("GET", "/api/sync/changes"),
        ("POST", "/api/sync/push"),
        ("GET", "/api/events"),
        ("GET", "/api/ws"),
        ("GET", "/api/devices"),
        ("POST", "/api/devices/abc/revoke"),
        ("POST", "/mcp"),
    ];
    for (i, (m, p)) in routes.into_iter().enumerate() {
        let ip = format!("5.5.5.{i}");
        assert_eq!(hit(&a, m, p, "127.0.0.1:1", Some(&ip), None).await, StatusCode::UNAUTHORIZED, "{m} {p}");
    }
    assert_eq!(hit(&a, "GET", "/api/health", "127.0.0.1:1", None, None).await, StatusCode::OK);
}

#[tokio::test]
async fn event_streams_are_not_buffered_or_transformed() {
    let s = state();
    let good = s.store.create_device_token("test", None).unwrap().token;
    let a = app(&s, Limits::default());
    let req = Request::builder()
        .uri("/api/events")
        .header(header::AUTHORIZATION, format!("Bearer {good}"))
        .body(Body::empty())
        .unwrap();
    let resp = a.oneshot(req).await.unwrap();
    assert_eq!(resp.headers()["x-accel-buffering"], "no");
    assert_eq!(resp.headers()[header::CACHE_CONTROL], "no-cache, no-transform");
    assert!(!resp.headers().contains_key(header::CONTENT_ENCODING));
}
