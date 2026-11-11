//! Grok Bot bridge.
//!
//! **Read side** (built): Grok Bot keeps each chat's recent transcript in
//! `%APPDATA%\Grok Bot\sand-client-persistence\<base32 name>.blob`, where the decoded name ends in
//! `transcript.replicas.<chat-id>` and the body is plain JSON
//! `{"schemaVersion":1,"value":{"entries":[...]}}`. The roster (`...roster.last-roster`) maps chat
//! ids to names. We watch the folder with `notify` (debounced), parse Nora's transcript and append
//! new entries to the thread, deduped by entry id. **Read only: these files are never written.**
//!
//! **Write side**: [`GrokWriter`] types into the Grok window ([`crate::grok_uia`]); the queue,
//! retries and transcript confirmation live in [`crate::grok_write`]. Our own typed messages come
//! back through the transcript as `user` entries and are skipped by [`ingest`] (see [`crate::grok_write::is_own_echo`]).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use data_encoding::BASE32_NOPAD;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::events::NoraEvent;
use crate::nora::Nora;
use crate::store::NewMsg;

#[derive(Debug, Clone)]
pub struct GrokConfig {
    /// The `sand-client-persistence` folder. `AOS_GROK_DIR`, default under `%APPDATA%\Grok Bot`.
    pub dir: Option<PathBuf>,
    /// Nora's chat id in Grok Bot. `AOS_GROK_NORA_CHAT`, else the roster entry whose name starts with `chat_name`.
    pub chat_id: Option<String>,
    pub chat_name: String,
    /// On the very first run, import only entries from the last N hours.
    pub backfill_hours: i64,
}

impl GrokConfig {
    pub fn from_env() -> GrokConfig {
        let dir = std::env::var_os("AOS_GROK_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::config_dir().map(|d| d.join("Grok Bot").join("sand-client-persistence")));
        GrokConfig {
            dir,
            chat_id: std::env::var("AOS_GROK_NORA_CHAT").ok().filter(|s| !s.trim().is_empty()),
            chat_name: std::env::var("AOS_GROK_NORA_NAME").unwrap_or_else(|_| "Nora".into()),
            backfill_hours: std::env::var("AOS_GROK_BACKFILL_HOURS").ok().and_then(|v| v.parse().ok()).unwrap_or(24),
        }
    }
}

/// Why a send did not happen. Only [`WriteError::Failed`] counts against the retry budget.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteError {
    /// Grok Bot is not running and could not be started in time.
    NotRunning(String),
    /// Windows is locked: wait for him.
    Locked,
    /// Grok is still replying, or a draft sits in the composer: try again shortly.
    Busy(String),
    Failed(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::NotRunning(m) | WriteError::Busy(m) | WriteError::Failed(m) => f.write_str(m),
            WriteError::Locked => f.write_str("Windows is locked"),
        }
    }
}

/// The write side: put text into Nora's Grok Bot chat and press send. Confirmation (the entry
/// showing up in the transcript) is the runner's job, see [`crate::grok_write`].
pub trait GrokWriter: Send + Sync {
    fn send(&self, text: &str) -> Result<(), WriteError>;
    fn describe(&self) -> String {
        "none".into()
    }
}

/// Used where UI Automation is unavailable (non-Windows, tests, `AOS_GROK_WRITE=0`).
pub struct NotYetWriter;

impl GrokWriter for NotYetWriter {
    fn send(&self, _text: &str) -> Result<(), WriteError> {
        Err(WriteError::Failed("Grok write side is off".into()))
    }
}

/// Decode a persistence file name (`<base32>.blob`) to its slice name.
pub fn decode_name(file_name: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".blob")?;
    let bytes = BASE32_NOPAD.decode(stem.to_ascii_uppercase().as_bytes()).ok()?;
    String::from_utf8(bytes).ok()
}

