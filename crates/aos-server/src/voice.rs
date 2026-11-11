//! Voice: chunked audio upload and transcription through Codex on the owner's ChatGPT login.
//!
//! | Route | What |
//! |---|---|
//! | `GET /api/voice/status` | Codex login state (never the token) |
//! | `PUT /api/voice/takes/{id}/parts/{p}/chunks/{n}` | one ~1 s chunk of a part; PUT again replaces it |
//! | `POST /api/voice/takes/{id}/finish` | join the chunks of every part, transcribe, return the text |
//! | `GET /api/voice/takes/{id}` | what the server holds for a take |
//! | `POST /api/voice/transcribe` | one-shot: raw audio body (`Content-Type` = its type) to text |
//!
//! A take is a folder under `AOS_VOICE_DIR` (default `~/.aos/voice`). Chunks are written to disk
//! before the reply, so a dead network or a closed app loses nothing already uploaded. Parts exist
//! because a MediaRecorder file only has a header in its first chunk: the client starts a new part
//! every few minutes, and each part is a self-contained file sent to Codex on its own.
//!
//! The transcribe call is the one the Codex desktop dictation uses (`POST
//! https://chatgpt.com/backend-api/transcribe`, multipart `file`). The token is read fresh from
//! `~/.codex/auth.json` per call, is never refreshed here (Codex rotates it) and is never logged.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine as _;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::{ApiError, DeviceAuth};
use crate::state::AppState;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/transcribe";
const MAX_CHUNK: usize = 1_900_000;

/// Why a transcription failed, with a code the app can act on.
#[derive(Debug, Clone)]
pub struct VoiceError {
    pub code: &'static str,
    pub message: String,
}

impl VoiceError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        VoiceError { code, message: message.into() }
    }
}

/// Speech to text. The Codex route in production, a fake in tests.
pub trait Transcribe: Send + Sync {
    fn transcribe(&self, audio: Vec<u8>, mime: String) -> BoxFuture<'static, Result<String, VoiceError>>;
    fn status(&self) -> Value;
}

pub struct Voice {
    pub dir: PathBuf,
    pub engine: Arc<dyn Transcribe>,
}

