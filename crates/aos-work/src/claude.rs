//! Claude through the `claude` CLI in stream-json mode, on the owner's own login (no API key).
//!
//! `claude -p --input-format stream-json --output-format stream-json --verbose
//!  --include-partial-messages --permission-prompt-tool stdio --permission-mode <mode>`
//!
//! One process per thread stays open between turns; each user message is one JSON line on stdin.
//! With `--permission-prompt-tool stdio` the CLI asks for tool permission with a
//! `control_request {subtype: "can_use_tool"}` on stdout and waits for our `control_response`:
//! the same mechanism the Agent SDK's `canUseTool` (and so T3 Code) uses. Interrupt is a
//! `control_request {subtype: "interrupt"}`. Resume after a restart with `--resume <session_id>`.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::proc;
use crate::session::{Ctx, SessionCmd, Update, head_chars};
use crate::store::{ThreadRow, new_id};
use crate::supervisor::Inner;
use crate::types::*;

const OUTPUT_KEEP: usize = 4000;

/// CLI arguments for a thread.
pub fn args(t: &ThreadRow) -> Vec<String> {
    let s = &t.summary;
    let mut a: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mode = match (s.interaction_mode, s.runtime_mode) {
        (InteractionMode::Plan, _) => "plan",
        (_, RuntimeMode::Supervised) => "default",
        (_, RuntimeMode::AutoEdit) => "acceptEdits",
        (_, RuntimeMode::FullAccess) => "bypassPermissions",
    };
    a.push("--permission-mode".into());
    a.push(mode.into());
    if s.runtime_mode == RuntimeMode::FullAccess {
        a.push("--allow-dangerously-skip-permissions".into());
    }
    if !s.model.trim().is_empty() {
        a.push("--model".into());
        a.push(s.model.trim().into());
    }
    if let Some(sid) = &t.provider_session_id {
        a.push("--resume".into());
        a.push(sid.clone());
    }
    a
}

pub fn user_message(text: &str) -> Value {
    json!({
        "type": "user",
        "message": { "role": "user", "content": [{ "type": "text", "text": text }] },
        "parent_tool_use_id": null,
        "session_id": ""
    })
}

/// Turns stream-json lines into [`Update`]s. Pure, so it can be tested on recorded output.
#[derive(Default)]
pub struct Translator {
    /// Message id of the message streaming now (from `message_start`).
    msg_id: String,
    /// Content blocks of that message that are text/thinking, by index.
    open_blocks: HashMap<u64, (String, ItemKind)>,
    /// How many content blocks of each message the full `assistant` events have shown.
    seen_blocks: HashMap<String, u64>,
    /// Tool-use id -> (title, input) so results can be matched.
    tools: HashMap<String, (String, Value)>,
    /// Inputs of open permission requests, echoed back on allow.
    pub pending_inputs: HashMap<String, Value>,
    /// Set when we sent an interrupt, so the result reads as interrupted, not failed.
    pub interrupted: bool,
}