/// Find Nora's chat id in the roster file.
pub fn chat_id_from_roster(dir: &Path, name_prefix: &str) -> Option<String> {
    let prefix = name_prefix.to_lowercase();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if !decode_name(&fname).is_some_and(|n| n.ends_with("roster.last-roster")) {
            continue;
        }
        let v: Value = serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()?;
        return v["value"]["rows"].as_array()?.iter().find_map(|r| {
            let name = r["name"].as_str()?.to_lowercase();
            name.starts_with(&prefix).then(|| r["id"].as_str().map(String::from)).flatten()
        });
    }
    None
}

/// The transcript file for a chat id.
pub fn transcript_path(dir: &Path, chat_id: &str) -> Option<PathBuf> {
    let suffix = format!("transcript.replicas.{chat_id}");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| decode_name(&e.file_name().to_string_lossy()).is_some_and(|n| n.ends_with(&suffix)))
        .map(|e| e.path())
}

#[derive(Debug, Clone, PartialEq)]
pub struct GrokEntry {
    pub id: String,
    /// `user` (he typed it in Grok Bot) or `grok` (Nora on Grok wrote it).
    pub role: &'static str,
    pub text: String,
    pub at: i64,
}

/// Parse a transcript body. Skips streaming partials, events and attachments.
pub fn parse_transcript(body: &[u8]) -> Result<Vec<GrokEntry>> {
    let v: Value = serde_json::from_slice(body).context("transcript is not JSON (mid-write?)")?;
    let entries = v["value"]["entries"].as_array().context("no entries")?;
    let mut out = Vec::new();
    for e in entries {
        let (Some(id), Some(at)) = (e["id"].as_str(), e["timestampMs"].as_i64()) else { continue };
        if e["isStreaming"].as_bool() == Some(true) {
            continue;
        }
        let (role, text) = match e["kind"].as_str() {
            Some("send-message") => {
                let m = &e["message"];
                let text = match m["type"].as_str() {
                    Some("text") => m["content"].as_str().map(String::from),
                    Some("widget") => m["widget"]["prompt"].as_str().map(|p| format!("[choice] {p}")),
                    _ => None,
                };
                ("grok", text)
            }
            Some("message") => {
                let role = if e["role"] == "user" { "user" } else { "grok" };
                (role, e["content"].as_str().map(String::from))
            }
            _ => continue,
        };
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
            out.push(GrokEntry { id: id.to_string(), role, text, at });
        }
    }
    Ok(out)
}

/// Import new entries of one transcript file into the thread. Returns how many were added.
pub fn ingest(nora: &Nora, chat_id: &str, path: &Path) -> Result<usize> {
    let body = std::fs::read(path)?;
    let entries = parse_transcript(&body)?;
    let store = nora.db();
    let since: i64 = match store.get_kv("grok_since")? {
        Some(v) => v.parse().unwrap_or(0),
        None => {
            let s = crate::store::now_ms() - nora.config().grok.backfill_hours * 3_600_000;
            store.set_kv("grok_since", &s.to_string())?;
            s
        }
    };
    let mut added = 0;
    for e in entries.into_iter().filter(|e| e.at >= since) {
        if e.role == "user" && crate::grok_write::is_own_echo(store, &e) {
            continue;
        }
        let mut m = NewMsg::new(if e.role == "user" { "user" } else { "grok" }, "grok", "message", e.text);
        m.at = Some(e.at);
        m.external_id = Some(format!("grok:{chat_id}:{}", e.id));
        if let Some(msg) = store.append(&m)? {
            added += 1;
            nora.bus().send(NoraEvent::Message { msg });
        }
    }
    if added > 0 {
        debug!(added, "grok entries imported");
    }
    Ok(added)
}