impl Voice {
    pub fn from_env() -> Voice {
        let dir = std::env::var_os("AOS_VOICE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| aos_core::paths::aos_home().join("voice"));
        Voice { dir, engine: Arc::new(Codex) }
    }

    pub fn with_engine(dir: PathBuf, engine: Arc<dyn Transcribe>) -> Voice {
        Voice { dir, engine }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/voice/status", get(status))
        .route("/api/voice/takes/{id}", get(take_info))
        .route("/api/voice/takes/{id}/parts/{p}/chunks/{n}", put(put_chunk))
        .route("/api/voice/takes/{id}/finish", post(finish))
        .route("/api/voice/transcribe", post(one_shot))
}

fn valid_id(id: &str) -> bool {
    (6..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn take_dir(state: &AppState, id: &str) -> Result<PathBuf, ApiError> {
    if !valid_id(id) {
        return Err(ApiError::bad_request("take id is 6 to 64 letters, digits, - or _"));
    }
    Ok(state.voice.dir.join(id))
}

fn io_err(e: std::io::Error) -> ApiError {
    tracing::error!("voice storage: {e}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "could not write the recording to disk")
}

fn base_mime(headers: &HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty() && v != "application/octet-stream")
        .unwrap_or_else(|| "audio/webm".to_string())
}

fn ext_of(mime: &str) -> &'static str {
    match mime {
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" | "video/mp4" | "audio/aac" => "m4a",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/ogg" => "ogg",
        "audio/mpeg" | "audio/mp3" => "mp3",
        _ => "webm",
    }
}

async fn status(State(state): State<AppState>, _a: DeviceAuth) -> Json<Value> {
    Json(state.voice.engine.status())
}

async fn put_chunk(
    State(state): State<AppState>,
    _a: DeviceAuth,
    UrlPath((id, p, n)): UrlPath<(String, u32, u32)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    if n >= 100_000 || p >= 1000 {
        return Err(ApiError::bad_request("part or chunk number out of range"));
    }
    if body.is_empty() {
        return Err(ApiError::bad_request("empty chunk"));
    }
    if body.len() > MAX_CHUNK {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "chunk over 1.9 MB"));
    }
    let dir = take_dir(&state, &id)?;
    tokio::fs::create_dir_all(&dir).await.map_err(io_err)?;
    // Written then renamed, so a half-written chunk is never read back as a whole one.
    let file = dir.join(format!("{p}-{n}.chunk"));
    let tmp = dir.join(format!("{p}-{n}.tmp"));
    tokio::fs::write(&tmp, &body).await.map_err(io_err)?;
    tokio::fs::rename(&tmp, &file).await.map_err(io_err)?;
    if n == 0 {
        tokio::fs::write(dir.join(format!("{p}.mime")), base_mime(&headers)).await.map_err(io_err)?;
    }
    Ok(Json(json!({ "ok": true, "bytes": body.len() })))
}

#[derive(Debug, Deserialize, Default)]
struct FinishArgs {
    /// The chunks the client wrote per part, so a missing one is reported instead of skipped.
    #[serde(default)]
    parts: Vec<PartCount>,
}

#[derive(Debug, Deserialize)]
struct PartCount {
    part: u32,
    chunks: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    text: String,
    parts: usize,
}

async fn finish(
    State(state): State<AppState>,
    _a: DeviceAuth,
    UrlPath(id): UrlPath<String>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let dir = take_dir(&state, &id)?;
    let args: FinishArgs = if body.iter().all(u8::is_ascii_whitespace) {
        FinishArgs::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiError::bad_request(format!("body is not JSON: {e}")))?
    };
    // Finishing twice returns the first answer. Codex is not called again.
    if let Ok(done) = tokio::fs::read_to_string(dir.join("result.json")).await
        && let Ok(s) = serde_json::from_str::<Saved>(&done)
    {
        return Ok(Json(json!({ "text": s.text, "parts": s.parts, "cached": true })));
    }
    if !dir.exists() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "not_found", "no audio was uploaded for this take"));
    }
    let present = chunk_index(&dir).await.map_err(io_err)?;
    let mut missing = Vec::new();
    for pc in &args.parts {
        for n in 0..pc.chunks {
            if !present.contains(&(pc.part, n)) {
                missing.push(json!({ "part": pc.part, "chunk": n }));
            }
        }
    }
    if !missing.is_empty() {
        missing.truncate(50);
        return Err(ApiError::new(StatusCode::CONFLICT, "missing_chunks", format!("missing chunks: {}", Value::Array(missing))));
    }
    let mut parts: Vec<u32> = present.iter().map(|(p, _)| *p).collect();
    parts.dedup();
    if parts.is_empty() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "not_found", "no audio was uploaded for this take"));
    }
    let mut texts = Vec::new();
    for p in &parts {
        let audio = join_part(&dir, *p, &present).await.map_err(io_err)?;
        let mime = tokio::fs::read_to_string(dir.join(format!("{p}.mime"))).await.unwrap_or_else(|_| "audio/webm".into());
        let t = state.voice.engine.transcribe(audio, mime.trim().to_string()).await.map_err(voice_api_error)?;
        texts.push(t.trim().to_string());
    }
    let text = texts.into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
    let saved = Saved { text: text.clone(), parts: parts.len() };
    let _ = tokio::fs::write(dir.join("result.json"), serde_json::to_vec(&saved).unwrap_or_default()).await;
    Ok(Json(json!({ "text": text, "parts": parts.len() })))
}

async fn take_info(State(state): State<AppState>, _a: DeviceAuth, UrlPath(id): UrlPath<String>) -> Result<Json<Value>, ApiError> {
    let dir = take_dir(&state, &id)?;
    if !dir.exists() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such take"));
    }
    let present = chunk_index(&dir).await.map_err(io_err)?;
    let done = tokio::fs::read_to_string(dir.join("result.json")).await.ok().and_then(|s| serde_json::from_str::<Saved>(&s).ok());
    Ok(Json(json!({ "chunks": present.len(), "done": done.is_some(), "text": done.map(|d| d.text) })))
}

async fn one_shot(State(state): State<AppState>, _a: DeviceAuth, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
    if body.is_empty() {
        return Err(ApiError::bad_request("empty audio"));
    }
    let text = state.voice.engine.transcribe(body.to_vec(), base_mime(&headers)).await.map_err(voice_api_error)?;
    Ok(Json(json!({ "text": text.trim() })))
}

fn voice_api_error(e: VoiceError) -> ApiError {
    let status = match e.code {
        "login_required" | "login_expired" => StatusCode::SERVICE_UNAVAILABLE,
        "bad_audio" => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::BAD_GATEWAY,
    };
    ApiError::new(status, e.code, e.message)
}

/// `(part, n)` for every chunk file in a take folder, sorted.
async fn chunk_index(dir: &Path) -> std::io::Result<Vec<(u32, u32)>> {
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(stem) = name.strip_suffix(".chunk")
            && let Some((p, n)) = stem.split_once('-')
            && let (Ok(p), Ok(n)) = (p.parse(), n.parse())
        {
            out.push((p, n));
        }
    }
    out.sort_unstable();
    Ok(out)
}

