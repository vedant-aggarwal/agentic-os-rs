//! The voice half of a call: `codex app-server` realtime on the owner's ChatGPT login (no API key,
//! no per-minute cost). Port of the old app's `nora-codex-call.ts`, checked on codex-cli 0.167.0:
//!
//! - WebRTC with `version: "v3"` works on a ChatGPT login; the websocket transport refuses it.
//! - The phone's offer goes to `thread/realtime/start`; Codex answers with `thread/realtime/sdp`
//!   and the phone then talks to OpenAI directly. Audio never passes through this server.
//! - Her persona goes in `initialItems` (developer role). A custom `prompt` stops the voice from
//!   handing work off.
//! - `clientManagedHandoffs: true`: when the voice hands a job off, Codex starts a turn on the
//!   thread. We stop that turn at once and run the job as a Claude turn instead (`crate::call`),
//!   then speak the result with `thread/realtime/appendSpeech`. Codex only provides the voice.
//!
//! Codex runs in its own home (`~/.aos/nora/codex-home`): `auth.json` is a link to the owner's (one
//! login, one refresh token), and `config.toml` turns off plugins, apps, skills and the rest of a
//! coding setup, which made a cold start slow in the old app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

/// Voices realtime v3 accepts (`ThreadRealtimeStartParams`). `marin` is not one of them.
pub const V3_VOICES: [&str; 25] = [
    "juniper", "maple", "spruce", "ember", "vale", "breeze", "arbor", "sol", "cove", "aube", "bubbie", "haetsal", "hanul", "himari",
    "leher", "miko", "neer", "porto", "rindo", "rio", "rivage", "selva", "sonna", "tinta", "viola",
];
pub const DEFAULT_VOICE: &str = "leher";

pub fn voice_or_default(v: &str) -> &'static str {
    V3_VOICES.iter().find(|x| x.eq_ignore_ascii_case(v.trim())).copied().unwrap_or(DEFAULT_VOICE)
}

/// Codex features a relay thread runs without (names from `codex features list`).
const FEATURES_OFF: [&str; 15] = [
    "code_mode", "code_mode_only", "multi_agent", "multi_agent_v2", "skill_search", "browser_use", "browser_use_external",
    "computer_use", "apps", "plugins", "memories", "image_generation", "goals", "hooks", "shell_tool",
];

/// What the Codex thread is told. It never answers: every turn it starts is stopped, and Claude
/// does the job. The line is there in case a turn slips through before the stop lands.
pub const RELAY_INSTRUCTIONS: &str = "You are a silent relay for a voice call. Another system does every job. \
Do not call tools. Reply with the single word: ok";

pub fn owner_codex_home() -> PathBuf {
    std::env::var_os("AOS_OWNER_CODEX_HOME")
        .or_else(|| std::env::var_os("CODEX_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"))
}

pub fn relay_config() -> String {
    let mut s = String::from(
        "# Nora's call relay, written by Agentic OS (aos-nora realtime.rs) on start; edits here are overwritten.\n\
check_for_update_on_startup = false\ninclude_collaboration_mode_instructions = false\n\n[features]\n",
    );
    for f in FEATURES_OFF {
        s.push_str(&format!("{f} = false\n"));
    }
    s.push_str("\n[skills]\ninclude_instructions = false\n\n[skills.bundled]\nenabled = false\n");
    s
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x == y && std::fs::metadata(b).map(|m| m.len() > 0).unwrap_or(false),
        _ => false,
    }
}

/// Make (or heal) the relay's Codex home. `None`: no owner login to link to, so use his home.
pub fn ensure_codex_home(dir: &Path, owner: &Path) -> Option<PathBuf> {
    let src = owner.join("auth.json");
    if !src.exists() {
        return None;
    }
    std::fs::create_dir_all(dir).ok()?;
    let dst = dir.join("auth.json");
    if !same_file(&src, &dst) {
        let _ = std::fs::remove_file(&dst);
        // A hard link keeps one file (Codex rewrites auth.json in place), so a token refresh on
        // either side lands in both. A copy would fork the refresh token.
        let linked = std::fs::hard_link(&src, &dst).is_ok() || symlink_file(&src, &dst);
        if !linked {
            return None;
        }
    }
    let cfg = dir.join("config.toml");
    let want = relay_config();
    if std::fs::read_to_string(&cfg).ok().as_deref() != Some(want.as_str()) {
        std::fs::write(&cfg, want).ok()?;
    }
    Some(dir.to_path_buf())
}