/// Watch the Grok Bot folder until shutdown. Also re-reads every 30 s in case an event is missed.
pub async fn watch(nora: Nora) {
    let cfg = nora.config().grok.clone();
    let Some(dir) = cfg.dir.clone().filter(|d| d.is_dir()) else {
        info!("Grok Bot folder not found; Grok read side off (set AOS_GROK_DIR)");
        return;
    };
    let Some(chat_id) = cfg.chat_id.clone().or_else(|| chat_id_from_roster(&dir, &cfg.chat_name)) else {
        warn!("Nora's Grok chat not found in the roster; set AOS_GROK_NORA_CHAT");
        return;
    };
    info!(%chat_id, "watching Grok Bot transcript");
    let suffix = format!("transcript.replicas.{chat_id}");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let suffix_w = suffix.clone();
    let debouncer = new_debouncer(Duration::from_millis(250), move |res: DebounceEventResult| {
        if let Ok(events) = res {
            let hit = events.iter().any(|e| {
                e.path.file_name().and_then(|f| decode_name(&f.to_string_lossy())).is_some_and(|n| n.ends_with(&suffix_w))
            });
            if hit {
                let _ = tx.send(());
            }
        }
    });
    let mut debouncer = match debouncer {
        Ok(d) => d,
        Err(e) => {
            warn!("grok watcher: {e}");
            return;
        }
    };
    if let Err(e) = debouncer.watcher().watch(&dir, RecursiveMode::NonRecursive) {
        warn!("grok watcher: {e}");
        return;
    }
    let shutdown = nora.shutdown_token();
    let run = |nora: &Nora| {
        let nora = nora.clone();
        let dir = dir.clone();
        let chat_id = chat_id.clone();
        async move {
            let _ = tokio::task::spawn_blocking(move || {
                if let Some(path) = transcript_path(&dir, &chat_id)
                    && let Err(e) = ingest(&nora, &chat_id, &path)
                {
                    debug!("grok ingest: {e}");
                }
            })
            .await;
        }
    };
    run(&nora).await;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            Some(()) = rx.recv() => run(&nora).await,
            _ = tokio::time::sleep(Duration::from_secs(30)) => run(&nora).await,
        }
    }
    drop(debouncer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_names() {
        let name = "sand.client.slice.account.x.transcript.replicas.abc-123";
        let file = format!("{}.blob", BASE32_NOPAD.encode(name.as_bytes()).to_ascii_lowercase());
        assert_eq!(decode_name(&file).as_deref(), Some(name));
        assert_eq!(decode_name("not base32!.blob"), None);
    }

    #[test]
    fn parses_transcript_kinds() {
        let body = br#"{"schemaVersion":1,"value":{"entries":[
            {"kind":"send-message","id":"t1","message":{"type":"text","content":"Hey"},"timestampMs":1000,"seq":1},
            {"kind":"message","id":"u1","role":"user","content":"hi nora","isStreaming":false,"timestampMs":2000,"seq":2},
            {"kind":"message","id":"u2","role":"assistant","content":"partial","isStreaming":true,"timestampMs":2500,"seq":3},
            {"kind":"send-message","id":"t2","message":{"type":"widget","widget":{"prompt":"Pick one"}},"timestampMs":3000,"seq":4},
            {"kind":"event","id":"e1","event":{},"timestampMs":4000,"seq":5}
        ]}}"#;
        let es = parse_transcript(body).unwrap();
        assert_eq!(es.len(), 3);
        assert_eq!(es[0], GrokEntry { id: "t1".into(), role: "grok", text: "Hey".into(), at: 1000 });
        assert_eq!(es[1].role, "user");
        assert_eq!(es[2].text, "[choice] Pick one");
        assert!(parse_transcript(b"{\"schemaVer").is_err());
    }

    #[test]
    fn finds_chat_in_roster() {
        let dir = std::env::temp_dir().join(format!("aos-grok-{}", aos_core::ids::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let enc = |n: &str| format!("{}.blob", BASE32_NOPAD.encode(n.as_bytes()).to_ascii_lowercase());
        std::fs::write(dir.join(enc("sand.client.slice.account.a.roster.last-roster")), r#"{"schemaVersion":1,"value":{"rows":[{"id":"c1","name":"Scout"},{"id":"c2","name":"Nora - assistant"}]}}"#).unwrap();
        std::fs::write(dir.join(enc("sand.client.slice.account.a.transcript.replicas.c2")), "{}").unwrap();
        assert_eq!(chat_id_from_roster(&dir, "Nora").as_deref(), Some("c2"));
        assert!(transcript_path(&dir, "c2").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