async fn join_part(dir: &Path, part: u32, present: &[(u32, u32)]) -> std::io::Result<Vec<u8>> {
    let mut all = Vec::new();
    for (_, n) in present.iter().filter(|(p, _)| *p == part) {
        all.extend(tokio::fs::read(dir.join(format!("{part}-{n}.chunk"))).await?);
    }
    Ok(all)
}

// ---------------------------------------------------------------------------------------------
// Codex

pub struct Codex;

struct Auth {
    token: String,
    account: Option<String>,
    exp: Option<i64>,
}

fn read_auth() -> Result<Auth, VoiceError> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|h| PathBuf::from(h).join(".codex"))
        })
        .ok_or_else(|| VoiceError::new("login_required", "no home folder to find the Codex login"))?;
    let raw = std::fs::read_to_string(home.join("auth.json"))
        .map_err(|_| VoiceError::new("login_required", "Codex login is missing on the PC. Open Codex and sign in."))?;
    let v: Value = serde_json::from_str(&raw).map_err(|_| VoiceError::new("login_required", "Codex login file is unreadable. Open Codex."))?;
    let token = v["tokens"]["access_token"].as_str().unwrap_or_default().to_string();
    let mode_ok = v["auth_mode"].as_str().is_none_or(|m| m == "chatgpt");
    if token.is_empty() || !mode_ok {
        return Err(VoiceError::new("login_required", "Sign in to Codex with ChatGPT on the PC."));
    }
    let exp = token
        .split('.')
        .nth(1)
        .and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p.trim_end_matches('=')).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|c| c["exp"].as_i64());
    Ok(Auth { token, account: v["tokens"]["account_id"].as_str().map(str::to_string), exp })
}

/// A multipart body with one `file` field, written by hand so no extra crate feature is needed.
fn multipart(audio: &[u8], mime: &str, filename: &str, boundary: &str) -> Vec<u8> {
    let mut b = Vec::with_capacity(audio.len() + 256);
    b.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
    );
    b.extend_from_slice(audio);
    b.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    b
}

impl Transcribe for Codex {
    fn status(&self) -> Value {
        match read_auth() {
            Ok(a) => {
                let now = chrono::Utc::now().timestamp();
                match a.exp {
                    Some(e) if e < now + 30 => json!({ "provider": "codex", "login": "expired", "message": "The Codex login on the PC has expired. Open Codex on the PC." }),
                    e => json!({ "provider": "codex", "login": "ok", "expires_in_s": e.map(|e| e - now) }),
                }
            }
            Err(e) => json!({ "provider": "codex", "login": "missing", "message": e.message }),
        }
    }

