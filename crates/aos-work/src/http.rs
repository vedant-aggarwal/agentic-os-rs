//! REST + SSE routes under `/api/work` for anything that isn't the Dioxus UI (scripts, other
//! agents, a future native client). The UI uses server functions that call the same [`Work`].
//!
//! | Method | Path | |
//! |---|---|---|
//! | GET | `/api/work/projects` | list projects |
//! | POST | `/api/work/projects` | `{name?, folder}` add a folder |
//! | GET | `/api/work/threads?project=` | list threads |
//! | POST | `/api/work/threads` | [`NewThread`] create (and send the first prompt) |
//! | GET | `/api/work/threads/{id}` | [`ThreadDetail`] |
//! | POST | `/api/work/threads/{id}/messages` | `{text}` |
//! | POST | `/api/work/threads/{id}/interrupt` | |
//! | GET | `/api/work/threads/{id}/diff` | [`ThreadDiff`] |
//! | POST | `/api/work/approvals/{id}` | `{decision: approve / approve_session / deny}` |
//! | POST | `/api/work/launch` | [`LaunchMany`] |
//! | GET | `/api/work/events?thread=` | SSE stream of [`WorkEvent`] |
//!
//! Auth: [`router_with_auth`] requires a paired device token (`Authorization: Bearer <token>` or
//! `?token=`), checked against foundation's `aos_core` store, same as `/api/tools`.

use std::convert::Infallible;
use std::time::Duration;

use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::middleware::{self, Next};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::error::WorkError;
use crate::store::now_ms;
use crate::supervisor::Work;
use crate::types::*;

/// The routes behind foundation's device-token check.
pub fn router_with_auth(work: Work, store: Arc<aos_core::Store>) -> Router {
    router(work).route_layer(middleware::from_fn(move |req: Request, next: Next| {
        let store = store.clone();
        async move {
            let token = token(&req);
            let ok = match token {
                Some(t) => tokio::task::spawn_blocking(move || store.authenticate(&t).ok().flatten().is_some())
                    .await
                    .unwrap_or(false),
                None => false,
            };
            if ok {
                next.run(req).await
            } else {
                (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized", "message": "missing or unknown device token" })))
                    .into_response()
            }
        }
    }))
}

fn token(req: &Request) -> Option<String> {
    let header = req.headers().get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    if let Some(t) = header.and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))) {
        return Some(t.trim().to_string()).filter(|t| !t.is_empty());
    }
    req.uri().query()?.split('&').find_map(|p| p.strip_prefix("token=")).map(str::to_string).filter(|t| !t.is_empty())
}

/// The routes without auth (tests, or behind your own layer).
pub fn router(work: Work) -> Router {
    Router::new()
        .route("/api/work/projects", get(projects).post(add_project))
        .route("/api/work/threads", get(threads).post(create_thread))
        .route("/api/work/threads/{id}", get(thread))
        .route("/api/work/threads/{id}/messages", post(send))
        .route("/api/work/threads/{id}/interrupt", post(interrupt))
        .route("/api/work/threads/{id}/diff", get(diff))
        .route("/api/work/approvals/{id}", post(decide))
        .route("/api/work/launch", post(launch))
        .route("/api/work/events", get(events))
        .with_state(work)
}

impl IntoResponse for WorkError {
    fn into_response(self) -> Response {
        let status = match self.code() {
            "not_found" => StatusCode::NOT_FOUND,
            "invalid_args" => StatusCode::BAD_REQUEST,
            "provider" => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({ "error": self.code(), "message": self.to_string() }))).into_response()
    }
}

type R<T> = Result<Json<T>, WorkError>;

async fn projects(State(w): State<Work>) -> R<Vec<Project>> {
    Ok(Json(w.projects()?))
}

#[derive(Deserialize)]
struct AddProject {
    #[serde(default)]
    name: String,
    folder: String,
}

async fn add_project(State(w): State<Work>, Json(a): Json<AddProject>) -> R<Project> {
    Ok(Json(w.add_project(&a.name, &a.folder)?))
}

#[derive(Deserialize)]
struct ThreadQuery {
    project: Option<String>,
}

async fn threads(State(w): State<Work>, Query(q): Query<ThreadQuery>) -> R<Vec<ThreadSummary>> {
    Ok(Json(w.threads(q.project.as_deref())?))
}

async fn create_thread(State(w): State<Work>, Json(a): Json<NewThread>) -> R<ThreadSummary> {
    Ok(Json(w.create_thread(a)?))
}

async fn thread(State(w): State<Work>, Path(id): Path<String>) -> R<ThreadDetail> {
    Ok(Json(w.thread(&id)?))
}

#[derive(Deserialize)]
struct SendBody {
    text: String,
}

async fn send(State(w): State<Work>, Path(id): Path<String>, Json(b): Json<SendBody>) -> R<ThreadSummary> {
    Ok(Json(w.send(&id, &b.text)?))
}

async fn interrupt(State(w): State<Work>, Path(id): Path<String>) -> R<ThreadSummary> {
    Ok(Json(w.interrupt(&id)?))
}

async fn diff(State(w): State<Work>, Path(id): Path<String>) -> R<ThreadDiff> {
    // git can take a moment on big repos: keep it off the async workers.
    let d = tokio::task::spawn_blocking(move || w.diff(&id))
        .await
        .map_err(|e| WorkError::Io(std::io::Error::other(e.to_string())))??;
    Ok(Json(d))
}

#[derive(Deserialize)]
struct DecideBody {
    decision: String,
}

async fn decide(State(w): State<Work>, Path(id): Path<String>, Json(b): Json<DecideBody>) -> R<Approval> {
    let d = Decision::parse(&b.decision).ok_or_else(|| WorkError::Invalid(format!("unknown decision '{}'", b.decision)))?;
    Ok(Json(w.decide(&id, d)?))
}

async fn launch(State(w): State<Work>, Json(a): Json<LaunchMany>) -> Result<Json<serde_json::Value>, WorkError> {
    let results = w.launch(a)?;
    let started: Vec<&ThreadSummary> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    let failed: Vec<&String> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    Ok(Json(json!({ "started": started, "failed": failed })))
}

#[derive(Deserialize)]
struct EventQuery {
    thread: Option<String>,
}

async fn events(State(w): State<Work>, Query(q): Query<EventQuery>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = event_stream(&w, q.thread).map(|e| Ok(Event::default().json_data(e).unwrap_or_default()));
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// `Hello`, then every event (optionally for one thread). A lagging client gets `Resync`.
pub fn event_stream(w: &Work, thread: Option<String>) -> impl Stream<Item = WorkEvent> + Send + use<> {
    let hello = futures::stream::once(async { WorkEvent::Hello { at: now_ms() } });
    let live = BroadcastStream::new(w.subscribe()).filter_map(move |r| {
        let thread = thread.clone();
        async move {
            match r {
                Ok(e) => match (&thread, e.thread_id()) {
                    (Some(t), Some(id)) if t != id => None,
                    _ => Some(e),
                },
                Err(BroadcastStreamRecvError::Lagged(_)) => Some(WorkEvent::Resync),
            }
        }
    });
    hello.chain(live)
}

/// [`event_stream`] as `Result`s, the shape Dioxus `ServerEvents::from_stream` takes.
pub fn event_results(w: &Work) -> impl Stream<Item = Result<WorkEvent, axum::BoxError>> + Send + use<> {
    event_stream(w, None).map(Ok)
}
