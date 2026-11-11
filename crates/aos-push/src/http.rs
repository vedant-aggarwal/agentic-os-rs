//! `/api/push/*`, behind the device token. The device is the one whose token made the request.
//!
//! | Method | Path | |
//! |---|---|---|
//! | POST | `/api/push/register` | `{token, platform: ios/android, env?: production/sandbox}` |
//! | POST | `/api/push/unregister` | `{token}` |
//! | POST | `/api/push/local` | `{alarm_ids: [...]}`: alarms this phone rings itself (full list) |
//! | GET | `/api/push/status` | `{enabled, off_reason, devices: [{device_id, platform, env, tail, last_ok_at, last_error}]}` |
//! | POST | `/api/push/test` | send one test push to this device |

use std::sync::Arc;

use aos_core::Store;
use aos_core::devices::Device;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::Push;
use crate::payload::Notification;

#[derive(Clone)]
struct Ctx {
    push: Push,
    auth: Arc<Store>,
}

pub fn router(push: Push, auth: Arc<Store>) -> Router {
    Router::new()
        .route("/api/push/register", post(register))
        .route("/api/push/unregister", post(unregister))
        .route("/api/push/local", post(local))
        .route("/api/push/status", get(status))
        .route("/api/push/test", post(test))
        .with_state(Ctx { push, auth })
}

fn err(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": { "code": code, "message": message.into() } }))).into_response()
}

async fn device(ctx: &Ctx, headers: &HeaderMap, uri: &Uri) -> Result<Device, Response> {
    let unauthorized = || err(StatusCode::UNAUTHORIZED, "unauthorized", "missing or unknown device token. Nothing was revoked.");
    let token = aos_server::auth::token_from(headers, uri).ok_or_else(unauthorized)?;
    let auth = ctx.auth.clone();
    tokio::task::spawn_blocking(move || auth.authenticate(&token).ok().flatten())
        .await
        .ok()
        .flatten()
        .ok_or_else(unauthorized)
}

async fn blocking<T: serde::Serialize + Send + 'static>(ctx: &Ctx, f: impl FnOnce(&Push) -> anyhow::Result<T> + Send + 'static) -> Response {
    let push = ctx.push.clone();
    match tokio::task::spawn_blocking(move || f(&push)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_REQUEST, "invalid_args", format!("{e:#}")),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()),
    }
}

#[derive(Deserialize)]
struct RegisterBody {
    token: String,
    #[serde(default = "ios")]
    platform: String,
    #[serde(default)]
    env: String,
}

fn ios() -> String {
    "ios".into()
}

async fn register(State(ctx): State<Ctx>, headers: HeaderMap, uri: Uri, Json(b): Json<RegisterBody>) -> Response {
    let dev = match device(&ctx, &headers, &uri).await {
        Ok(d) => d,
        Err(r) => return r,
    };
    blocking(&ctx, move |p| {
        p.store().register(&dev.id, &b.token, &b.platform, &b.env)?;
        Ok(json!({ "ok": true, "device_id": dev.id, "enabled": p.enabled() }))
    })
    .await
}

#[derive(Deserialize)]
struct TokenBody {
    token: String,
}

async fn unregister(State(ctx): State<Ctx>, headers: HeaderMap, uri: Uri, Json(b): Json<TokenBody>) -> Response {
    let dev = match device(&ctx, &headers, &uri).await {
        Ok(d) => d,
        Err(r) => return r,
    };
    blocking(&ctx, move |p| Ok(json!({ "removed": p.store().unregister(&dev.id, &b.token)? }))).await
}

#[derive(Deserialize)]
struct LocalBody {
    alarm_ids: Vec<String>,
}

async fn local(State(ctx): State<Ctx>, headers: HeaderMap, uri: Uri, Json(b): Json<LocalBody>) -> Response {
    let dev = match device(&ctx, &headers, &uri).await {
        Ok(d) => d,
        Err(r) => return r,
    };
    blocking(&ctx, move |p| {
        p.store().set_local(&dev.id, &b.alarm_ids)?;
        Ok(json!({ "ok": true, "count": b.alarm_ids.len() }))
    })
    .await
}

async fn status(State(ctx): State<Ctx>, headers: HeaderMap, uri: Uri) -> Response {
    if let Err(r) = device(&ctx, &headers, &uri).await {
        return r;
    }
    blocking(&ctx, |p| {
        Ok(json!({ "enabled": p.enabled(), "off_reason": p.off_reason(), "devices": p.store().active(None)? }))
    })
    .await
}

async fn test(State(ctx): State<Ctx>, headers: HeaderMap, uri: Uri) -> Response {
    let dev = match device(&ctx, &headers, &uri).await {
        Ok(d) => d,
        Err(r) => return r,
    };
    let report = ctx.push.notify(&Notification::test(), Some(&dev.id)).await;
    Json(json!({ "enabled": ctx.push.enabled(), "report": report })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn register_needs_a_device_and_stores_the_token() {
        let path = crate::store::temp_db("http");
        let core = Arc::new(Store::open(&path).unwrap());
        let paired = core.create_device_token("iPhone", Some("ios")).unwrap();
        let push = Push::without_sender(&path).unwrap();
        let app = router(push.clone(), core.clone());
        let body = json!({ "token": "ab".repeat(32), "platform": "ios", "env": "production" }).to_string();

        let anon = Request::post("/api/push/register").header("content-type", "application/json").body(Body::from(body.clone())).unwrap();
        assert_eq!(app.clone().oneshot(anon).await.unwrap().status(), StatusCode::UNAUTHORIZED);

        let req = Request::post("/api/push/register")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", paired.token))
            .body(Body::from(body))
            .unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);
        let rows = push.store().active(None).unwrap();
        assert_eq!((rows.len(), rows[0].device_id.as_str()), (1, paired.device.id.as_str()));

        let local = Request::post("/api/push/local")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", paired.token))
            .body(Body::from(json!({ "alarm_ids": ["al1"] }).to_string()))
            .unwrap();
        assert_eq!(app.oneshot(local).await.unwrap().status(), StatusCode::OK);
        assert!(push.store().has_local(&paired.device.id, "al1").unwrap());
        let _ = std::fs::remove_file(&path);
    }
}