    fn transcribe(&self, audio: Vec<u8>, mime: String) -> BoxFuture<'static, Result<String, VoiceError>> {
        Box::pin(async move {
            let auth = read_auth()?;
            if auth.exp.is_some_and(|e| e < chrono::Utc::now().timestamp() + 30) {
                return Err(VoiceError::new("login_expired", "The Codex login on the PC has expired. Open Codex on the PC, then retry."));
            }
            let filename = format!("dictation.{}", ext_of(&mime));
            let boundary = format!("aos{:x}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
            let body = multipart(&audio, &mime, &filename, &boundary);
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| VoiceError::new("network_error", e.to_string()))?;
            let mut last = VoiceError::new("network_error", "could not reach Codex transcription");
            for attempt in 0..3u32 {
                let mut req = client
                    .post(ENDPOINT)
                    .bearer_auth(&auth.token)
                    .header("originator", "Codex Desktop")
                    .header("User-Agent", "Codex Desktop/26.1016.6573 (Windows NT 10.0; x64)")
                    .header("Accept", "application/json")
                    .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
                    .body(body.clone());
                if let Some(a) = &auth.account {
                    req = req.header("ChatGPT-Account-Id", a);
                }
                match req.send().await {
                    Err(_) => last = VoiceError::new("network_error", "Could not reach Codex transcription. The recording is kept; retry."),
                    Ok(r) => {
                        let st = r.status().as_u16();
                        if r.status().is_success() {
                            let v: Value = r.json().await.map_err(|_| VoiceError::new("invalid_response", "Codex returned an unreadable reply"))?;
                            return v["text"]
                                .as_str()
                                .map(str::to_string)
                                .ok_or_else(|| VoiceError::new("invalid_response", "Codex returned no transcript"));
                        }
                        last = match st {
                            401 => VoiceError::new("login_expired", "Codex rejected the saved login. Open Codex on the PC."),
                            403 => VoiceError::new("upstream_error", "This Codex account cannot use dictation."),
                            429 => VoiceError::new("upstream_error", "Codex dictation is rate limited. Wait, then retry."),
                            413 => VoiceError::new("bad_audio", "Codex says this recording is too large."),
                            400 => VoiceError::new("bad_audio", "Codex could not read this audio."),
                            s => VoiceError::new("upstream_error", format!("Codex transcription failed (HTTP {s}).")),
                        };
                        if ![408, 500, 502, 503, 504].contains(&st) {
                            return Err(last);
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(500 << attempt)).await;
            }
            Err(last)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    /// Says how many bytes it was given, so a test can see the chunks were joined in order.
    struct Fake;
    impl Transcribe for Fake {
        fn transcribe(&self, audio: Vec<u8>, mime: String) -> BoxFuture<'static, Result<String, VoiceError>> {
            Box::pin(async move { Ok(format!("{} bytes {} first {}", audio.len(), mime, audio.first().copied().unwrap_or(0))) })
        }
        fn status(&self) -> Value {
            json!({ "provider": "fake", "login": "ok" })
        }
    }

    fn app() -> (AppState, String, PathBuf) {
        let dir = std::env::temp_dir().join(format!("aos-voice-test-{:x}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let mut s = AppState::new(Arc::new(aos_core::Store::open_temporary().unwrap()));
        s.voice = Arc::new(Voice::with_engine(dir.clone(), Arc::new(Fake)));
        let t = s.store.create_device_token("test", None).unwrap().token;
        (s, t, dir)
    }

    async fn call(s: &AppState, method: &str, uri: &str, tok: Option<&str>, mime: &str, body: Vec<u8>) -> (StatusCode, Value) {
        let mut r = Request::builder().method(method).uri(uri).header(header::CONTENT_TYPE, mime);
        if let Some(t) = tok {
            r = r.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let resp = crate::api_router(s.clone()).oneshot(r.body(Body::from(body)).unwrap()).await.unwrap();
        let st = resp.status();
        let b = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (st, serde_json::from_slice(&b).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn chunks_join_in_order_and_finish_is_idempotent() {
        let (s, t, dir) = app();
        let base = "/api/voice/takes/take-abc123";
        let (st, _) = call(&s, "PUT", &format!("{base}/parts/0/chunks/0"), None, "audio/webm", vec![1]).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        // Out of order, and a retry of chunk 1 replaces it.
        for (n, byte) in [(1u32, 9u8), (0, 7), (2, 3), (1, 5)] {
            let (st, _) = call(&s, "PUT", &format!("{base}/parts/0/chunks/{n}"), Some(&t), "audio/webm;codecs=opus", vec![byte; 10]).await;
            assert_eq!(st, StatusCode::OK);
        }
        // A chunk the client wrote but the server never got is named, not skipped.
        let (st, body) = call(&s, "POST", &format!("{base}/finish"), Some(&t), "application/json", br#"{"parts":[{"part":0,"chunks":4}]}"#.to_vec()).await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "missing_chunks");
        let (st, body) = call(&s, "POST", &format!("{base}/finish"), Some(&t), "application/json", br#"{"parts":[{"part":0,"chunks":3}]}"#.to_vec()).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["text"], "30 bytes audio/webm first 7");
        let (_, again) = call(&s, "POST", &format!("{base}/finish"), Some(&t), "application/json", Vec::new()).await;
        assert_eq!(again["cached"], true);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn parts_are_transcribed_separately_and_joined() {
        let (s, t, dir) = app();
        let base = "/api/voice/takes/take-parts1";
        for p in 0..2u32 {
            call(&s, "PUT", &format!("{base}/parts/{p}/chunks/0"), Some(&t), "audio/mp4", vec![p as u8 + 1; 4]).await;
        }
        let (_, body) = call(&s, "POST", &format!("{base}/finish"), Some(&t), "application/json", Vec::new()).await;
        assert_eq!(body["text"], "4 bytes audio/mp4 first 1 4 bytes audio/mp4 first 2");
        assert_eq!(body["parts"], 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn bad_ids_and_one_shot() {
        let (s, t, dir) = app();
        let (st, _) = call(&s, "PUT", "/api/voice/takes/a..b/parts/0/chunks/0", Some(&t), "audio/webm", vec![1]).await;
        assert!(st == StatusCode::BAD_REQUEST || st == StatusCode::NOT_FOUND);
        let (st, body) = call(&s, "POST", "/api/voice/transcribe", Some(&t), "audio/wav", vec![5; 8]).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["text"], "8 bytes audio/wav first 5");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn multipart_body_is_well_formed() {
        let b = multipart(b"AB", "audio/wav", "dictation.wav", "XX");
        let s = String::from_utf8(b).unwrap();
        assert!(s.starts_with("--XX\r\nContent-Disposition: form-data; name=\"file\"; filename=\"dictation.wav\"\r\nContent-Type: audio/wav\r\n\r\nAB\r\n--XX--"));
    }
}