impl Translator {
    pub fn on_line(&mut self, v: &Value) -> Vec<Update> {
        let mut out = Vec::new();
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "system" => {
                if v.get("subtype").and_then(Value::as_str) == Some("init") {
                    if let Some(sid) = v.get("session_id").and_then(Value::as_str) {
                        out.push(Update::SessionId(sid.to_string()));
                    }
                }
            }
            "stream_event" => self.stream_event(&v["event"], &mut out),
            "assistant" => self.assistant(v, &mut out),
            "user" => self.tool_results(v, &mut out),
            "control_request" => self.control_request(v, &mut out),
            "result" => {
                let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("success");
                if let Some(sid) = v.get("session_id").and_then(Value::as_str) {
                    out.push(Update::SessionId(sid.to_string()));
                }
                let (status, error) = if self.interrupted {
                    (TurnStatus::Interrupted, None)
                } else if is_error || subtype.starts_with("error") {
                    let msg = v
                        .get("result")
                        .and_then(Value::as_str)
                        .map(String::from)
                        .or_else(|| v.get("errors").map(|e| e.to_string()))
                        .unwrap_or_else(|| subtype.to_string());
                    (TurnStatus::Failed, Some(msg))
                } else {
                    (TurnStatus::Completed, None)
                };
                self.interrupted = false;
                out.push(Update::TurnDone { status, error });
            }
            _ => {}
        }
        out
    }

    fn stream_event(&mut self, e: &Value, out: &mut Vec<Update>) {
        match e.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                self.msg_id = e["message"]["id"].as_str().unwrap_or_default().to_string();
                self.open_blocks.clear();
            }
            "content_block_start" => {
                let idx = e["index"].as_u64().unwrap_or(0);
                let kind = match e["content_block"]["type"].as_str() {
                    Some("text") => ItemKind::AssistantMessage,
                    Some("thinking") => ItemKind::Reasoning,
                    _ => return,
                };
                let key = format!("{}:{idx}", self.msg_id);
                self.open_blocks.insert(idx, (key.clone(), kind));
                // Reasoning items are only created once there is visible text (this CLI build
                // often streams empty thinking).
                if kind == ItemKind::AssistantMessage {
                    out.push(Update::ItemStart { key, kind, title: String::new(), text: String::new(), data: Value::Null });
                }
            }
            "content_block_delta" => {
                let idx = e["index"].as_u64().unwrap_or(0);
                let Some((key, kind)) = self.open_blocks.get(&idx).cloned() else { return };
                let d = &e["delta"];
                let text = match d["type"].as_str() {
                    Some("text_delta") => d["text"].as_str(),
                    Some("thinking_delta") => d["thinking"].as_str(),
                    _ => None,
                };
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    if kind == ItemKind::Reasoning && !self.seen_blocks.contains_key(&format!("r:{key}")) {
                        self.seen_blocks.insert(format!("r:{key}"), 1);
                        out.push(Update::ItemStart {
                            key: key.clone(),
                            kind,
                            title: "Thinking".into(),
                            text: String::new(),
                            data: Value::Null,
                        });
                    }
                    out.push(Update::Delta { key, text: text.to_string() });
                }
            }
            _ => {}
        }
    }

    fn assistant(&mut self, v: &Value, out: &mut Vec<Update>) {
        let msg = &v["message"];
        let msg_id = msg["id"].as_str().unwrap_or_default().to_string();
        let Some(blocks) = msg["content"].as_array() else { return };
        for block in blocks {
            let idx = {
                let n = self.seen_blocks.entry(msg_id.clone()).or_insert(0);
                let i = *n;
                *n += 1;
                i
            };
            let key = format!("{msg_id}:{idx}");
            match block["type"].as_str() {
                Some("text") => {
                    let text = block["text"].as_str().unwrap_or_default().to_string();
                    out.push(Update::ItemDone {
                        key,
                        kind: ItemKind::AssistantMessage,
                        status: ItemStatus::Done,
                        title: String::new(),
                        text,
                        data: Value::Null,
                    });
                }
                Some("thinking") => {
                    let text = block["thinking"].as_str().unwrap_or_default();
                    if !text.trim().is_empty() {
                        out.push(Update::ItemDone {
                            key,
                            kind: ItemKind::Reasoning,
                            status: ItemStatus::Done,
                            title: "Thinking".into(),
                            text: text.to_string(),
                            data: Value::Null,
                        });
                    }
                }
                Some("tool_use") => {
                    let id = block["id"].as_str().unwrap_or(&key).to_string();
                    let name = block["name"].as_str().unwrap_or("tool");
                    let input = block["input"].clone();
                    let title = tool_title(name, &input);
                    if name == "TodoWrite" {
                        if let Some(steps) = todo_steps(&input) {
                            out.push(Update::Plan(steps));
                        }
                    }
                    self.tools.insert(id.clone(), (title.clone(), input.clone()));
                    out.push(Update::ItemStart {
                        key: id,
                        kind: ItemKind::ToolCall,
                        title,
                        text: String::new(),
                        data: json!({ "tool": name, "input": input }),
                    });
                }
                _ => {}
            }
        }
    }

    fn tool_results(&mut self, v: &Value, out: &mut Vec<Update>) {
        let Some(blocks) = v["message"]["content"].as_array() else { return };
        for block in blocks {
            if block["type"].as_str() != Some("tool_result") {
                continue;
            }
            let Some(id) = block["tool_use_id"].as_str() else { continue };
            let is_error = block["is_error"].as_bool().unwrap_or(false);
            let text = match &block["content"] {
                Value::String(s) => s.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            let (title, _) = self.tools.get(id).cloned().unwrap_or_default();
            let declined = is_error && text.contains("denied");
            out.push(Update::ItemDone {
                key: id.to_string(),
                kind: ItemKind::ToolCall,
                status: if declined {
                    ItemStatus::Declined
                } else if is_error {
                    ItemStatus::Failed
                } else {
                    ItemStatus::Done
                },
                title,
                text: head_chars(&text, OUTPUT_KEEP),
                data: Value::Null,
            });
        }
    }

    fn control_request(&mut self, v: &Value, out: &mut Vec<Update>) {
        let req = &v["request"];
        if req["subtype"].as_str() != Some("can_use_tool") {
            return;
        }
        let Some(request_id) = v["request_id"].as_str() else { return };
        let tool = req["tool_name"].as_str().unwrap_or("tool");
        let input = req["input"].clone();
        self.pending_inputs.insert(request_id.to_string(), input.clone());
        let (kind, title, detail) = match tool {
            "Bash" | "PowerShell" => (
                ApprovalKind::Command,
                format!("Run: {}", one_line(input["command"].as_str().unwrap_or(""), 80)),
                [input["command"].as_str().unwrap_or(""), input["description"].as_str().unwrap_or("")]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            ),
            "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
                let path = input["file_path"].as_str().or(input["notebook_path"].as_str()).unwrap_or("");
                let body = input["content"]
                    .as_str()
                    .or(input["new_string"].as_str())
                    .or(input["new_source"].as_str())
                    .unwrap_or("");
                (ApprovalKind::FileChange, format!("{tool} {}", short_path(path)), head_chars(body, 2000))
            }
            "ExitPlanMode" => (
                ApprovalKind::Plan,
                "Start building from this plan".to_string(),
                input["plan"].as_str().unwrap_or("").to_string(),
            ),
            _ => (
                ApprovalKind::Tool,
                format!("Use {tool}"),
                head_chars(&serde_json::to_string_pretty(&input).unwrap_or_default(), 2000),
            ),
        };
        if kind == ApprovalKind::Plan {
            out.push(Update::ItemDone {
                key: format!("plan:{request_id}"),
                kind: ItemKind::Plan,
                status: ItemStatus::Done,
                title: "Plan".into(),
                text: detail.clone(),
                data: Value::Null,
            });
        }
        out.push(Update::Approval { request_id: request_id.to_string(), kind, title, detail });
    }

    /// The `control_response` that answers a permission request.
    pub fn decision_response(&mut self, request_id: &str, decision: Decision) -> Value {
        let input = self.pending_inputs.remove(request_id).unwrap_or(Value::Object(Default::default()));
        let response = match decision {
            Decision::Approve => json!({ "behavior": "allow", "updatedInput": input }),
            Decision::ApproveSession => json!({
                "behavior": "allow",
                "updatedInput": input,
                // Same rule T3 adds for "accept for session".
                "updatedPermissions": [{ "type": "setMode", "mode": "acceptEdits", "destination": "session" }]
            }),
            Decision::Deny => json!({ "behavior": "deny", "message": "The owner denied this. Ask or try another way." }),
        };
        json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": request_id, "response": response }
        })
    }
}

fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 { p.to_string() } else { parts[parts.len() - 2..].join("/") }
}

/// One-line title for a tool call (`Bash: cargo test`, `Edit src/lib.rs`).
pub fn tool_title(name: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    let detail = match name {
        "Bash" | "PowerShell" => one_line(s("command"), 80),
        "Read" | "Write" | "Edit" | "MultiEdit" => short_path(s("file_path")),
        "NotebookEdit" => short_path(s("notebook_path")),
        "Grep" | "Glob" => one_line(s("pattern"), 60),
        "WebFetch" => one_line(s("url"), 60),
        "WebSearch" => one_line(s("query"), 60),
        "Task" | "Agent" => one_line(s("description"), 60),
        "TodoWrite" => "update plan".into(),
        _ => String::new(),
    };
    if detail.is_empty() { name.to_string() } else { format!("{name} {detail}") }
}

fn todo_steps(input: &Value) -> Option<Vec<PlanStep>> {
    let todos = input.get("todos")?.as_array()?;
    Some(
        todos
            .iter()
            .map(|t| PlanStep {
                step: t["content"].as_str().unwrap_or("").to_string(),
                status: t["status"].as_str().unwrap_or("pending").to_string(),
            })
            .collect(),
    )
}

/// The session task for one Claude thread. Ends on `Stop`, idle timeout or process exit.
pub async fn run(inner: Arc<Inner>, thread: ThreadRow, bin: PathBuf, mut rx: mpsc::UnboundedReceiver<SessionCmd>) {
    let mut ctx = Ctx::new(inner.clone(), thread);
    let args = args(&ctx.thread);
    let resumed = ctx.thread.provider_session_id.is_some();
    // Env vars a parent Claude Code session sets that would make the child think it is nested.
    let env_remove = ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"];
    let mut child = match proc::spawn(&bin, &args, std::path::Path::new(&ctx.thread.folder), &env_remove) {
        Ok(c) => c,
        Err(e) => {
            fail_queued(&mut ctx, &mut rx, &e.to_string());
            inner.session_ended(ctx.thread_id());
            return;
        }
    };
    info!(thread = ctx.thread_id(), pid = ?child.pid, resumed, "claude session started");
    child.send_json(&json!({ "type": "control_request", "request_id": new_id(), "request": { "subtype": "initialize" } }));

    let mut tr = Translator::default();
    let mut queue: VecDeque<String> = VecDeque::new();
    let idle = inner.config.idle_timeout;

    loop {
        let busy = ctx.turn.is_some();
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(SessionCmd::Send { text }) => {
                    if busy { queue.push_back(text) } else { start_turn(&mut ctx, &child, &text) }
                }
                Some(SessionCmd::Decide { provider_request_id, decision }) => {
                    child.send_json(&tr.decision_response(&provider_request_id, decision));
                    ctx.after_decision();
                }
                Some(SessionCmd::Interrupt) => {
                    if busy {
                        tr.interrupted = true;
                        child.send_json(&json!({ "type": "control_request", "request_id": new_id(), "request": { "subtype": "interrupt" } }));
                    }
                    queue.clear();
                }
                Some(SessionCmd::Stop) | None => break,
            },
            line = child.lines.recv() => match line {
                Some(line) => {
                    let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                    for u in tr.on_line(&v) {
                        let done = matches!(u, Update::TurnDone { .. });
                        ctx.apply(u);
                        if done {
                            if let Some(next) = queue.pop_front() { start_turn(&mut ctx, &child, &next) }
                        }
                    }
                }
                None => {
                    let tail = child.stderr_tail(8);
                    warn!(thread = ctx.thread_id(), "claude exited: {tail}");
                    ctx.process_gone(&tail);
                    break;
                }
            },
            _ = tokio::time::sleep(idle), if !busy => {
                info!(thread = ctx.thread_id(), "claude session idle, stopping");
                break;
            }
        }
    }
    child.shutdown(Duration::from_secs(3)).await;
    if ctx.turn.is_some() {
        ctx.apply(Update::TurnDone { status: TurnStatus::Interrupted, error: None });
    }
    inner.session_ended(ctx.thread_id());
}

