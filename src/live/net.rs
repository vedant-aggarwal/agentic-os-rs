//! Talking to the Agentic OS server, and keeping small things on the device.
//!
//! - HTTP: `POST /api/tools/{name}`, `POST /api/sync/push`, `POST /api/pair/claim`, Nora's
//!   `/api/nora/*`. The device token goes in an `Authorization: Bearer` header on every call
//!   (reqwest on native, fetch on wasm). On web the token is also set as the `aos_token` cookie so
//!   Dioxus server functions and SSE (Work tab) pass the server's gate.
//! - Storage: a JSON file per key in the app's data folder on native (inside the app sandbox on
//!   iOS), `localStorage` on web. No platform keychain yet (see WIREUP-BUILD.md).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Where the server is and who this device is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Conn {
    pub url: String,
    pub token: String,
    pub device_id: String,
    pub device_name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NetError {
    /// No connection, timeout, DNS: try again later.
    Offline(String),
    /// 401: the token was refused. Never clears the pairing by itself.
    Unauthorized,
    /// The server understood and said no (bad arguments, unknown tool). Retrying won't help.
    Rejected { code: String, message: String },
    /// 5xx or a body we could not read.
    Server(String),
}

impl NetError {
    pub fn message(&self) -> String {
        match self {
            NetError::Offline(m) => format!("Offline: {m}"),
            NetError::Unauthorized => "The server refused this device's token. Re-pair in You → Server.".into(),
            NetError::Rejected { message, .. } => message.clone(),
            NetError::Server(m) => format!("Server error: {m}"),
        }
    }

    pub fn is_not_found_tool(&self) -> bool {
        matches!(self, NetError::Rejected { code, .. } if code == "unknown_tool")
    }
}

fn client() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        let b = reqwest::Client::builder();
        #[cfg(not(target_arch = "wasm32"))]
        let b = b.timeout(std::time::Duration::from_secs(20)).connect_timeout(std::time::Duration::from_secs(6));
        b.build().expect("http client")
    })
}

pub fn join(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

async fn send(req: reqwest::RequestBuilder) -> Result<Value, NetError> {
    let resp = req.send().await.map_err(|e| NetError::Offline(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| NetError::Offline(e.to_string()))?;
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
    if status.as_u16() == 401 {
        return Err(NetError::Unauthorized);
    }
    if status.is_client_error() {
        let err = body.get("error").cloned().unwrap_or(body.clone());
        let code = err.get("code").and_then(Value::as_str).unwrap_or("bad_request").to_string();
        let message = err.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| err.to_string());
        return Err(NetError::Rejected { code, message });
    }
    if !status.is_success() {
        return Err(NetError::Server(format!("{status}: {}", text.chars().take(200).collect::<String>())));
    }
    Ok(body)
}

pub async fn get(conn: &Conn, path: &str) -> Result<Value, NetError> {
    send(client().get(join(&conn.url, path)).bearer_auth(&conn.token)).await
}

pub async fn post(conn: &Conn, path: &str, body: &Value) -> Result<Value, NetError> {
    send(client().post(join(&conn.url, path)).bearer_auth(&conn.token).json(body)).await
}

pub async fn tool(conn: &Conn, name: &str, args: Value) -> Result<Value, NetError> {
    post(conn, &format!("/api/tools/{name}"), &args).await
}

/// Claim a 6-digit pairing code. Returns the new connection.
pub async fn pair(url: &str, code: &str, name: &str, platform: &str) -> Result<Conn, NetError> {
    let body = json!({ "code": code.trim(), "name": name, "platform": platform });
    let v = send(client().post(join(url, "/api/pair/claim")).json(&body)).await?;
    let token = v["token"].as_str().unwrap_or_default().to_string();
    if token.is_empty() {
        return Err(NetError::Server("no token in the reply".into()));
    }
    Ok(Conn {
        url: url.trim_end_matches('/').to_string(),
        token,
        device_id: v["device"]["id"].as_str().unwrap_or_default().to_string(),
        device_name: v["device"]["name"].as_str().unwrap_or(name).to_string(),
    })
}

/// Server-sent events as JSON values, one callback per `data:` line. Returns when the stream ends.
pub async fn sse(conn: &Conn, path: &str, mut on: impl FnMut(Value)) -> Result<(), NetError> {
    use futures::StreamExt;
    let resp = client()
        .get(join(&conn.url, path))
        .bearer_auth(&conn.token)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .map_err(|e| NetError::Offline(e.to_string()))?;
    if resp.status().as_u16() == 401 {
        return Err(NetError::Unauthorized);
    }
    if !resp.status().is_success() {
        return Err(NetError::Server(resp.status().to_string()));
    }
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| NetError::Offline(e.to_string()))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(i) = buf.find('\n') {
            let line: String = buf.drain(..=i).collect();
            if let Some(data) = line.trim_end().strip_prefix("data:") {
                if let Ok(v) = serde_json::from_str::<Value>(data.trim()) {
                    on(v);
                }
            }
        }
    }
    Ok(())
}

