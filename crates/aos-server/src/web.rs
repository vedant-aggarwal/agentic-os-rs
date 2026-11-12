//! The static web UI (the React build in `web/dist`) and CORS for the phone shell.
//!
//! [`with_web`] serves a folder at `/`: real files as they are, any other non-API path gets
//! `index.html` (the app routes on the client). `/api/*` and `/mcp` never fall through to the
//! page: an unknown API path stays a JSON 404. Set `AOS_WEB_DIR` (or `aos-server --web DIR`).
//!
//! [`cors`] lets the Capacitor shell (`capacitor://localhost`, `https://localhost`, ...) call the
//! API with a Bearer token. No cookies cross origins. Extra origins: `AOS_CORS_ORIGINS`
//! (comma separated).

use std::path::{Path, PathBuf};

use axum::Router;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::any;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

/// `AOS_WEB_DIR`, if it is set and holds an `index.html`.
pub fn web_dir_from_env() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("AOS_WEB_DIR")?);
    if dir.join("index.html").is_file() {
        Some(dir)
    } else {
        tracing::warn!(dir = %dir.display(), "AOS_WEB_DIR has no index.html; not serving a web UI");
        None
    }
}

/// Serve `dir` at `/` behind the API routes already in `router`.
pub fn with_web(router: Router, dir: &Path) -> Router {
    let index = ServeFile::new(dir.join("index.html"));
    let files = ServeDir::new(dir)
        .append_index_html_on_directories(true)
        .fallback(index);
    // The page itself must revalidate so a new build shows up; hashed assets can be cached.
    let files = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .service(files);
    router
        .route("/api/{*rest}", any(api_not_found))
        .fallback_service(files)
}

async fn api_not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({ "error": { "code": "not_found", "message": "no such API route" } })),
    )
}

/// CORS for the native shell's own origins plus `AOS_CORS_ORIGINS`.
pub fn cors() -> CorsLayer {
    let mut origins: Vec<HeaderValue> = [
        "capacitor://localhost",
        "ionic://localhost",
        "https://localhost",
        "http://localhost",
    ]
    .iter()
    .map(|o| HeaderValue::from_static(o))
    .collect();
    if let Ok(extra) = std::env::var("AOS_CORS_ORIGINS") {
        origins.extend(
            extra
                .split(',')
                .map(str::trim)
                .filter(|o| !o.is_empty())
                .filter_map(|o| HeaderValue::from_str(o).ok()),
        );
    }
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .max_age(std::time::Duration::from_secs(600))
}
