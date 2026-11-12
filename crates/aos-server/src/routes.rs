//! HTTP routes.
//!
//! | Route | Auth | What |
//! |---|---|---|
//! | `GET /api/health` | no | liveness and the newest change version |
//! | `POST /api/pair/claim` | no | 6-digit code -> device token |
//! | `GET /api/tools` | yes | every tool with its JSON Schema |
//! | `POST /api/tools/{name}` | yes | call a tool; body = JSON arguments |
//! | `GET /api/sync/changes?since=N` | yes | rows changed after version N |
//! | `POST /api/sync/push` | yes | apply an outbox of tool calls, idempotent by `op_id` |
//! | `GET /api/events` | yes | server-sent events from the bus |
//! | `GET /api/ws` | yes | the same events over a WebSocket |
//! | `GET /api/devices`, `POST /api/devices/{id}/revoke` | yes | list / revoke devices |
//! | `POST /mcp` | yes | MCP over streamable HTTP (JSON responses) |
//! | `/api/voice/*` | yes | chunked audio upload and transcription (see `voice.rs`) |
//! | `/api/media*` | yes | board images: upload, fetch from a link, serve (see `media.rs`) |

use std::convert::Infallible;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use aos_tools::mcp::{self, McpAction};

use crate::auth::{ApiError, DeviceAuth};
use crate::events::Event;
use crate::state::AppState;

type ApiResult<T = Json<Value>> = Result<T, ApiError>;

/// Every API route with its layers, ready to merge into a bigger router.
pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/pair/claim", post(pair_claim))
        .route("/api/tools", get(list_tools))
        .route("/api/tools/{name}", post(call_tool))
        .route("/api/sync/changes", get(sync_changes))
        .route("/api/sync/push", post(sync_push))
        .route("/api/events", get(events_sse))
        .route("/api/ws", get(events_ws))
        .route("/api/devices", get(list_devices))
        .route("/api/devices/{id}/revoke", post(revoke_device))
        .route("/mcp", post(mcp_post).get(mcp_get))
        .merge(crate::voice::routes())
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        // Images carry their own 12 MB limit, so they join after the 2 MB layer.
        .merge(crate::media::routes())
        .layer(TraceLayer::new_for_http())
        .layer(CatchPanicLayer::new())
        .with_state(state)
}

/// Parse a request body as JSON; an empty body means `null` (no arguments).
fn json_body(body: &Bytes) -> Result<Value, ApiError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Null);
    }
    serde_json::from_slice(body)
        .map_err(|e| ApiError::bad_request(format!("body is not JSON: {e}")))
}

async fn health(State(state): State<AppState>) -> ApiResult {
    let latest = state.blocking(|s| Ok(s.latest_version()?)).await?;
    Ok(Json(
        json!({ "ok": true, "version": env!("CARGO_PKG_VERSION"), "latest": latest }),
    ))
}

#[derive(Deserialize)]
struct ClaimBody {
    code: String,
    name: String,
    platform: Option<String>,
}

async fn pair_claim(State(state): State<AppState>, Json(body): Json<ClaimBody>) -> ApiResult {
    let paired = state
        .blocking(move |s| {
            Ok(s.claim_pairing_code(&body.code, &body.name, body.platform.as_deref())?)
        })
        .await
        .map_err(|e| match e.code() {
            "unauthorized" => ApiError::new(
                StatusCode::UNAUTHORIZED,
                "bad_code",
                "code is wrong, used or expired",
            ),
            _ => e.into(),
        })?;
    tracing::info!(device = %paired.device.id, name = %paired.device.name, "device paired");
    Ok(Json(json!(paired)))
}

async fn list_tools(State(state): State<AppState>, _auth: DeviceAuth) -> ApiResult {
    let tools: Vec<Value> = state
        .registry
        .tools()
        .iter()
        .map(|t| t.describe())
        .collect();
    Ok(Json(json!({ "tools": tools })))
}