#[cfg(windows)]
fn symlink_file(src: &Path, dst: &Path) -> bool {
    std::os::windows::fs::symlink_file(src, dst).is_ok()
}
#[cfg(unix)]
fn symlink_file(src: &Path, dst: &Path) -> bool {
    std::os::unix::fs::symlink(src, dst).is_ok()
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// One `codex app-server` process: JSON-RPC requests out, notifications in.
pub struct Rpc {
    child: tokio::sync::Mutex<aos_work::proc::ChildProc>,
    tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    pending: Pending,
    next: AtomicU64,
    ended: Arc<AtomicBool>,
}

/// A server notification: `(method, params)`.
pub type Note = (String, Value);

impl Rpc {
    /// Start the process. Notifications arrive on the returned channel until it exits.
    pub fn spawn(bin: &Path, args: &[String], cwd: &Path, codex_home: Option<&Path>) -> Result<(Arc<Rpc>, mpsc::UnboundedReceiver<Note>)> {
        std::fs::create_dir_all(cwd).ok();
        let home = codex_home.map(|p| p.to_string_lossy().to_string());
        let env_set: Vec<(&str, &str)> = home.as_deref().map(|h| vec![("CODEX_HOME", h)]).unwrap_or_default();
        // No API key in the environment, so nothing silently bills the API instead of his login.
        let env_remove = ["OPENAI_API_KEY", "OPENAI_BASE_URL", "CLAUDECODE"];
        let mut child = aos_work::proc::spawn_env(bin, args, cwd, &env_remove, &env_set).map_err(|e| anyhow!("{e}"))?;
        let (dummy_tx, dummy_rx) = mpsc::unbounded_channel();
        drop(dummy_tx);
        let mut lines = std::mem::replace(&mut child.lines, dummy_rx);
        let (note_tx, note_rx) = mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let ended = Arc::new(AtomicBool::new(false));
        let rpc = Arc::new(Rpc {
            child: tokio::sync::Mutex::new(child),
            tx: Mutex::new(Some(out_tx)),
            pending: pending.clone(),
            next: AtomicU64::new(1),
            ended: ended.clone(),
        });
        // Writer: one task owns stdin access through the child's own queue.
        let w = rpc.clone();
        tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if !w.child.lock().await.send(line) {
                    break;
                }
            }
        });
        let r = rpc.clone();
        tokio::spawn(async move {
            while let Some(line) = lines.recv().await {
                let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
                let method = m["method"].as_str().map(String::from);
                match (m.get("id").filter(|v| !v.is_null()), method) {
                    (Some(id), None) => {
                        let Some(id) = id.as_u64() else { continue };
                        let tx = r.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                        if let Some(tx) = tx {
                            let res = match m.get("error").filter(|e| !e.is_null()) {
                                Some(e) => Err(e["message"].as_str().unwrap_or("Codex error").to_string()),
                                None => Ok(m["result"].clone()),
                            };
                            let _ = tx.send(res);
                        }
                    }
                    (Some(id), Some(method)) => {
                        // Approvals can't be asked mid-call; the relay never needs one.
                        warn!("nora call: declined Codex request {method}");
                        r.raw(json!({ "id": id, "result": { "decision": "decline" } }));
                    }
                    (None, Some(method)) => {
                        let _ = note_tx.send((method, m["params"].clone()));
                    }
                    _ => {}
                }
            }
            r.ended.store(true, Ordering::SeqCst);
            for (_, tx) in r.pending.lock().unwrap_or_else(|e| e.into_inner()).drain() {
                let _ = tx.send(Err("Codex stopped.".into()));
            }
        });
        Ok((rpc, note_rx))
    }

    fn raw(&self, v: Value) {
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(v.to_string());
        }
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.raw(if params.is_null() { json!({ "method": method }) } else { json!({ "method": method, "params": params }) });
    }

    pub async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        if self.ended() {
            bail!("Codex has stopped");
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        self.raw(json!({ "id": id, "method": method, "params": params }));
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => Err(anyhow!("{method}: {e}")),
            Ok(Err(_)) => Err(anyhow!("{method}: Codex stopped")),
            Err(_) => {
                self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                Err(anyhow!("Codex didn't answer {method} in time"))
            }
        }
    }

    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    pub async fn kill(&self) {
        self.ended.store(true, Ordering::SeqCst);
        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.child.lock().await.kill().await;
    }
}

/// Codex up with a relay thread ready: everything but the phone. Made ahead of time so
/// answering takes about the WebRTC handshake, not a cold start.
pub struct Prepared {
    pub rpc: Arc<Rpc>,
    pub thread_id: String,
    pub notes: mpsc::UnboundedReceiver<Note>,
    pub made: Instant,
}

#[derive(Clone)]
pub struct RelayConfig {
    pub bin: PathBuf,
    pub cwd: PathBuf,
    pub codex_home: Option<PathBuf>,
    /// Extra `-c key=value` pairs for `codex app-server`.
    pub config: Vec<String>,
}

impl RelayConfig {
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["app-server".to_string()];
        for c in &self.config {
            a.push("-c".into());
            a.push(c.clone());
        }
        a
    }
}

