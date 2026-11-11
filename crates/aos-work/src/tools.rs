//! Work tools for the foundation registry (ARCHITECTURE.md §2): the app, Nora, Grok Bot (MCP)
//! and the `aos` CLI call the same four tools.
//!
//! `work_launch`, `work_status`, `work_send`, `work_approve` (`work.launch` etc. resolve too).
//! Handlers are sync, as the registry wants: every [`Work`] method only writes SQLite and hands
//! the slow part to a session task, so they return at once with thread ids.

use aos_tools::{Risk, Tool, ToolCtx, ToolError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::WorkError;
use crate::supervisor::Work;
use crate::types::*;

/// Foundation's registry plus the Work tools.
pub fn registry(work: &Work) -> aos_tools::Registry {
    let mut r = aos_tools::registry();
    r.extend(tools(work));
    r
}

/// The Work tools, bound to a running supervisor. Add them with `Registry::extend`.
pub fn tools(work: &Work) -> Vec<Tool> {
    let (w1, w2, w3, w4) = (work.clone(), work.clone(), work.clone(), work.clone());
    vec![
        Tool::new(
            "work_launch",
            "Start one or more coding sessions (Claude or Codex) in a project folder, one per task. Returns the new thread ids at once; the sessions keep running. Use work_status to follow them.",
            Risk::Tell,
            move |_ctx: &ToolCtx<'_>, a: LaunchArgs| launch(&w1, a),
        )
        .positional(&["project", "tasks"]),
        Tool::new(
            "work_status",
            "Coding sessions now: which are working, which need the owner (approvals waiting), recent ones. Give `thread` for one session's latest messages and open approvals.",
            Risk::Free,
            move |_ctx: &ToolCtx<'_>, a: StatusArgs| status(&w2, a),
        )
        .read_only()
        .positional(&["thread"]),
        Tool::new(
            "work_send",
            "Send a message to a coding session (resumes it if it was stopped; queues if it is busy).",
            Risk::Tell,
            move |_ctx: &ToolCtx<'_>, a: SendArgs| send(&w3, a),
        )
        .positional(&["thread", "text"]),
        Tool::new(
            "work_approve",
            "Answer a session's approval request: approve (default), approve_session (allow this kind for the rest of the session) or deny. Give the approval id, or a thread id to answer its oldest open request.",
            Risk::Tell,
            move |_ctx: &ToolCtx<'_>, a: ApproveArgs| approve(&w4, a),
        )
        .positional(&["id", "decision"]),
    ]
}

fn err(e: WorkError) -> ToolError {
    match e {
        WorkError::NotFound(what) => ToolError::Core(aos_core::Error::NotFound { kind: "work", id: what }),
        WorkError::Invalid(m) | WorkError::Provider(m) => ToolError::InvalidArgs(m),
        other => ToolError::Core(aos_core::Error::Io(std::io::Error::other(other.to_string()))),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TaskArg {
    /// Just the prompt.
    Prompt(String),
    Full(LaunchTask),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaunchArgs {
    /// Project name or id (see work_status / the Work tab). Or give `folder`.
    #[serde(default)]
    pub project: Option<String>,
    /// A folder path; added as a project if new.
    #[serde(default)]
    pub folder: Option<String>,
    /// What each session should do: plain prompts, or objects with prompt, title, provider, model.
    pub tasks: Vec<TaskArg>,
    /// `claude` (default) or `codex`.
    #[serde(default)]
    pub provider: Option<String>,
    /// Model alias or id (claude: opus, sonnet, haiku). Empty for the CLI default.
    #[serde(default)]
    pub model: Option<String>,
    /// `supervised` (ask before edits and commands, default), `auto_edit` or `full_access`.
    #[serde(default)]
    pub runtime_mode: Option<String>,
    /// `plan` to have sessions plan first and wait for a go.
    #[serde(default)]
    pub interaction_mode: Option<String>,
}

fn launch(work: &Work, a: LaunchArgs) -> Result<Value, ToolError> {
    let tasks = a
        .tasks
        .into_iter()
        .map(|t| match t {
            TaskArg::Prompt(prompt) => LaunchTask { prompt, ..Default::default() },
            TaskArg::Full(t) => t,
        })
        .collect();
    let results = work
        .launch(LaunchMany {
            project: a.project,
            folder: a.folder,
            provider: a.provider,
            model: a.model,
            runtime_mode: a.runtime_mode,
            interaction_mode: a.interaction_mode,
            tasks,
        })
        .map_err(err)?;
    let started: Vec<Value> = results
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .map(|t| json!({ "thread": t.id, "title": t.title, "provider": t.provider, "status": t.status }))
        .collect();
    let failed: Vec<&String> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    Ok(json!({ "started": started, "failed": failed }))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusArgs {
    /// A thread id (or unique prefix) for that session's details.
    #[serde(default)]
    pub thread: Option<String>,
}

fn status(work: &Work, a: StatusArgs) -> Result<Value, ToolError> {
    match a.thread.filter(|t| !t.trim().is_empty()) {
        None => Ok(json!(work.overview().map_err(err)?)),
        Some(id) => {
            let d = work.thread(&id).map_err(err)?;
            let last: Vec<Value> = d
                .items
                .iter()
                .rev()
                .filter(|i| matches!(i.kind, ItemKind::AssistantMessage | ItemKind::UserMessage | ItemKind::Error | ItemKind::Plan))
                .take(6)
                .map(|i| json!({ "kind": i.kind, "text": crate::session::head_chars(&i.text, 600) }))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let open: Vec<&Approval> = d.approvals.iter().filter(|a| a.state == ApprovalState::Pending).collect();
            Ok(json!({ "thread": d.thread, "project": d.project, "last": last, "approvals": open, "plan": d.plan }))
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendArgs {
    /// Thread id or unique prefix.
    pub thread: String,
    /// The message.
    pub text: String,
}

fn send(work: &Work, a: SendArgs) -> Result<Value, ToolError> {
    let t = work.send(&a.thread, &a.text).map_err(err)?;
    Ok(json!({ "thread": t.id, "status": t.status }))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApproveArgs {
    /// Approval id, or a thread id to answer that thread's oldest open request.
    pub id: String,
    /// `approve` (default), `approve_session` or `deny`.
    #[serde(default)]
    pub decision: Option<String>,
}

fn approve(work: &Work, a: ApproveArgs) -> Result<Value, ToolError> {
    let decision = match a.decision.as_deref().filter(|s| !s.is_empty()) {
        Some(d) => Decision::parse(d).ok_or_else(|| ToolError::InvalidArgs(format!("unknown decision '{d}'")))?,
        None => Decision::Approve,
    };
    let approval = match work.decide(&a.id, decision) {
        Err(WorkError::NotFound(_)) => work.decide_oldest(&a.id, decision),
        other => other,
    }
    .map_err(err)?;
    Ok(json!({ "approval": approval }))
}
