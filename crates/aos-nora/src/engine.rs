//! The model behind a turn. [`ClaudeCli`] runs one fresh `claude -p` process per turn (stream-json
//! in and out) through `aos-work`'s process supervisor and translator, so no hidden reasoning or
//! session carries over between turns: the thread with timestamps is the only memory.
//!
//! [`Engine`] is a trait so tests (and later other models) can stand in.

use std::path::PathBuf;
use std::time::Duration;

use aos_work::claude::{Translator, user_message};
use aos_work::session::Update;
use aos_work::types::ItemKind;
use futures::future::BoxFuture;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// One model run.
#[derive(Debug, Clone, Default)]
pub struct TurnRequest {
    pub system: String,
    pub prompt: String,
    /// `None` = the CLI's default model.
    pub model: Option<String>,
    /// MCP config JSON (`{"mcpServers": {...}}`); `None` runs without tools.
    pub mcp_config: Option<String>,
    /// MCP tool names the model must not call (e.g. `nora_send`, which would loop).
    pub deny_tools: Vec<String>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TurnUpdate {
    /// More text of the block being written.
    Delta(String),
    /// A finished text block.
    Text(String),
    /// A tool call started (`done = false`) or finished.
    Tool { name: String, title: String, done: bool },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnOutcome {
    pub ok: bool,
    pub error: Option<String>,
    /// All finished text blocks, in order.
    pub texts: Vec<String>,
    pub tools: Vec<String>,
    pub cost_usd: Option<f64>,
}

impl TurnOutcome {
    pub fn final_text(&self) -> Option<&str> {
        self.texts.iter().rev().map(|s| s.trim()).find(|s| !s.is_empty())
    }
}

pub trait Engine: Send + Sync {
    fn run(&self, req: TurnRequest, updates: mpsc::UnboundedSender<TurnUpdate>) -> BoxFuture<'static, TurnOutcome>;
}

/// Run a request and return only the final text (heartbeat, consolidation).
pub async fn ask(engine: &dyn Engine, req: TurnRequest) -> Result<String, String> {
    let (tx, _rx) = mpsc::unbounded_channel();
    let out = engine.run(req, tx).await;
    match (out.ok, out.final_text()) {
        (true, Some(t)) => Ok(t.to_string()),
        (true, None) => Err("empty reply".into()),
        (false, _) => Err(out.error.unwrap_or_else(|| "model run failed".into())),
    }
}

/// The `claude` CLI on his own login.
pub struct ClaudeCli {
    pub bin: Option<PathBuf>,
    /// Working folder for the process (empty, so no project CLAUDE.md is picked up).
    pub cwd: PathBuf,
}

impl ClaudeCli {
    pub fn args(req: &TurnRequest) -> Vec<String> {
        let mut a: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--no-session-persistence",
            // No user/project settings, hooks, plugins or CLAUDE.md: Nora's prompt is the whole brief.
            "--setting-sources",
            "",
            "--strict-mcp-config",
            // No built-in tools (Bash, files, web); her tools come from our MCP server.
            "--tools",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        a.push("--system-prompt".into());
        a.push(req.system.clone());
        if let Some(cfg) = &req.mcp_config {
            a.push("--mcp-config".into());
            a.push(cfg.clone());
            a.push("--allowedTools".into());
            a.push("mcp__aos".into());
            if !req.deny_tools.is_empty() {
                a.push("--disallowedTools".into());
                a.push(req.deny_tools.iter().map(|t| format!("mcp__aos__{t}")).collect::<Vec<_>>().join(","));
            }
        }
        if let Some(m) = req.model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
            a.push("--model".into());
            a.push(m.into());
        }
        a
    }
}

impl Engine for ClaudeCli {
    fn run(&self, req: TurnRequest, updates: mpsc::UnboundedSender<TurnUpdate>) -> BoxFuture<'static, TurnOutcome> {
        let bin = self.bin.clone();
        let cwd = self.cwd.clone();
        Box::pin(async move {
            let Some(bin) = bin else {
                return TurnOutcome { error: Some("claude CLI not found (set AOS_CLAUDE_BIN)".into()), ..Default::default() };
            };
            let _ = std::fs::create_dir_all(&cwd);
            let env_remove = ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"];
            let mut child = match aos_work::proc::spawn(&bin, &ClaudeCli::args(&req), &cwd, &env_remove) {
                Ok(c) => c,
                Err(e) => return TurnOutcome { error: Some(e.to_string()), ..Default::default() },
            };
            info!(pid = ?child.pid, model = ?req.model, "nora claude turn started");
            child.send_json(&user_message(&req.prompt));
            let timeout = req.timeout.unwrap_or(Duration::from_secs(600));
            let out = tokio::time::timeout(timeout, read_turn(&mut child.lines, &updates)).await;
            let out = match out {
                Ok(Some(o)) => o,
                Ok(None) => {
                    let tail = child.stderr_tail(6);
                    warn!("nora claude exited early: {tail}");
                    TurnOutcome { error: Some(format!("claude exited: {}", tail.trim())), ..Default::default() }
                }
                Err(_) => TurnOutcome { error: Some("turn timed out".into()), ..Default::default() },
            };
            child.shutdown(Duration::from_secs(3)).await;
            out
        })
    }
}

