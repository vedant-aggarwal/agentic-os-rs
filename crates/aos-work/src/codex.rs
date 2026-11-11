//! Codex through `codex app-server`: JSON-RPC over stdio, one JSON object per line, no
//! `"jsonrpc"` field. Handshake `initialize` -> `initialized`, then `thread/start` (or
//! `thread/resume`) and one `turn/start` per user message. The server asks for approval with
//! requests (`item/commandExecution/requestApproval`, `item/fileChange/requestApproval`) that we
//! answer with `{decision: accept | acceptForSession | decline}`, as T3 Code does.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::proc::{self, ChildProc};
use crate::session::{Ctx, SessionCmd, Update, head_chars};
use crate::store::ThreadRow;
use crate::supervisor::Inner;
use crate::types::*;

const OUTPUT_KEEP: usize = 4000;
const PLAN_INSTRUCTIONS: &str = "Plan mode: investigate and reply with a numbered plan. Do not edit files or run commands that change anything until the user says go.";

/// `approvalPolicy` and `sandbox` for a thread (T3: approval-required = untrusted + read-only,
/// full-access = never + danger-full-access).
pub fn policy(t: &ThreadSummary) -> (&'static str, &'static str) {
    match (t.interaction_mode, t.runtime_mode) {
        (InteractionMode::Plan, _) => ("untrusted", "read-only"),
        (_, RuntimeMode::Supervised) => ("untrusted", "read-only"),
        (_, RuntimeMode::AutoEdit) => ("on-request", "workspace-write"),
        (_, RuntimeMode::FullAccess) => ("never", "danger-full-access"),
    }
}

pub fn thread_params(t: &ThreadRow) -> (String, Value) {
    let s = &t.summary;
    let (approval, sandbox) = policy(s);
    let mut p = json!({ "cwd": t.folder, "approvalPolicy": approval, "sandbox": sandbox });
    if !s.model.trim().is_empty() {
        p["model"] = json!(s.model.trim());
    }
    if s.interaction_mode == InteractionMode::Plan {
        p["developerInstructions"] = json!(PLAN_INSTRUCTIONS);
    }
    match &t.provider_session_id {
        Some(id) => {
            p["threadId"] = json!(id);
            ("thread/resume".into(), p)
        }
        None => ("thread/start".into(), p),
    }
}

/// Notifications -> [`Update`]s. Pure, so it can be tested on recorded output.
#[derive(Default)]
pub struct Translator;