async fn call_tool(
    State(state): State<AppState>,
    auth: DeviceAuth,
    Path(name): Path<String>,
    body: Bytes,
) -> ApiResult {
    let args = json_body(&body)?;
    let result = state.call_tool(auth.id().to_string(), name, args).await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct SinceQuery {
    #[serde(default)]
    since: i64,
    limit: Option<usize>,
}

async fn sync_changes(
    State(state): State<AppState>,
    _auth: DeviceAuth,
    Query(q): Query<SinceQuery>,
) -> ApiResult {
    let batch = state
        .blocking(move |s| Ok(s.changes_since(q.since, q.limit.unwrap_or(1000))?))
        .await?;
    Ok(Json(json!(batch)))
}

#[derive(Deserialize)]
struct PushBody {
    ops: Vec<PushOp>,
}

#[derive(Deserialize)]
struct PushOp {
    /// Made by the client when it queued the op; resending the same id is a no-op.
    op_id: String,
    tool: String,
    #[serde(default)]
    args: Value,
}

/// Apply a client's outbox in order. Each op is a tool call; its result is stored by `op_id`
/// so a retry after a dropped connection returns the first result instead of acting twice.
async fn sync_push(
    State(state): State<AppState>,
    auth: DeviceAuth,
    Json(body): Json<PushBody>,
) -> ApiResult {
    let mut results = Vec::with_capacity(body.ops.len());
    for op in body.ops {
        let op_id = op.op_id.clone();
        if let Some(previous) = state.blocking(move |s| Ok(s.applied_op(&op_id)?)).await? {
            results.push(json!({ "op_id": op.op_id, "replayed": true, "outcome": previous }));
            continue;
        }
        let outcome = match state
            .call_tool(auth.id().to_string(), op.tool.clone(), op.args)
            .await
        {
            Ok(result) => json!({ "ok": true, "result": result }),
            // Caller errors are final (retrying won't help), so they are remembered too.
            Err(e) if e.is_caller_error() => {
                json!({ "ok": false, "error": { "code": e.code(), "message": e.to_string() } })
            }
            Err(e) => return Err(e.into()),
        };
        let (op_id, tool, device, stored) = (
            op.op_id.clone(),
            op.tool,
            auth.id().to_string(),
            outcome.clone(),
        );
        state
            .blocking(move |s| Ok(s.record_applied_op(&op_id, Some(&device), &tool, &stored)?))
            .await?;
        results.push(json!({ "op_id": op.op_id, "replayed": false, "outcome": outcome }));
    }
    let latest = state.blocking(|s| Ok(s.latest_version()?)).await?;
    Ok(Json(json!({ "results": results, "latest": latest })))
}

/// Bus events as an SSE stream: `hello` first, then changes. Ends on shutdown.
fn event_stream(state: &AppState, latest: i64) -> impl Stream<Item = Event> + Send + use<> {
    let hello = futures::stream::once(async move { Event::Hello { latest } });
    let live = BroadcastStream::new(state.bus.subscribe()).map(|r| match r {
        Ok(ev) => ev,
        Err(BroadcastStreamRecvError::Lagged(_)) => Event::Resync,
    });
    hello
        .chain(live)
        .take_until(state.shutdown.clone().cancelled_owned())
}

async fn events_sse(
    State(state): State<AppState>,
    _auth: DeviceAuth,
) -> ApiResult<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>> {
    let latest = state.blocking(|s| Ok(s.latest_version()?)).await?;
    let stream = event_stream(&state, latest).map(|ev| {
        let name = match &ev {
            Event::Hello { .. } => "hello",
            Event::Change { .. } => "change",
            Event::Resync => "resync",
        };
        Ok(SseEvent::default()
            .event(name)
            .json_data(&ev)
            .unwrap_or_default())
    });
    // A comment every 15 s keeps proxies and Cloudflare Tunnel from closing the stream.
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

async fn events_ws(
    State(state): State<AppState>,
    _auth: DeviceAuth,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| ws_session(state, socket))
}

/// Push bus events as JSON text frames; answer `{"type":"ping"}` with `{"type":"pong"}`.
async fn ws_session(state: AppState, mut socket: WebSocket) {
    let mut rx = state.bus.subscribe();
    let latest = state
        .blocking(|s| Ok(s.latest_version()?))
        .await
        .unwrap_or(0);
    let send = |ev: &Event| Message::Text(serde_json::to_string(ev).unwrap_or_default().into());
    if socket.send(send(&Event::Hello { latest })).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            ev = rx.recv() => {
                let ev = match ev {
                    Ok(ev) => ev,
                    Err(RecvError::Lagged(_)) => Event::Resync,
                    Err(RecvError::Closed) => break,
                };
                if socket.send(send(&ev)).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let is_ping = serde_json::from_str::<Value>(&text)
                        .is_ok_and(|v| v.get("type").and_then(Value::as_str) == Some("ping"));
                    if is_ping && socket.send(Message::Text(r#"{"type":"pong"}"#.into())).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn list_devices(State(state): State<AppState>, _auth: DeviceAuth) -> ApiResult {
    let devices = state.blocking(|s| Ok(s.list_devices()?)).await?;
    Ok(Json(json!({ "devices": devices })))
}

async fn revoke_device(
    State(state): State<AppState>,
    auth: DeviceAuth,
    Path(id): Path<String>,
) -> ApiResult {
    let device = state.blocking(move |s| Ok(s.revoke_device(&id)?)).await?;
    tracing::info!(revoked = %device.id, by = %auth.id(), "device revoked");
    Ok(Json(json!(device)))
}

/// MCP streamable HTTP, JSON-response mode: one JSON-RPC message in, one JSON response out
/// (202 Accepted for notifications). Tools run as the calling device.
async fn mcp_post(State(state): State<AppState>, auth: DeviceAuth, body: Bytes) -> Response {
    let text = String::from_utf8_lossy(&body);
    match mcp::route_text(&state.registry, &text) {
        McpAction::Respond(v) => Json(v).into_response(),
        McpAction::Nothing => StatusCode::ACCEPTED.into_response(),
        McpAction::Call { id, name, args } => {
            let result = state.call_tool(auth.id().to_string(), name, args).await;
            Json(mcp::call_response(id, result)).into_response()
        }
    }
}

/// We don't open server-to-client MCP streams.
async fn mcp_get() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}
