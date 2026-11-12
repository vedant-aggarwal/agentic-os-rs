//! Mind OS on the server: the save endpoint for share sheets and the free enrichment stages.
//!
//! | Route | What |
//! |---|---|
//! | `POST /api/mind/save` | JSON `{text?, url?, title?, kind?, tags?, origin?}`, or raw image bytes (`Content-Type: image/*`, up to 12 MB; `?text=&title=` in the query) -> the saved item |
//!
//! Needs a paired device token, like every API route. The iOS share extension and Android share
//! intent (later) post here.
//!
//! [`worker`] runs the `fetch` and `thumb` stages of the queue (no API cost). The `tag` stage
//! (Claude) runs in Nora, on a low-priority queue that pauses during calls.

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, header};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::auth::{ApiError, DeviceAuth};
use crate::mind_fetch;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/mind/save", post(save)).layer(DefaultBodyLimit::max(mind_fetch::MAX_IMAGE))
}

#[derive(Debug, Default, Deserialize)]
struct SaveQuery {
    text: Option<String>,
    title: Option<String>,
    origin: Option<String>,
}

async fn save(State(state): State<AppState>, auth: DeviceAuth, headers: HeaderMap, Query(q): Query<SaveQuery>, body: Bytes) -> Result<Json<Value>, ApiError> {
    let ctype = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let actor = auth.id().to_string();
    let args = if ctype.starts_with("image/") || ctype.starts_with("application/octet-stream") {
        if body.is_empty() {
            return Err(ApiError::bad_request("empty body: send the image bytes"));
        }
        let mime = crate::media::sniff(&body, ctype.split(';').next().unwrap_or("").trim());
        let a = actor.clone();
        let asset = state.blocking(move |s| Ok(s.mc_add_asset(&a, &body, &mime, None)?)).await?;
        json!({ "asset_id": asset["id"], "text": q.text.unwrap_or_default(), "title": q.title, "origin": q.origin.unwrap_or_else(|| "share".into()) })
    } else {
        let mut v: Value = if body.iter().all(u8::is_ascii_whitespace) {
            json!({})
        } else {
            serde_json::from_slice(&body).map_err(|e| ApiError::bad_request(format!("body is not JSON: {e}")))?
        };
        if !v.is_object() {
            return Err(ApiError::bad_request("send a JSON object"));
        }
        if v.get("origin").is_none() {
            v["origin"] = json!("share");
        }
        v
    };
    let row = state.call_tool(actor, "mind_save".into(), args).await?;
    Ok(Json(row))
}

/// Run the free stages (fetch pages, make thumbnails) until shutdown.
pub async fn worker(state: AppState) {
    let _ = state.blocking(|s| Ok(s.mind_requeue_running()?)).await;
    loop {
        let job = state.blocking(|s| Ok(s.mind_next_job(&["fetch", "thumb"])?)).await;
        let idle = match job {
            Ok(Some((id, stage))) => {
                let err = run_stage(&state, &id, &stage).await.err();
                if let Some(e) = &err {
                    debug!(%id, %stage, error = %e, "mind stage failed");
                }
                let (i, st) = (id.clone(), stage.clone());
                if let Err(e) = state.blocking(move |s| Ok(s.mind_job_finish(&i, &st, err.as_deref())?)).await {
                    warn!(%id, error = %e, "mind job finish failed");
                }
                state.bus.poke();
                false
            }
            Ok(None) => true,
            Err(e) => {
                warn!(error = %e, "mind queue read failed");
                true
            }
        };
        let wait = if idle { Duration::from_secs(3) } else { Duration::from_millis(50) };
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

async fn run_stage(state: &AppState, id: &str, stage: &str) -> Result<(), String> {
    let i = id.to_string();
    let input = state.blocking(move |s| Ok(s.mind_job_input(&i)?)).await.map_err(|e| e.to_string())?;
    match stage {
        "fetch" => {
            let url = input["source_url"].as_str().ok_or("no link to fetch")?.to_string();
            let f = if input["kind"] == "image" && input["asset_id"].is_null() {
                aos_core::mind::jobs::Fetched { image_url: Some(url.clone()), ..Default::default() }
            } else {
                mind_fetch::fetch_page(&url).await?
            };
            let i = id.to_string();
            state.blocking(move |s| Ok(s.mind_apply_fetch(&i, &f)?)).await.map_err(|e| e.to_string())
        }
        "thumb" => {
            let (bytes, original) = match input["asset_id"].as_str() {
                Some(a) => {
                    let a = a.to_string();
                    let (path, _) = state.blocking(move |s| Ok(s.mc_asset_file(&a)?)).await.map_err(|e| e.to_string())?;
                    (tokio::fs::read(&path).await.map_err(|e| format!("image file missing: {e}"))?, None)
                }
                None => {
                    let url = input["image_url"].as_str().ok_or("no image")?.to_string();
                    let (bytes, mime) = mind_fetch::fetch_image(&url).await?;
                    let mime = crate::media::sniff(&bytes, &mime);
                    let (b, u) = (bytes.clone(), url.clone());
                    let asset = state.blocking(move |s| Ok(s.mc_add_asset("mind-worker", &b, &mime, Some(&u))?)).await.map_err(|e| e.to_string())?;
                    (bytes, asset["id"].as_str().map(String::from))
                }
            };
            let (jpg, w, h, color) = tokio::task::spawn_blocking(move || mind_fetch::thumbnail(&bytes)).await.map_err(|e| e.to_string())??;
            let i = id.to_string();
            state
                .blocking(move |s| {
                    let thumb = s.mc_add_asset("mind-worker", &jpg, "image/jpeg", None)?;
                    s.mind_apply_thumb(&i, original.as_deref(), thumb["id"].as_str().unwrap_or_default(), w as i64, h as i64, Some(&color))?;
                    Ok(())
                })
                .await
                .map_err(|e| e.to_string())
        }
        other => Err(format!("unknown stage {other}")),
    }
}
