//! Personal files for the apps: the hero's presets and 3D models. They live in
//! `paths::personal_dir()` (default `~/.aos/personal`, or `AOS_PERSONAL`), never in the repo or
//! the app bundle, so a public build carries none of them.
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/personal/{*path}` | one file under the personal folder (device token required) |
//!
//! Only plain files are served: no directory listings, no `..`, no hidden names, no symlinks
//! that lead outside the folder. Responses carry an `ETag` (size + mtime) and answer
//! `If-None-Match` with 304 (weak tags match too: Cloudflare weakens them when it compresses),
//! so the phone can revalidate its cached copy cheaply.

use std::path::{Path as FsPath, PathBuf};
use std::time::UNIX_EPOCH;

use axum::extract::Path;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use aos_core::paths;

use crate::auth::{ApiError, DeviceAuth};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/personal/{*path}", get(serve))
}

async fn serve(_auth: DeviceAuth, Path(path): Path<String>, headers: HeaderMap) -> Result<Response, ApiError> {
    serve_from(&paths::personal_dir(), &path, &headers).await
}

fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such personal file")
}

/// `rel` as a path under `root`, or `None` when any segment is unsafe. Segments may use
/// letters, digits, `-`, `_` and `.`, must not start with `.`, and there are at most 8.
pub(crate) fn safe_join(root: &FsPath, rel: &str) -> Option<PathBuf> {
    let segs: Vec<&str> = rel.split('/').collect();
    if segs.is_empty() || segs.len() > 8 {
        return None;
    }
    let mut out = root.to_path_buf();
    for s in segs {
        let ok = !s.is_empty()
            && s.len() <= 128
            && !s.starts_with('.')
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if !ok {
            return None;
        }
        out.push(s);
    }
    Some(out)
}

pub(crate) fn content_type(path: &FsPath) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("glb") => "model/gltf-binary",
        Some("gltf") => "model/gltf+json",
        Some("json") => "application/json",
        Some("webp") => "image/webp",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("ktx2") => "image/ktx2",
        Some("bin") => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

pub(crate) async fn serve_from(root: &FsPath, rel: &str, headers: &HeaderMap) -> Result<Response, ApiError> {
    let path = safe_join(root, rel).ok_or_else(not_found)?;
    // Resolve symlinks and make sure the file is still inside the folder.
    let (Ok(root_c), Ok(file_c)) = (tokio::fs::canonicalize(root).await, tokio::fs::canonicalize(&path).await) else {
        return Err(not_found());
    };
    if !file_c.starts_with(&root_c) {
        return Err(not_found());
    }
    let meta = tokio::fs::metadata(&file_c).await.map_err(|_| not_found())?;
    if !meta.is_file() {
        return Err(not_found());
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    let etag = format!("\"{:x}-{:x}\"", meta.len(), mtime);
    let ctype = content_type(&file_c);
    // Presets change by hand and must show up at once; models are big and change rarely.
    let cache = if ctype == "application/json" { "private, no-cache" } else { "private, max-age=86400" };
    let fresh = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == etag));
    let mut resp = if fresh {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let bytes = tokio::fs::read(&file_c).await.map_err(|_| not_found())?;
        let mut r = bytes.into_response();
        r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
        r
    };
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_blocks_traversal_and_hidden_names() {
        let root = FsPath::new("/data/personal");
        assert!(safe_join(root, "hero3d.json").is_some());
        assert!(safe_join(root, "character/stage-01.glb").is_some());
        for bad in ["", "/", "..", "../x", "a/../b", "a//b", ".env", "a/.git/x", "a\\b", "c:x", "a/", "%2e%2e/x", "a b"] {
            assert!(safe_join(root, bad).is_none(), "{bad:?} should be refused");
        }
        assert!(safe_join(root, &"a/".repeat(9).trim_end_matches('/').to_string()).is_none());
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type(FsPath::new("x/stage-01.glb")), "model/gltf-binary");
        assert_eq!(content_type(FsPath::new("hero3d.json")), "application/json");
        assert_eq!(content_type(FsPath::new("t.WEBP")), "image/webp");
    }
}
