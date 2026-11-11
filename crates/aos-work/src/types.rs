//! Types shared by the server and every client (including wasm). Plain serde, no runtime deps.
//!
//! The model follows T3 Code's orchestration read model (see `NOTICE`): a **project** is a folder,
//! a **thread** is one conversation with one provider, a thread has **turns** (one user message and
//! everything the agent did in reply), and a turn has **items** (messages, reasoning, tool calls,
//! plans, errors). **Approvals** are requests from the agent that wait for the owner.

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch.
pub type Millis = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// The `claude` CLI in stream-json mode, on his own login.
    #[default]
    Claude,
    /// `codex app-server` (JSON-RPC over stdio), on his ChatGPT login.
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Claude, Provider::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Codex => "Codex",
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" | "anthropic" => Some(Provider::Claude),
            "codex" | "openai" => Some(Provider::Codex),
            _ => None,
        }
    }

    /// Model choices shown in the picker. `""` means "the CLI's default". Any other string can be
    /// typed in by hand; these are only shortcuts.
    pub fn models(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Provider::Claude => &[("", "Default"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")],
            Provider::Codex => &[("", "Default")],
        }
    }
}

/// How much the agent may do without asking. T3 calls this the runtime mode
/// (`approval-required` / `full-access`); we add the middle step Claude supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    /// Ask before edits and commands.
    #[default]
    Supervised,
    /// File edits go through; commands still ask.
    AutoEdit,
    /// No questions. Use on folders where that is fine.
    FullAccess,
}

impl RuntimeMode {
    pub const ALL: [RuntimeMode; 3] = [RuntimeMode::Supervised, RuntimeMode::AutoEdit, RuntimeMode::FullAccess];

    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeMode::Supervised => "supervised",
            RuntimeMode::AutoEdit => "auto_edit",
            RuntimeMode::FullAccess => "full_access",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RuntimeMode::Supervised => "Ask first",
            RuntimeMode::AutoEdit => "Auto-edit",
            RuntimeMode::FullAccess => "Full access",
        }
    }

    pub fn parse(s: &str) -> Option<RuntimeMode> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "supervised" | "ask" | "approval_required" | "default" => Some(RuntimeMode::Supervised),
            "auto_edit" | "accept_edits" | "autoedit" => Some(RuntimeMode::AutoEdit),
            "full_access" | "full" | "bypass" | "yolo" => Some(RuntimeMode::FullAccess),
            _ => None,
        }
    }
}

/// T3's interaction mode: normal work, or plan first and wait for a go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
    #[default]
    Default,
    Plan,
}

impl InteractionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            InteractionMode::Default => "default",
            InteractionMode::Plan => "plan",
        }
    }

    pub fn parse(s: &str) -> Option<InteractionMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" | "build" | "normal" => Some(InteractionMode::Default),
            "plan" => Some(InteractionMode::Plan),
            _ => None,
        }
    }
}

/// What the sidebar shows. `NeedsYou` wins over everything while an approval is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    /// Created, nothing sent yet, or stopped by hand.
    #[default]
    Idle,
    /// Starting the CLI.
    Starting,
    /// A turn is running.
    Working,
    /// An approval (or a plan) is waiting for him.
    NeedsYou,
    /// The last turn finished.
    Done,
    /// The last turn failed or the CLI died.
    Error,
}