/// Let server functions (Work tab) carry the token too: header on every platform, plus the
/// cookie on web. The server URL for server functions is set once per launch.
pub fn install_for_server_fns(conn: &Conn) {
    let mut headers = http::HeaderMap::new();
    if let Ok(v) = http::HeaderValue::from_str(&format!("Bearer {}", conn.token)) {
        headers.insert(http::header::AUTHORIZATION, v);
    }
    dioxus::fullstack::set_request_headers(headers);
    #[cfg(not(target_arch = "wasm32"))]
    {
        if dioxus::fullstack::get_server_url().is_empty() && !conn.url.is_empty() {
            let url: &'static str = Box::leak(conn.url.clone().into_boxed_str());
            dioxus::fullstack::set_server_url(url);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dioxus::prelude::document::eval(&format!(
            "document.cookie = 'aos_token={}; path=/; max-age=31536000; SameSite=Lax';",
            conn.token
        ));
    }
}

pub fn clear_server_fn_auth() {
    dioxus::fullstack::clear_request_headers();
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dioxus::prelude::document::eval("document.cookie = 'aos_token=; path=/; max-age=0';");
    }
}

/// This page's origin on web (the server that served it); empty on native.
pub async fn page_origin() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        return dioxus::prelude::document::eval("return location.origin;").join::<String>().await.unwrap_or_default();
    }
    #[allow(unreachable_code)]
    String::new()
}

pub fn platform() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "web"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "desktop"
    }
}

/// Wait without a runtime timer crate: the webview's `setTimeout` works on every renderer here.
pub async fn sleep_ms(ms: u32) {
    let _ = dioxus::prelude::document::eval(&format!("await new Promise(r => setTimeout(r, {ms})); return 1;"))
        .join::<i32>()
        .await;
}

// ---------------------------------------------------------------------------------------------
// Storage

#[cfg(not(target_arch = "wasm32"))]
fn store_dir() -> std::path::PathBuf {
    let base = dirs::data_local_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Library")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("agentic-os")
}

pub async fn load(key: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        return std::fs::read_to_string(store_dir().join(format!("{key}.json"))).ok();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let js = format!("try {{ return localStorage.getItem('aos.{key}') || ''; }} catch (e) {{ return ''; }}");
        let v = dioxus::prelude::document::eval(&js).join::<String>().await.unwrap_or_default();
        return if v.is_empty() { None } else { Some(v) };
    }
}

pub fn save(key: &str, value: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dir = store_dir();
        let _ = std::fs::create_dir_all(&dir);
        let tmp = dir.join(format!("{key}.json.tmp"));
        if std::fs::write(&tmp, value).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(format!("{key}.json")));
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let js = format!(
            "try {{ localStorage.setItem('aos.{key}', {}); }} catch (e) {{}}",
            serde_json::to_string(value).unwrap_or_default()
        );
        let _ = dioxus::prelude::document::eval(&js);
    }
}

pub fn remove(key: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = std::fs::remove_file(store_dir().join(format!("{key}.json")));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dioxus::prelude::document::eval(&format!("try {{ localStorage.removeItem('aos.{key}'); }} catch (e) {{}}"));
    }
}