impl Translator {
    pub fn on_notification(&mut self, method: &str, p: &Value) -> Vec<Update> {
        let mut out = Vec::new();
        match method {
            "thread/started" => {
                if let Some(id) = p["thread"]["id"].as_str() {
                    out.push(Update::SessionId(id.into()));
                }
            }
            "turn/started" => {
                if let Some(id) = p["turn"]["id"].as_str() {
                    out.push(Update::ProviderTurnId(id.into()));
                }
            }
            "item/started" => {
                if let Some((key, kind, title, text, data)) = item_parts(&p["item"]) {
                    out.push(Update::ItemStart { key, kind, title, text, data });
                }
            }
            "item/agentMessage/delta" | "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" | "item/plan/delta" => {
                if let (Some(key), Some(delta)) = (p["itemId"].as_str(), p["delta"].as_str()) {
                    out.push(Update::Delta { key: key.into(), text: delta.into() });
                }
            }
            "item/completed" => {
                let item = &p["item"];
                if let Some((key, kind, title, text, data)) = item_parts(item) {
                    let status = match item["status"].as_str() {
                        Some("failed") => ItemStatus::Failed,
                        Some("declined") => ItemStatus::Declined,
                        _ => ItemStatus::Done,
                    };
                    let is_file_change = item["type"].as_str() == Some("fileChange");
                    out.push(Update::ItemDone { key, kind, status, title, text, data });
                    if is_file_change {
                        out.push(Update::DiffChanged);
                    }
                }
            }
            "turn/plan/updated" => {
                let steps = p["plan"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|s| PlanStep {
                                step: s["step"].as_str().unwrap_or("").into(),
                                status: match s["status"].as_str() {
                                    Some("inProgress") => "in_progress".into(),
                                    Some(other) => other.into(),
                                    None => "pending".into(),
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(Update::Plan(steps));
            }
            "turn/diff/updated" => out.push(Update::DiffChanged),
            "turn/completed" => {
                let turn = &p["turn"];
                let error = turn["error"]["message"].as_str().map(String::from);
                let status = match turn["status"].as_str() {
                    Some("interrupted") => TurnStatus::Interrupted,
                    Some("failed") => TurnStatus::Failed,
                    _ => TurnStatus::Completed,
                };
                out.push(Update::TurnDone { status, error });
            }
            "error" => {
                if !p["willRetry"].as_bool().unwrap_or(false) {
                    let msg = p["error"]["message"].as_str().unwrap_or("Codex reported an error").to_string();
                    out.push(Update::Error(msg));
                }
            }
            _ => {}
        }
        out
    }

    /// A server request asking for approval -> (kind, title, detail). `None` for requests we
    /// don't handle (answered with an error).
    pub fn approval(&self, method: &str, p: &Value) -> Option<(ApprovalKind, String, String)> {
        match method {
            "item/commandExecution/requestApproval" | "execCommandApproval" => {
                let cmd = match &p["command"] {
                    Value::String(s) => s.clone(),
                    Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
                    _ => String::new(),
                };
                let reason = p["reason"].as_str().unwrap_or("");
                let detail = [cmd.as_str(), reason].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n\n");
                Some((ApprovalKind::Command, format!("Run: {}", one_line(&cmd, 80)), detail))
            }
            "item/fileChange/requestApproval" | "applyPatchApproval" => {
                let reason = p["reason"].as_str().unwrap_or("Codex wants to change files.");
                Some((ApprovalKind::FileChange, "Change files".into(), reason.into()))
            }
            _ => None,
        }
    }
}

/// (key, kind, title, text, data) for a Codex `ThreadItem`. User messages are skipped: we
/// record those ourselves when the turn starts.
fn item_parts(item: &Value) -> Option<(String, ItemKind, String, String, Value)> {
    let key = item["id"].as_str()?.to_string();
    let s = |k: &str| item[k].as_str().unwrap_or("").to_string();
    Some(match item["type"].as_str()? {
        "agentMessage" => (key, ItemKind::AssistantMessage, String::new(), s("text"), Value::Null),
        "reasoning" => {
            let text = item["summary"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();
            (key, ItemKind::Reasoning, "Thinking".into(), text, Value::Null)
        }
        "plan" => (key, ItemKind::Plan, "Plan".into(), s("text"), Value::Null),
        "commandExecution" => {
            let out = head_chars(item["aggregatedOutput"].as_str().unwrap_or(""), OUTPUT_KEEP);
            (
                key,
                ItemKind::ToolCall,
                format!("Run {}", one_line(&s("command"), 80)),
                out,
                json!({ "tool": "command", "command": s("command"), "exitCode": item["exitCode"] }),
            )
        }
        "fileChange" => {
            let paths: Vec<String> = item["changes"]
                .as_array()
                .map(|a| a.iter().filter_map(|c| c["path"].as_str().map(String::from)).collect())
                .unwrap_or_default();
            let title = match paths.len() {
                0 => "Edit files".to_string(),
                1 => format!("Edit {}", paths[0].rsplit(['/', '\\']).next().unwrap_or(&paths[0])),
                n => format!("Edit {n} files"),
            };
            (key, ItemKind::ToolCall, title, paths.join("\n"), json!({ "tool": "fileChange", "changes": item["changes"] }))
        }
        "mcpToolCall" => (
            key,
            ItemKind::ToolCall,
            format!("{} {}", s("server"), s("tool")),
            head_chars(&item["result"].to_string(), OUTPUT_KEEP),
            json!({ "tool": "mcp", "arguments": item["arguments"] }),
        ),
        "webSearch" => (key, ItemKind::ToolCall, format!("Search {}", s("query")), String::new(), Value::Null),
        "dynamicToolCall" => (key, ItemKind::ToolCall, s("tool"), String::new(), json!({ "arguments": item["arguments"] })),
        _ => return None,
    })
}

enum Pending {
    Initialize,
    Thread,
    Turn,
    Other,
}

struct Rpc {
    child: ChildProc,
    next_id: i64,
    pending: HashMap<i64, Pending>,
}

impl Rpc {
    fn request(&mut self, method: &str, params: Value, kind: Pending) -> bool {
        self.next_id += 1;
        self.pending.insert(self.next_id, kind);
        self.child.send_json(&json!({ "id": self.next_id, "method": method, "params": params }))
    }
}

/// The session task for one Codex thread.
pub async fn run(inner: Arc<Inner>, thread: ThreadRow, bin: PathBuf, mut rx: mpsc::UnboundedReceiver<SessionCmd>) {
    let mut ctx = Ctx::new(inner.clone(), thread);
    let child = match proc::spawn(&bin, &["app-server".to_string()], std::path::Path::new(&ctx.thread.folder), &[]) {
        Ok(c) => c,
        Err(e) => {
            crate::claude::fail_queued(&mut ctx, &mut rx, &e.to_string());
            inner.session_ended(ctx.thread_id());
            return;
        }
    };
    info!(thread = ctx.thread_id(), pid = ?child.pid, "codex session started");
    let mut rpc = Rpc { child, next_id: 0, pending: HashMap::new() };
    rpc.request(
        "initialize",
        json!({
            "clientInfo": { "name": "agentic_os", "title": "Agentic OS", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "experimentalApi": true, "requestAttestation": false }
        }),
        Pending::Initialize,
    );

    let mut tr = Translator;
    let mut codex_thread: Option<String> = None;
    let mut model = ctx.thread.summary.model.clone();
    let mut provider_turn: Option<String> = None;
    // Open server requests: our provider_request_id -> JSON-RPC id.
    let mut requests: HashMap<String, Value> = HashMap::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut waiting_turn = false;
    let idle = inner.config.idle_timeout;

    loop {
        let busy = ctx.turn.is_some();
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(SessionCmd::Send { text }) => queue.push_back(text),
                Some(SessionCmd::Decide { provider_request_id, decision }) => {
                    if let Some(id) = requests.remove(&provider_request_id) {
                        let d = match decision {
                            Decision::Approve => "accept",
                            Decision::ApproveSession => "acceptForSession",
                            Decision::Deny => "decline",
                        };
                        rpc.child.send_json(&json!({ "id": id, "result": { "decision": d } }));
                    }
                    ctx.after_decision();
                }
                Some(SessionCmd::Interrupt) => {
                    queue.clear();
                    if let (Some(t), Some(turn)) = (&codex_thread, &provider_turn) {
                        rpc.request("turn/interrupt", json!({ "threadId": t, "turnId": turn }), Pending::Other);
                    } else if busy {
                        ctx.apply(Update::TurnDone { status: TurnStatus::Interrupted, error: None });
                    }
                }
                Some(SessionCmd::Stop) | None => break,
            },
            line = rpc.child.lines.recv() => {
                let Some(line) = line else {
                    let tail = rpc.child.stderr_tail(8);
                    warn!(thread = ctx.thread_id(), "codex exited: {tail}");
                    ctx.process_gone(&tail);
                    break;
                };
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                let method = msg["method"].as_str().map(String::from);
                let id = msg.get("id").cloned().filter(|v| !v.is_null());
                match (method, id) {
                    // A request from the server.
                    (Some(method), Some(id)) => {
                        if let Some((kind, title, detail)) = tr.approval(&method, &msg["params"]) {
                            let key = format!("{}:{}", ctx.thread_id(), id);
                            requests.insert(key.clone(), id);
                            ctx.apply(Update::Approval { request_id: key, kind, title, detail });
                        } else {
                            rpc.child.send_json(&json!({ "id": id, "error": { "code": -32601, "message": format!("{method} is not supported by Agentic OS") } }));
                        }
                    }
                    // A notification.
                    (Some(method), None) => {
                        for u in tr.on_notification(&method, &msg["params"]) {
                            match &u {
                                Update::ProviderTurnId(t) => provider_turn = Some(t.clone()),
                                Update::TurnDone { .. } => { provider_turn = None; requests.clear(); }
                                _ => {}
                            }
                            ctx.apply(u);
                        }
                    }
                    // A response to one of ours.
                    (None, Some(id)) => {
                        let kind = id.as_i64().and_then(|n| rpc.pending.remove(&n));
                        let err = msg["error"]["message"].as_str().map(String::from);
                        match kind {
                            Some(Pending::Initialize) => {
                                rpc.child.send_json(&json!({ "method": "initialized" }));
                                let (method, params) = thread_params(&ctx.thread);
                                rpc.request(&method, params, Pending::Thread);
                            }
                            Some(Pending::Thread) => {
                                if let Some(e) = err {
                                    ctx.note(ItemKind::Error, &format!("Codex could not open the thread: {e}"));
                                    ctx.inner.set_status(ctx.thread_id(), ThreadStatus::Error, Some(&e));
                                    break;
                                }
                                let r = &msg["result"];
                                if let Some(tid) = r["thread"]["id"].as_str() {
                                    codex_thread = Some(tid.to_string());
                                    ctx.apply(Update::SessionId(tid.to_string()));
                                }
                                if let Some(m) = r["model"].as_str() { model = m.to_string(); }
                            }
                            Some(Pending::Turn) => {
                                waiting_turn = false;
                                if let Some(e) = err {
                                    ctx.apply(Update::TurnDone { status: TurnStatus::Failed, error: Some(e) });
                                } else if let Some(t) = msg["result"]["turn"]["id"].as_str() {
                                    provider_turn = Some(t.to_string());
                                    ctx.apply(Update::ProviderTurnId(t.to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(idle), if !busy && queue.is_empty() => {
                info!(thread = ctx.thread_id(), "codex session idle, stopping");
                break;
            }
        }

        // Start the next queued turn once the thread is open and nothing is running.
        if ctx.turn.is_none() && !waiting_turn {
            if let (Some(tid), Some(text)) = (codex_thread.clone(), queue.front().cloned()) {
                queue.pop_front();
                if ctx.begin_turn(&text) {
                    let mut params = json!({
                        "threadId": tid,
                        "input": [{ "type": "text", "text": text, "text_elements": [] }]
                    });
                    if ctx.thread.summary.interaction_mode == InteractionMode::Plan && !model.is_empty() {
                        params["collaborationMode"] = json!({
                            "mode": "plan",
                            "settings": { "model": model, "reasoning_effort": null, "developer_instructions": null }
                        });
                    }
                    waiting_turn = rpc.request("turn/start", params, Pending::Turn);
                }
            }
        }
    }
    rpc.child.shutdown(Duration::from_secs(3)).await;
    if ctx.turn.is_some() {
        ctx.apply(Update::TurnDone { status: TurnStatus::Interrupted, error: None });
    }
    inner.session_ended(ctx.thread_id());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_a_recorded_pong_turn() {
        let mut tr = Translator;
        let mut all = Vec::new();
        for line in include_str!("../tests/fixtures/codex_pong_turn.jsonl").lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line).unwrap();
            if let Some(m) = v["method"].as_str() {
                all.extend(tr.on_notification(m, &v["params"]));
            }
        }
        assert!(all.iter().any(|u| matches!(u, Update::SessionId(_))));
        let text: String = all
            .iter()
            .filter_map(|u| match u {
                Update::Delta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "PONG");
        assert!(all.iter().any(|u| matches!(u, Update::ItemDone { kind: ItemKind::AssistantMessage, text, .. } if text == "PONG")));
        assert!(matches!(all.last(), Some(Update::TurnDone { status: TurnStatus::Completed, .. })));
    }

    #[test]
    fn approval_requests() {
        let tr = Translator;
        let (kind, title, _) = tr
            .approval("item/commandExecution/requestApproval", &json!({"command": "cargo test", "threadId": "t"}))
            .unwrap();
        assert_eq!(kind, ApprovalKind::Command);
        assert_eq!(title, "Run: cargo test");
        assert!(tr.approval("item/tool/requestUserInput", &json!({})).is_none());
    }

    #[test]
    fn policy_by_mode() {
        let row = ThreadRow {
            summary: ThreadSummary {
                id: "t".into(),
                project_id: "p".into(),
                title: "x".into(),
                provider: Provider::Codex,
                model: String::new(),
                runtime_mode: RuntimeMode::FullAccess,
                interaction_mode: InteractionMode::Default,
                status: ThreadStatus::Idle,
                pending_approvals: 0,
                preview: String::new(),
                last_error: None,
                origin: None,
                created_at: 0,
                updated_at: 0,
            },
            provider_session_id: Some("abc".into()),
            folder: "/x".into(),
        };
        let (method, p) = thread_params(&row);
        assert_eq!(method, "thread/resume");
        assert_eq!(p["approvalPolicy"], "never");
        assert_eq!(p["sandbox"], "danger-full-access");
        assert_eq!(p["threadId"], "abc");
    }
}