impl ThreadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadStatus::Idle => "idle",
            ThreadStatus::Starting => "starting",
            ThreadStatus::Working => "working",
            ThreadStatus::NeedsYou => "needs_you",
            ThreadStatus::Done => "done",
            ThreadStatus::Error => "error",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThreadStatus::Idle => "Idle",
            ThreadStatus::Starting => "Starting",
            ThreadStatus::Working => "Working",
            ThreadStatus::NeedsYou => "Needs you",
            ThreadStatus::Done => "Done",
            ThreadStatus::Error => "Error",
        }
    }

    pub fn parse(s: &str) -> Option<ThreadStatus> {
        Some(match s {
            "idle" => ThreadStatus::Idle,
            "starting" => ThreadStatus::Starting,
            "working" => ThreadStatus::Working,
            "needs_you" => ThreadStatus::NeedsYou,
            "done" => ThreadStatus::Done,
            "error" => ThreadStatus::Error,
            _ => return None,
        })
    }

    /// The agent is busy (show a spinner, offer Stop).
    pub fn is_active(self) -> bool {
        matches!(self, ThreadStatus::Starting | ThreadStatus::Working | ThreadStatus::NeedsYou)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Absolute folder path on the PC.
    pub folder: String,
    /// `manual`, or `t3` when imported from T3 Code.
    pub source: String,
    pub created_at: Millis,
}

/// One row of the thread list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub provider: Provider,
    /// `""` = the CLI's default model.
    pub model: String,
    pub runtime_mode: RuntimeMode,
    pub interaction_mode: InteractionMode,
    pub status: ThreadStatus,
    /// Open approvals.
    pub pending_approvals: u32,
    /// The last assistant text, one line, for the list.
    pub preview: String,
    pub last_error: Option<String>,
    /// Link back to an idea / task / job (`task:<id>` ...), when launched from one.
    pub origin: Option<String>,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    Completed,
    Interrupted,
    Failed,
}