pub async fn prepare(cfg: &RelayConfig) -> Result<Prepared> {
    let t0 = Instant::now();
    let (rpc, notes) = Rpc::spawn(&cfg.bin, &cfg.args(), &cfg.cwd, cfg.codex_home.as_deref())?;
    let res = async {
        rpc.request(
            "initialize",
            json!({ "clientInfo": { "name": "agentic-os-nora-call", "version": "1.0" }, "capabilities": { "experimentalApi": true } }),
            Duration::from_secs(60),
        )
        .await?;
        rpc.notify("initialized", Value::Null);
        let t = rpc
            .request(
                "thread/start",
                json!({ "cwd": cfg.cwd, "approvalPolicy": "never", "sandbox": "read-only", "ephemeral": true, "developerInstructions": RELAY_INSTRUCTIONS }),
                Duration::from_secs(90),
            )
            .await?;
        t["thread"]["id"].as_str().map(String::from).context("Codex didn't start a thread")
    }
    .await;
    match res {
        Ok(thread_id) => {
            info!("nora call: relay ready in {:.1} s", t0.elapsed().as_secs_f32());
            Ok(Prepared { rpc, thread_id, notes, made: Instant::now() })
        }
        Err(e) => {
            rpc.kill().await;
            Err(e)
        }
    }
}

/// `thread/realtime/start` params for a WebRTC call with her persona.
pub fn start_params(thread_id: &str, sdp: &str, voice: &str, persona: &str) -> Value {
    json!({
        "threadId": thread_id,
        "outputModality": "audio",
        "version": "v3",
        "voice": voice_or_default(voice),
        "transport": { "type": "webrtc", "sdp": sdp },
        "clientManagedHandoffs": true,
        "initialItems": [{ "role": "developer", "text": persona }],
    })
}

/// Text of a `userMessage` item (what the voice handed off).
pub fn user_message_text(item: &Value) -> Option<String> {
    if item["type"] != "userMessage" {
        return None;
    }
    let text = item["content"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let t = text.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// The voice wraps a hand-off as `<realtime_delegation><input>…</input><transcript_delta>…</transcript_delta></realtime_delegation>`
/// (codex-cli 0.167.0). Returns `(what he asked, the recent transcript)`; plain text passes through.
pub fn parse_delegation(text: &str) -> (String, String) {
    let tag = |name: &str| -> Option<String> {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        let a = text.find(&open)? + open.len();
        let b = text[a..].find(&close)? + a;
        Some(text[a..b].trim().to_string())
    };
    match tag("input") {
        Some(input) if !input.is_empty() => (input, tag("transcript_delta").unwrap_or_default()),
        _ => (text.trim().to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unwraps_delegation() {
        let raw = "<realtime_delegation>
  <input>Set an alarm in 2 minutes</input>
  <transcript_delta>user:  Set an alarm</transcript_delta>
</realtime_delegation>";
        let (asked, t) = super::parse_delegation(raw);
        assert_eq!(asked, "Set an alarm in 2 minutes");
        assert_eq!(t, "user:  Set an alarm");
        assert_eq!(super::parse_delegation("plain ask").0, "plain ask");
    }

    use super::*;

    #[test]
    fn voices_fall_back() {
        assert_eq!(voice_or_default("Juniper"), "juniper");
        assert_eq!(voice_or_default("marin"), DEFAULT_VOICE);
    }

    #[test]
    fn start_params_use_v3_webrtc_and_client_handoffs() {
        let p = start_params("t1", "v=0", "x", "You are Nora.");
        assert_eq!(p["version"], "v3");
        assert_eq!(p["transport"]["type"], "webrtc");
        assert_eq!(p["clientManagedHandoffs"], true);
        assert_eq!(p["initialItems"][0]["role"], "developer");
        assert!(p.get("prompt").is_none());
    }

    #[test]
    fn reads_handed_off_text() {
        let item = json!({ "type": "userMessage", "content": [{ "type": "text", "text": "set an alarm" }, { "type": "text", "text": "in two minutes" }] });
        assert_eq!(user_message_text(&item).as_deref(), Some("set an alarm in two minutes"));
        assert!(user_message_text(&json!({ "type": "agentMessage" })).is_none());
    }

    #[test]
    fn relay_config_turns_off_plugins() {
        let c = relay_config();
        assert!(c.contains("plugins = false"));
        assert!(c.contains("[skills.bundled]"));
    }

    #[test]
    fn codex_home_links_auth() {
        let base = std::env::temp_dir().join(format!("aos-cx-{}", aos_core::ids::new_id()));
        let owner = base.join("owner");
        std::fs::create_dir_all(&owner).unwrap();
        std::fs::write(owner.join("auth.json"), "{\"a\":1}").unwrap();
        let home = ensure_codex_home(&base.join("relay"), &owner).unwrap();
        assert_eq!(std::fs::read_to_string(home.join("auth.json")).unwrap(), "{\"a\":1}");
        assert!(std::fs::read_to_string(home.join("config.toml")).unwrap().contains("apps = false"));
        assert!(ensure_codex_home(&base.join("x"), &base.join("nobody")).is_none());
        let _ = std::fs::remove_dir_all(base);
    }
}