fn start_turn(ctx: &mut Ctx, child: &proc::ChildProc, text: &str) {
    if ctx.begin_turn(text) && !child.send_json(&user_message(text)) {
        ctx.apply(Update::TurnDone { status: TurnStatus::Failed, error: Some("The CLI is not accepting input.".into()) });
    }
}

/// The CLI could not start: show the error and fail whatever was sent.
pub(crate) fn fail_queued(ctx: &mut Ctx, rx: &mut mpsc::UnboundedReceiver<SessionCmd>, err: &str) {
    let mut any = false;
    while let Ok(cmd) = rx.try_recv() {
        if let SessionCmd::Send { text } = cmd {
            ctx.begin_turn(&text);
            ctx.apply(Update::TurnDone { status: TurnStatus::Failed, error: Some(err.to_string()) });
            any = true;
        }
    }
    if !any {
        ctx.note(ItemKind::Error, err);
        ctx.inner.set_status(ctx.thread_id(), ThreadStatus::Error, Some(err));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<Value> {
        text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn translates_a_recorded_turn_with_a_tool_approval() {
        let mut tr = Translator::default();
        let mut all = Vec::new();
        for v in lines(include_str!("../tests/fixtures/claude_write_turn.jsonl")) {
            all.extend(tr.on_line(&v));
        }
        assert!(matches!(&all[0], Update::SessionId(s) if s == "sess-1"));
        let approval = all.iter().find_map(|u| match u {
            Update::Approval { request_id, kind, title, .. } => Some((request_id.clone(), *kind, title.clone())),
            _ => None,
        });
        let (req, kind, title) = approval.expect("approval");
        assert_eq!(req, "req-1");
        assert_eq!(kind, ApprovalKind::FileChange);
        assert_eq!(title, "Write scratch/pong.txt");
        let deltas: String = all
            .iter()
            .filter_map(|u| match u {
                Update::Delta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, "DONE");
        assert!(all.iter().any(|u| matches!(u, Update::ItemDone { kind: ItemKind::ToolCall, status: ItemStatus::Done, .. })));
        assert!(matches!(all.last(), Some(Update::TurnDone { status: TurnStatus::Completed, error: None })));

        let resp = tr.decision_response("req-1", Decision::Approve);
        assert_eq!(resp["response"]["response"]["behavior"], "allow");
        assert_eq!(resp["response"]["response"]["updatedInput"]["content"], "PONG\n");
        assert_eq!(resp["response"]["request_id"], "req-1");
    }

    #[test]
    fn stream_text_item_key_matches_full_message() {
        let mut tr = Translator::default();
        let v = lines(include_str!("../tests/fixtures/claude_write_turn.jsonl"));
        let mut start_key = None;
        let mut done_key = None;
        for line in v {
            for u in tr.on_line(&line) {
                match u {
                    Update::ItemStart { key, kind: ItemKind::AssistantMessage, .. } => start_key = Some(key),
                    Update::ItemDone { key, kind: ItemKind::AssistantMessage, .. } => done_key = Some(key),
                    _ => {}
                }
            }
        }
        assert!(start_key.is_some());
        assert_eq!(start_key, done_key);
    }

    #[test]
    fn interrupted_result() {
        let mut tr = Translator::default();
        tr.interrupted = true;
        let out = tr.on_line(&json!({"type":"result","subtype":"error_during_execution","is_error":true}));
        assert!(matches!(out.last(), Some(Update::TurnDone { status: TurnStatus::Interrupted, .. })));
    }

    #[test]
    fn args_by_mode() {
        use crate::store::ThreadRow;
        let mut t = ThreadRow {
            summary: ThreadSummary {
                id: "t".into(),
                project_id: "p".into(),
                title: "x".into(),
                provider: Provider::Claude,
                model: "haiku".into(),
                runtime_mode: RuntimeMode::AutoEdit,
                interaction_mode: InteractionMode::Default,
                status: ThreadStatus::Idle,
                pending_approvals: 0,
                preview: String::new(),
                last_error: None,
                origin: None,
                created_at: 0,
                updated_at: 0,
            },
            provider_session_id: Some("s1".into()),
            folder: ".".into(),
        };
        let a = args(&t).join(" ");
        assert!(a.contains("--permission-mode acceptEdits"));
        assert!(a.contains("--model haiku"));
        assert!(a.contains("--resume s1"));
        t.summary.interaction_mode = InteractionMode::Plan;
        assert!(args(&t).join(" ").contains("--permission-mode plan"));
    }
}
