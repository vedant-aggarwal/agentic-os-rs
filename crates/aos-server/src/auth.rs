//! Request auth and API errors.
//!
//! Handlers that need a paired device take a [`DeviceAuth`] argument. It reads
//! `Authorization: Bearer <token>`, or `?token=` for browser `EventSource` and WebSocket, which
//! can't set headers, or an `aos_token` cookie. A bad token gets a 401 and nothing else happens: no session is touched.

use std::sync::Arc;

use aos_core::Store;
use aos_core::devices::Device;
use aos_tools::ToolError;
use axum::extract::{FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

/// A JSON error: `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn unauthorized() -> Self {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or unknown device token. Nothing was revoked; pair this device or check the token.",
        )
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_args", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": { "code": self.code, "message": self.message } }));
        (self.status, body).into_response()
    }
}

impl From<ToolError> for ApiError {
    fn from(e: ToolError) -> Self {
        let status = match e.code() {
            "not_found" | "unknown_tool" => StatusCode::NOT_FOUND,
            "unauthorized" => StatusCode::UNAUTHORIZED,
            "internal" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!("internal error: {e}");
        }
        ApiError::new(status, e.code(), e.to_string())
    }
}

impl From<aos_core::Error> for ApiError {
    fn from(e: aos_core::Error) -> Self {
        ToolError::Core(e).into()
    }
}

/// The paired device making this request.
#[derive(Debug, Clone)]
pub struct DeviceAuth(pub Device);

impl DeviceAuth {
    pub fn id(&self) -> &str {
        &self.0.id
    }
}

impl FromRequestParts<AppState> for DeviceAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = token_from(&parts.headers, &parts.uri).ok_or_else(ApiError::unauthorized)?;
        let device = state
            .blocking(move |store| Ok(store.authenticate(&token)?))
            .await
            .map_err(ApiError::from)?;
        device.map(DeviceAuth).ok_or_else(ApiError::unauthorized)
    }
}

/// The device token of a request: `Authorization: Bearer <token>`, then `?token=` (browser
/// `EventSource` and WebSocket can't set headers), then an `aos_token` cookie.
pub fn token_from(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .map(|t| t.trim().to_string());
    let query = || {
        uri.query()?
            .split('&')
            .find_map(|pair| pair.strip_prefix("token="))
            .map(str::to_string)
    };
    let cookie = || {
        headers
            .get_all(axum::http::header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .find_map(|c| c.trim().strip_prefix("aos_token=").map(str::to_string))
    };
    bearer
        .filter(|t| !t.is_empty())
        .or_else(|| query().filter(|t| !t.is_empty()))
        .or_else(|| cookie().filter(|t| !t.is_empty()))
}

async fn device_ok(store: Arc<Store>, headers: &HeaderMap, uri: &Uri) -> bool {
    let Some(token) = token_from(headers, uri) else {
        return false;
    };
    tokio::task::spawn_blocking(move || store.authenticate(&token).ok().flatten().is_some())
        .await
        .unwrap_or(false)
}

/// Every route of `router` needs a paired device token (the same check as `/api/tools`).
/// For routers built by other crates (`/api/nora`).
pub fn require_device(router: Router, store: Arc<Store>) -> Router {
    router.route_layer(middleware::from_fn(move |req: Request, next: Next| {
        let store = store.clone();
        async move {
            if device_ok(store, req.headers(), req.uri()).await {
                next.run(req).await
            } else {
                ApiError::unauthorized().into_response()
            }
        }
    }))
}

/// Requests whose path starts with `prefix` need a device token; others pass. Used for the
/// Dioxus server functions (under `/api/`) while pages and assets stay public. Set
/// `AOS_SERVER_FN_AUTH=off` to disable during local UI work.
pub fn require_device_under(router: Router, store: Arc<Store>, prefix: &'static str) -> Router {
    if std::env::var("AOS_SERVER_FN_AUTH").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        tracing::warn!("AOS_SERVER_FN_AUTH=off: server functions accept requests without a device token");
        return router;
    }
    router.layer(middleware::from_fn(move |req: Request, next: Next| {
        let store = store.clone();
        async move {
            if !req.uri().path().starts_with(prefix)
                || device_ok(store, req.headers(), req.uri()).await
            {
                next.run(req).await
            } else {
                ApiError::unauthorized().into_response()
            }
        }
    }))
}
