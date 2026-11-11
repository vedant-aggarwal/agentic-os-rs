//! Router tests: requests go straight into the router with `tower::ServiceExt::oneshot`.

use std::sync::Arc;
use std::time::Duration;

use aos_core::Store;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::events::{Event, watch_changes};
use crate::{AppState, api_router};

fn state() -> AppState {
    AppState::new(Arc::new(Store::open_temporary().unwrap()))
}

fn token(state: &AppState) -> String {
    state.store.create_device_token("test", None).unwrap().token
}

async fn send(
    state: &AppState,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    let resp = api_router(state.clone())
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
async fn health_is_public_tools_are_not() {
    let s = state();
    let (code, body) = send(&s, "GET", "/api/health", None, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["ok"], true);
    let (code, body) = send(&s, "GET", "/api/tools", None, Value::Null).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn bad_tokens_never_revoke_good_ones() {
    let s = state();
    let good = token(&s);
    for _ in 0..5 {
        let (code, _) = send(&s, "GET", "/api/tools", Some("aos_bogus"), Value::Null).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }
    let (code, body) = send(&s, "GET", "/api/tools", Some(&good), Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert!(body["tools"].as_array().unwrap().len() >= 15);
}

#[tokio::test]
async fn pairing_flow_and_revoke() {
    let s = state();
    let code = s.store.create_pairing_code(10).unwrap().code;
    let (status, _) = send(
        &s,
        "POST",
        "/api/pair/claim",
        None,
        json!({"code": "12345x", "name": "x"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, paired) = send(
        &s,
        "POST",
        "/api/pair/claim",
        None,
        json!({"code": code, "name": "Pixel", "platform": "android"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let t = paired["token"].as_str().unwrap().to_string();
    let (status, _) = send(&s, "GET", "/api/devices", Some(&t), Value::Null).await;
    assert_eq!(status, StatusCode::OK);

    let id = paired["device"]["id"].as_str().unwrap();
    let (status, _) = send(
        &s,
        "POST",
        &format!("/api/devices/{id}/revoke"),
        Some(&t),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&s, "GET", "/api/devices", Some(&t), Value::Null).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "only an explicit revoke ends a session"
    );
}

#[tokio::test]
async fn tool_call_then_sync_pull() {
    let s = state();
    let t = token(&s);
    let since = s.store.latest_version().unwrap();
    let (status, task) = send(
        &s,
        "POST",
        "/api/tools/task_add",
        Some(&t),
        json!({"title": "From phone"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task["title"], "From phone");

    let (status, err) = send(
        &s,
        "POST",
        "/api/tools/task_done",
        Some(&t),
        json!({"id": "nope"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(err["error"]["code"], "not_found");

    let (_, today) = send(&s, "POST", "/api/tools/today", Some(&t), Value::Null).await;
    assert!(today["score"].is_object());

    let (status, batch) = send(
        &s,
        "GET",
        &format!("/api/sync/changes?since={since}"),
        Some(&t),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let change = &batch["changes"][0];
    assert_eq!(change["entity"], "task");
    assert_eq!(change["row"]["title"], "From phone");
}

#[tokio::test]
async fn outbox_push_is_idempotent() {
    let s = state();
    let t = token(&s);
    let ops = json!({"ops": [
        {"op_id": "phone-1", "tool": "task_add", "args": {"title": "Offline task"}},
        {"op_id": "phone-2", "tool": "task_add", "args": {"nope": 1}}
    ]});
    let (status, first) = send(&s, "POST", "/api/sync/push", Some(&t), ops.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["results"][0]["outcome"]["ok"], true);
    assert_eq!(first["results"][1]["outcome"]["ok"], false);

    let (_, second) = send(&s, "POST", "/api/sync/push", Some(&t), ops).await;
    assert_eq!(second["results"][0]["replayed"], true);
    assert_eq!(
        second["results"][0]["outcome"]["result"]["id"],
        first["results"][0]["outcome"]["result"]["id"]
    );
    let tasks = s.store.list_tasks(&Default::default()).unwrap();
    assert_eq!(tasks.len(), 1, "the resend did not add a second task");
}

#[tokio::test]
async fn mcp_over_http() {
    let s = state();
    let t = token(&s);
    let (status, list) = send(
        &s,
        "POST",
        "/mcp",
        Some(&t),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "today")
    );

    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "task_add", "arguments": {"title": "Via MCP"}}});
    let (_, resp) = send(&s, "POST", "/mcp", Some(&t), call).await;
    assert_eq!(resp["result"]["structuredContent"]["title"], "Via MCP");

    let (status, _) = send(
        &s,
        "POST",
        "/mcp",
        Some(&t),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, _) = send(
        &s,
        "POST",
        "/mcp",
        None,
        json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bus_publishes_writes_from_any_process() {
    let s = state();
    let mut rx = s.bus.subscribe();
    let watcher = tokio::spawn(watch_changes(s.clone()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    // A write that bypasses the server entirely, like the `aos` CLI in another process.
    s.store
        .add_task(
            "cli",
            aos_core::tasks::NewTask {
                title: "From CLI".into(),
                ..Default::default()
            },
        )
        .unwrap();

    let ev = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    match ev {
        Event::Change { entity, actor, .. } => {
            assert_eq!(entity, "task");
            assert_eq!(actor.as_deref(), Some("cli"));
        }
        other => panic!("unexpected {other:?}"),
    }
    s.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(3), watcher)
        .await
        .unwrap()
        .unwrap();
}
