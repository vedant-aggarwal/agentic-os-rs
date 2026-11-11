//! REST + SSE under `/api/nora`, behind the device-token check ([`router_with_auth`]).
//!
//! | Method | Path | |
//! |---|---|---|
//! | GET | `/api/nora/thread?limit=&before=` | messages, oldest first |
//! | POST | `/api/nora/messages` | `{text, engine?: auto/claude/grok}` |
//! | GET | `/api/nora/events` | SSE of [`crate::events::NoraEvent`] |
//! | GET/POST | `/api/nora/settings` | `{heartbeat_enabled, consolidation_enabled}` |
//! | POST | `/api/nora/heartbeat` | run one heartbeat now (skips the rules) |
//! | GET | `/api/nora/heartbeats` | last 50 |
//! | GET | `/api/nora/turns` | last 50 |
//! | POST | `/api/nora/consolidate?day=YYYY-MM-DD` | run consolidation now |

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;

use crate::nora::{EngineChoice, Nora, Settings};

pub fn router_with_auth(nora: Nora, store: Arc<aos_core::Store>) -> Router {
    aos_server::auth::require_device(router(nora), store)
}

pub fn router(nora: Nora) -> Router {
    Router::new()
        .route("/api/nora/thread", get(thread))
        .route("/api/nora/messages", post(send))
        .route("/api/nora/events", get(events))
        .route("/api/nora/settings", get(settings).post(set_settings))
        .route("/api/nora/heartbeat", post(heartbeat))
        .route("/api/nora/heartbeats", get(heartbeats))
        .route("/api/nora/turns", get(turns))
        .route("/api/nora/consolidate", post(consolidate))
        .route("/api/nora/grok", get(grok_state))
        .with_state(nora)
}

fn fail(e: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": { "code": "nora", "message": format!("{e:#}") } }))).into_response()
}

fn ok<T: serde::Serialize>(r: anyhow::Result<T>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
struct ThreadQ {
    limit: Option<usize>,
    before: Option<i64>,
}

async fn thread(State(n): State<Nora>, Query(q): Query<ThreadQ>) -> Response {
    ok(tokio::task::spawn_blocking(move || n.thread(q.limit.unwrap_or(100), q.before)).await.map_err(anyhow::Error::from).and_then(|r| r))
}

#[derive(Deserialize)]
struct SendBody {
    text: String,
    #[serde(default)]
    engine: EngineChoice,
}

async fn send(State(n): State<Nora>, Json(b): Json<SendBody>) -> Response {
    ok(tokio::task::spawn_blocking(move || n.send(&b.text, b.engine, "app")).await.map_err(anyhow::Error::from).and_then(|r| r))
}

async fn events(State(n): State<Nora>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let shutdown = n.shutdown_token();
    let stream = BroadcastStream::new(n.bus().subscribe())
        .map(|r| {
            let data = match r {
                Ok(e) => serde_json::to_string(&e).unwrap_or_default(),
                Err(_) => json!({ "type": "resync" }).to_string(),
            };
            Ok(Event::default().data(data))
        })
        .take_until(async move { shutdown.cancelled().await });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn settings(State(n): State<Nora>) -> Response {
    ok(n.settings())
}

async fn set_settings(State(n): State<Nora>, Json(s): Json<Settings>) -> Response {
    ok(n.set_settings(&s))
}

async fn heartbeat(State(n): State<Nora>) -> Response {
    ok(crate::heartbeat::beat(&n, true).await)
}

async fn heartbeats(State(n): State<Nora>) -> Response {
    ok(n.db().heartbeats(50))
}

async fn turns(State(n): State<Nora>) -> Response {
    ok(n.db().turns(50))
}

#[derive(Deserialize)]
struct DayQ {
    day: Option<String>,
}

async fn consolidate(State(n): State<Nora>, Query(q): Query<DayQ>) -> Response {
    let day = q.day.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    ok(crate::scheduler::consolidate(&n, &day).await)
}

/// Grok Bot write-side state: idle | sending | busy | locked | not_running | error, plus the queue size.
async fn grok_state(State(n): State<Nora>) -> Response {
    let v: serde_json::Value = n
        .db()
        .get_kv("grok_write_state")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "state": "idle", "queued": 0 }));
    ok(Ok::<_, anyhow::Error>(v))
}