/// Read stream-json lines until the `result`. `None` if the process ended first.
async fn read_turn(lines: &mut mpsc::UnboundedReceiver<String>, updates: &mpsc::UnboundedSender<TurnUpdate>) -> Option<TurnOutcome> {
    let mut tr = Translator::default();
    let mut out = TurnOutcome::default();
    while let Some(line) = lines.recv().await {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["type"] == "result" {
            out.cost_usd = v["total_cost_usd"].as_f64();
        }
        if let Some(done) = apply(&mut tr, &v, &mut out, updates) {
            out.ok = done.0;
            out.error = done.1;
            return Some(out);
        }
    }
    None
}

/// Feed one line; returns `Some((ok, error))` when the turn ended.
pub(crate) fn apply(
    tr: &mut Translator,
    v: &Value,
    out: &mut TurnOutcome,
    updates: &mpsc::UnboundedSender<TurnUpdate>,
) -> Option<(bool, Option<String>)> {
    for u in tr.on_line(v) {
        match u {
            Update::Delta { text, .. } => {
                let _ = updates.send(TurnUpdate::Delta(text));
            }
            Update::ItemDone { kind: ItemKind::AssistantMessage, text, .. } if !text.trim().is_empty() => {
                out.texts.push(text.clone());
                let _ = updates.send(TurnUpdate::Text(text));
            }
            Update::ItemStart { kind: ItemKind::ToolCall, title, data, .. } => {
                let name = data["tool"].as_str().unwrap_or("tool").trim_start_matches("mcp__aos__").to_string();
                out.tools.push(name.clone());
                let _ = updates.send(TurnUpdate::Tool { name, title, done: false });
            }
            Update::ItemDone { kind: ItemKind::ToolCall, title, .. } => {
                let _ = updates.send(TurnUpdate::Tool { name: String::new(), title, done: true });
            }
            Update::TurnDone { status, error } => {
                return Some((status == aos_work::types::TurnStatus::Completed, error));
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn args_attach_mcp_and_deny_list() {
        let req = TurnRequest {
            system: "sys".into(),
            mcp_config: Some("{}".into()),
            deny_tools: vec!["nora_send".into()],
            model: Some("haiku".into()),
            ..Default::default()
        };
        let a = ClaudeCli::args(&req);
        let joined = a.join(" ");
        assert!(joined.contains("--mcp-config {}"));
        assert!(joined.contains("--disallowedTools mcp__aos__nora_send"));
        assert!(joined.contains("--model haiku"));
        assert!(a.windows(2).any(|w| w[0] == "--tools" && w[1].is_empty()));
    }

    #[test]
    fn translates_text_tool_and_result() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut tr = Translator::default();
        let mut out = TurnOutcome::default();
        let lines = [
            json!({"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"On it."}]}}),
            json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"mcp__aos__alarm_set","input":{"at":"+5m"}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}),
            json!({"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":"Alarm set for 21:50."}]}}),
        ];
        for l in &lines {
            assert!(apply(&mut tr, l, &mut out, &tx).is_none());
        }
        let end = apply(&mut tr, &json!({"type":"result","subtype":"success","is_error":false}), &mut out, &tx);
        assert_eq!(end, Some((true, None)));
        assert_eq!(out.texts, vec!["On it.", "Alarm set for 21:50."]);
        assert_eq!(out.tools, vec!["alarm_set"]);
        assert_eq!(out.final_text(), Some("Alarm set for 21:50."));
        let mut n = 0;
        while rx.try_recv().is_ok() {
            n += 1;
        }
        assert!(n >= 4);
    }
}