impl TurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TurnStatus::Running => "running",
            TurnStatus::Completed => "completed",
            TurnStatus::Interrupted => "interrupted",
            TurnStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> TurnStatus {
        match s {
            "completed" => TurnStatus::Completed,
            "interrupted" => TurnStatus::Interrupted,
            "failed" => TurnStatus::Failed,
            _ => TurnStatus::Running,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    pub thread_id: String,
    pub seq: i64,
    pub status: TurnStatus,
    pub error: Option<String>,
    pub started_at: Millis,
    pub completed_at: Option<Millis>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    UserMessage,
    AssistantMessage,
    Reasoning,
    /// A tool call or command (T3's "activity" / work-log entry).
    ToolCall,
    Plan,
    Error,
    /// Notes from the supervisor (session started, resumed, stopped).
    System,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::UserMessage => "user_message",
            ItemKind::AssistantMessage => "assistant_message",
            ItemKind::Reasoning => "reasoning",
            ItemKind::ToolCall => "tool_call",
            ItemKind::Plan => "plan",
            ItemKind::Error => "error",
            ItemKind::System => "system",
        }
    }

    pub fn parse(s: &str) -> ItemKind {
        match s {
            "user_message" => ItemKind::UserMessage,
            "assistant_message" => ItemKind::AssistantMessage,
            "reasoning" => ItemKind::Reasoning,
            "tool_call" => ItemKind::ToolCall,
            "plan" => ItemKind::Plan,
            "error" => ItemKind::Error,
            _ => ItemKind::System,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Running,
    Done,
    Failed,
    Declined,
}

impl ItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemStatus::Running => "running",
            ItemStatus::Done => "done",
            ItemStatus::Failed => "failed",
            ItemStatus::Declined => "declined",
        }
    }

    pub fn parse(s: &str) -> ItemStatus {
        match s {
            "done" => ItemStatus::Done,
            "failed" => ItemStatus::Failed,
            "declined" => ItemStatus::Declined,
            _ => ItemStatus::Running,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub seq: i64,
    pub kind: ItemKind,
    pub status: ItemStatus,
    /// For tool calls: a one-line title (`Bash: cargo test`, `Edit src/main.rs`).
    pub title: String,
    /// Message text, reasoning text, tool output (trimmed), plan text.
    pub text: String,
    /// Provider details as JSON text (tool input, plan steps). Kept as a string so every client
    /// can decode it without a JSON value type in its signals.
    pub data: String,
    pub created_at: Millis,
    pub updated_at: Millis,
}

impl Item {
    /// A tool call's input (or Codex arguments / file changes), pretty JSON; empty if none.
    pub fn input_pretty(&self) -> String {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&self.data) else { return String::new() };
        let input = v.get("input").or(v.get("arguments")).or(v.get("changes")).or(v.get("command"));
        match input {
            None | Some(serde_json::Value::Null) => String::new(),
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Object(o)) if o.is_empty() => String::new(),
            Some(other) => serde_json::to_string_pretty(other).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// Run a shell command.
    Command,
    /// Change files.
    FileChange,
    /// Any other tool (Claude's `can_use_tool`).
    Tool,
    /// Leave plan mode and start building (Claude's `ExitPlanMode`).
    Plan,
}

impl ApprovalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalKind::Command => "command",
            ApprovalKind::FileChange => "file_change",
            ApprovalKind::Tool => "tool",
            ApprovalKind::Plan => "plan",
        }
    }

    pub fn parse(s: &str) -> ApprovalKind {
        match s {
            "command" => ApprovalKind::Command,
            "file_change" => ApprovalKind::FileChange,
            "plan" => ApprovalKind::Plan,
            _ => ApprovalKind::Tool,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
    /// The session ended before he answered.
    Expired,
}

impl ApprovalState {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalState::Pending => "pending",
            ApprovalState::Approved => "approved",
            ApprovalState::Denied => "denied",
            ApprovalState::Expired => "expired",
        }
    }

    pub fn parse(s: &str) -> ApprovalState {
        match s {
            "approved" => ApprovalState::Approved,
            "denied" => ApprovalState::Denied,
            "expired" => ApprovalState::Expired,
            _ => ApprovalState::Pending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    /// Approve this and the same kind of request for the rest of the session.
    ApproveSession,
    Deny,
}

impl Decision {
    pub fn parse(s: &str) -> Option<Decision> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "approve" | "allow" | "yes" | "accept" | "ok" => Some(Decision::Approve),
            "approve_session" | "always" | "accept_for_session" => Some(Decision::ApproveSession),
            "deny" | "no" | "decline" | "reject" => Some(Decision::Deny),
            _ => None,
        }
    }

    pub fn approves(self) -> bool {
        !matches!(self, Decision::Deny)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub kind: ApprovalKind,
    pub state: ApprovalState,
    /// `Run cargo test`, `Write src/lib.rs`, `Start building from this plan`.
    pub title: String,
    /// The command, file list, or plan text.
    pub detail: String,
    pub created_at: Millis,
    pub decided_at: Option<Millis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanStep {
    pub step: String,
    /// `pending`, `in_progress`, `completed`.
    pub status: String,
}

/// Everything the thread view needs in one call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadDetail {
    pub thread: ThreadSummary,
    pub project: Option<Project>,
    pub turns: Vec<Turn>,
    pub items: Vec<Item>,
    pub approvals: Vec<Approval>,
    /// The latest plan the agent shared (Codex `turn/plan/updated`, Claude `TodoWrite`).
    pub plan: Vec<PlanStep>,
}

/// One file in the diff view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffFile {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    /// `modified`, `added`, `deleted`, `untracked`, `renamed`.
    pub change: String,
    /// The unified diff for this file (may be trimmed for very large files).
    pub patch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadDiff {
    pub thread_id: String,
    pub folder: String,
    /// False when the folder is not a git repo; `files` is then empty and `note` says why.
    pub is_git: bool,
    pub files: Vec<DiffFile>,
    pub note: Option<String>,
}

/// Arguments for a new thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "runtime", derive(schemars::JsonSchema))]
pub struct NewThread {
    /// Project id or name. Either this or `folder`.
    #[serde(default)]
    pub project: Option<String>,
    /// A folder path; becomes (or finds) a project.
    #[serde(default)]
    pub folder: Option<String>,
    /// `claude` (default) or `codex`.
    #[serde(default)]
    pub provider: Option<String>,
    /// Model alias or id; empty for the CLI default.
    #[serde(default)]
    pub model: Option<String>,
    /// `supervised` (default), `auto_edit`, `full_access`.
    #[serde(default)]
    pub runtime_mode: Option<String>,
    /// `default` or `plan`.
    #[serde(default)]
    pub interaction_mode: Option<String>,
    /// Thread title; defaults to the first line of the prompt.
    #[serde(default)]
    pub title: Option<String>,
    /// The first message. Empty creates an idle thread.
    #[serde(default)]
    pub prompt: String,
    /// Link to an idea / task / job, e.g. `task:01HX...`.
    #[serde(default)]
    pub origin: Option<String>,
}

