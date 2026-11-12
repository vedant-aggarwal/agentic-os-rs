//! Images for boards (Mission Control). Files live in `paths::media_dir()` (default
//! `~/.aos/media`), never in the repo; the `asset` table maps ids to files by content hash.
//!
//! | Route | What |
//! |---|---|
//! | `POST /api/media` | raw image body (`Content-Type: image/*`, up to 12 MB) -> asset row |
//! | `POST /api/media/fetch` | `{"url": "https://..."}`: the server downloads the image -> asset row |
//! | `GET /api/media/{id}` | the image (token in a header, `?token=` or the `aos_token` cookie) |
//!
//! Every route needs a paired device token.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::auth::{ApiError, DeviceAuth};
use crate::state::AppState;

pub const MAX_IMAGE: usize = 12 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/media", post(upload))
        .route("/api/media/fetch", post(fetch))
        .route("/api/media/{id}", get(serve))
        .layer(DefaultBodyLimit::max(MAX_IMAGE))
}

fn mime_of(headers: &HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Trust the bytes over the header: JPEG, PNG, GIF and WebP have clear signatures.
pub(crate) fn sniff(bytes: &[u8], claimed: &str) -> String {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg".into(),
        [0x89, b'P', b'N', b'G', ..] => "image/png".into(),
        [b'G', b'I', b'F', b'8', ..] => "image/gif".into(),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp".into(),
        _ => claimed.to_string(),
    }
}

async fn save(state: &AppState, actor: String, bytes: Bytes, mime: String, source: Option<String>) -> Result<Json<Value>, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::bad_request("empty body: send the image bytes"));
    }
    if bytes.len() > MAX_IMAGE {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "images up to 12 MB"));
    }
    let mime = sniff(&bytes, &mime);
    let row = state
        .blocking(move |store| Ok(store.mc_add_asset(&actor, &bytes, &mime, source.as_deref())?))
        .await?;
    state.bus.poke();
    Ok(Json(row))
}

async fn upload(State(state): State<AppState>, auth: DeviceAuth, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
    save(&state, auth.id().to_string(), body, mime_of(&headers), None).await
}

#[derive(Deserialize)]
struct FetchBody {
    url: String,
}

async fn fetch(State(state): State<AppState>, auth: DeviceAuth, Json(b): Json<FetchBody>) -> Result<Json<Value>, ApiError> {
    let url = b.url.trim().to_string();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(ApiError::bad_request("give an http(s) image link"));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("AgenticOS/1 (board image import)")
        .build()
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, "fetch_failed", format!("couldn't download the image: {e}")))?;
    if !resp.status().is_success() {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, "fetch_failed", format!("the site answered {}", resp.status())));
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_IMAGE) {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "images up to 12 MB"));
    }
    let mime = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, "fetch_failed", e.to_string()))?;
    save(&state, auth.id().to_string(), bytes, mime, Some(url)).await
}

async fn serve(State(state): State<AppState>, _auth: DeviceAuth, Path(id): Path<String>) -> Result<Response, ApiError> {
    if id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(ApiError::bad_request("bad asset id"));
    }
    let (path, mime) = state.blocking(move |store| Ok(store.mc_asset_file(&id)?)).await?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "not_found", "image file missing"))?;
    let mut resp = bytes.into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    // Content-addressed: the bytes behind an id never change.
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable"));
    Ok(resp)
}