/// One call that starts many threads (T3's "launch these sessions for these tasks").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "runtime", derive(schemars::JsonSchema))]
pub struct LaunchMany {
    /// Project id or name for every task (or give `folder`).
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    /// Default provider for tasks that don't name one.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub runtime_mode: Option<String>,
    #[serde(default)]
    pub interaction_mode: Option<String>,
    /// One entry per thread.
    pub tasks: Vec<LaunchTask>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "runtime", derive(schemars::JsonSchema))]
pub struct LaunchTask {
    /// What this session should do. Sent as the first message.
    pub prompt: String,
    #[serde(default)]
    pub title: Option<String>,
    /// Override the launch-wide provider for this task.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub origin: Option<String>,
}

/// The live event stream. Names follow T3's orchestration events, flattened for a single bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkEvent {
    /// First event on every stream.
    Hello { at: Millis },
    ProjectUpserted { project: Project },
    /// A thread was created or its summary changed (status, title, preview, pending approvals).
    ThreadUpserted { thread: ThreadSummary },
    TurnStarted { thread_id: String, turn: Turn },
    TurnCompleted { thread_id: String, turn: Turn },
    /// An item was created or replaced (final text, tool result, status).
    ItemUpserted { item: Item },
    /// Streaming text for an item (assistant message or reasoning).
    ItemDelta { thread_id: String, item_id: String, delta: String },
    ApprovalRequested { approval: Approval },
    ApprovalResolved { approval: Approval },
    PlanUpdated { thread_id: String, steps: Vec<PlanStep> },
    /// The agent changed files; refetch the diff.
    DiffChanged { thread_id: String },
    /// The client fell behind; refetch everything.
    Resync,
}

impl WorkEvent {
    /// The thread this event belongs to, if any.
    pub fn thread_id(&self) -> Option<&str> {
        match self {
            WorkEvent::ThreadUpserted { thread } => Some(&thread.id),
            WorkEvent::TurnStarted { thread_id, .. }
            | WorkEvent::TurnCompleted { thread_id, .. }
            | WorkEvent::ItemDelta { thread_id, .. }
            | WorkEvent::PlanUpdated { thread_id, .. }
            | WorkEvent::DiffChanged { thread_id } => Some(thread_id),
            WorkEvent::ItemUpserted { item } => Some(&item.thread_id),
            WorkEvent::ApprovalRequested { approval } | WorkEvent::ApprovalResolved { approval } => {
                Some(&approval.thread_id)
            }
            WorkEvent::Hello { .. } | WorkEvent::ProjectUpserted { .. } | WorkEvent::Resync => None,
        }
    }
}

/// First line of `text`, trimmed to `max` characters (for titles and previews).
pub fn one_line(text: &str, max: usize) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() <= max {
        line.to_string()
    } else {
        let mut s: String = line.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_json_is_tagged() {
        let e = WorkEvent::ItemDelta { thread_id: "t".into(), item_id: "i".into(), delta: "PO".into() };
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(s, r#"{"type":"item_delta","thread_id":"t","item_id":"i","delta":"PO"}"#);
        let back: WorkEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn one_line_trims() {
        assert_eq!(one_line("\n  hello world \nmore", 50), "hello world");
        assert_eq!(one_line("abcdef", 4), "abc…");
    }

    #[test]
    fn parse_aliases() {
        assert_eq!(Provider::parse("Codex"), Some(Provider::Codex));
        assert_eq!(RuntimeMode::parse("full-access"), Some(RuntimeMode::FullAccess));
        assert_eq!(Decision::parse("allow"), Some(Decision::Approve));
    }
}
