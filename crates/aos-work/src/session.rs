//! What the provider adapters share: the commands a session takes, the provider-neutral
//! [`Update`]s they produce, and the [`Ctx`] that turns updates into rows and events.

use std::sync::Arc;

use serde_json::Value;
use tracing::warn;

use crate::store::ThreadRow;
use crate::supervisor::Inner;
use crate::types::*;

/// Sent to a running session task.
#[derive(Debug)]
pub enum SessionCmd {
    /// Start a turn with this text (queued if a turn is running).
    Send { text: String },
    /// Answer an approval.
    Decide { provider_request_id: String, decision: Decision },
    Interrupt,
    Stop,
}

/// Provider-neutral things that happened, produced by the translators in `claude.rs` and
/// `codex.rs`. `key` is the provider's own id for an item.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// The provider's session / thread id (used to resume later).
    SessionId(String),
    /// The provider's id for the running turn (Codex needs it to interrupt).
    ProviderTurnId(String),
    /// A new item started streaming.
    ItemStart { key: String, kind: ItemKind, title: String, text: String, data: Value },
    /// More text for a streaming item.
    Delta { key: String, text: String },
    /// An item is complete (created if it was never started).
    ItemDone { key: String, kind: ItemKind, status: ItemStatus, title: String, text: String, data: Value },
    /// The agent asks before doing something.
    Approval { request_id: String, kind: ApprovalKind, title: String, detail: String },
    Plan(Vec<PlanStep>),
    DiffChanged,
    TurnDone { status: TurnStatus, error: Option<String> },
    /// A non-fatal problem worth showing in the thread.
    Error(String),
}

/// One running session's view of the world: applies updates for one thread.
pub struct Ctx {
    pub inner: Arc<Inner>,
    pub thread: ThreadRow,
    pub turn: Option<Turn>,
}

impl Ctx {
    pub fn new(inner: Arc<Inner>, thread: ThreadRow) -> Ctx {
        Ctx { inner, thread, turn: None }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread.summary.id
    }

    fn turn_id(&self) -> Option<&str> {
        self.turn.as_ref().map(|t| t.id.as_str())
    }

    /// Record the user's message and open a turn. Returns false if the store failed.
    pub fn begin_turn(&mut self, text: &str) -> bool {
        let store = &self.inner.store;
        let tid = self.thread.summary.id.clone();
        let turn = match store.start_turn(&tid) {
            Ok(t) => t,
            Err(e) => {
                warn!("start turn: {e}");
                return false;
            }
        };
        if let Ok(item) = store.upsert_item(&tid, Some(&turn.id), None, ItemKind::UserMessage, ItemStatus::Done, "", text, "")
        {
            self.inner.emit(WorkEvent::ItemUpserted { item });
        }
        self.inner.emit(WorkEvent::TurnStarted { thread_id: tid.clone(), turn: turn.clone() });
        self.turn = Some(turn);
        self.inner.set_status(&tid, ThreadStatus::Working, None);
        true
    }

    /// A supervisor note in the thread (session started, resumed, CLI missing...).
    pub fn note(&self, kind: ItemKind, text: &str) {
        let tid = self.thread_id();
        let status = if kind == ItemKind::Error { ItemStatus::Failed } else { ItemStatus::Done };
        if let Ok(item) = self.inner.store.upsert_item(tid, self.turn_id(), None, kind, status, "", text, "") {
            self.inner.emit(WorkEvent::ItemUpserted { item });
        }
    }

    pub fn apply(&mut self, update: Update) {
        let inner = self.inner.clone();
        let store = &inner.store;
        let tid = self.thread.summary.id.clone();
        match update {
            Update::SessionId(id) => {
                if self.thread.provider_session_id.as_deref() != Some(id.as_str()) {
                    let _ = store.set_session_id(&tid, &id);
                    self.thread.provider_session_id = Some(id);
                }
            }
            Update::ProviderTurnId(id) => {
                if let Some(turn) = self.turn_id() {
                    let _ = store.set_provider_turn_id(turn, &id);
                }
            }
            Update::ItemStart { key, kind, title, text, data } => {
                let data = data_str(&data);
                match store.upsert_item(&tid, self.turn_id(), Some(&key), kind, ItemStatus::Running, &title, &text, &data) {
                    Ok(item) => inner.emit(WorkEvent::ItemUpserted { item }),
                    Err(e) => warn!("item start: {e}"),
                }
            }
            Update::Delta { key, text } => {
                if text.is_empty() {
                    return;
                }
                let id = match store.append_item_text(&tid, &key, &text) {
                    Ok(Some(id)) => id,
                    Ok(None) => {
                        // Delta before start (shouldn't happen): create the item.
                        match store.upsert_item(
                            &tid,
                            self.turn_id(),
                            Some(&key),
                            ItemKind::AssistantMessage,
                            ItemStatus::Running,
                            "",
                            &text,
                            "",
                        ) {
                            Ok(item) => {
                                inner.emit(WorkEvent::ItemUpserted { item });
                                return;
                            }
                            Err(e) => return warn!("delta: {e}"),
                        }
                    }
                    Err(e) => return warn!("delta: {e}"),
                };
                inner.emit(WorkEvent::ItemDelta { thread_id: tid, item_id: id, delta: text });
            }
            Update::ItemDone { key, kind, status, title, text, data } => {
                let data = data_str(&data);
                match store.upsert_item(&tid, self.turn_id(), Some(&key), kind, status, &title, &text, &data) {
                    Ok(item) => {
                        if item.kind == ItemKind::AssistantMessage && !item.text.trim().is_empty() {
                            let _ = store.set_preview(&tid, &item.text);
                        }
                        inner.emit(WorkEvent::ItemUpserted { item });
                    }
                    Err(e) => warn!("item done: {e}"),
                }
            }
            Update::Approval { request_id, kind, title, detail } => {
                match store.insert_approval(&tid, self.turn_id(), kind, &title, &detail, &request_id) {
                    Ok(approval) => {
                        inner.emit(WorkEvent::ApprovalRequested { approval });
                        inner.set_status(&tid, ThreadStatus::NeedsYou, None);
                    }
                    Err(e) => warn!("approval: {e}"),
                }
            }
            Update::Plan(steps) => {
                let _ = store.set_plan(&tid, &steps);
                inner.emit(WorkEvent::PlanUpdated { thread_id: tid, steps });
            }
            Update::DiffChanged => inner.emit(WorkEvent::DiffChanged { thread_id: tid }),
            Update::TurnDone { status, error } => {
                for approval in store.expire_approvals(&tid).unwrap_or_default() {
                    inner.emit(WorkEvent::ApprovalResolved { approval });
                }
                if let Some(turn) = self.turn.take() {
                    if let Ok(turn) = store.finish_turn(&turn.id, status, error.as_deref()) {
                        inner.emit(WorkEvent::TurnCompleted { thread_id: tid.clone(), turn });
                    }
                }
                if let Some(err) = &error {
                    self.note(ItemKind::Error, err);
                }
                let next = match status {
                    TurnStatus::Completed => ThreadStatus::Done,
                    TurnStatus::Interrupted => ThreadStatus::Idle,
                    TurnStatus::Failed => ThreadStatus::Error,
                    TurnStatus::Running => ThreadStatus::Working,
                };
                inner.set_status(&tid, next, error.as_deref());
                inner.emit(WorkEvent::DiffChanged { thread_id: tid });
            }
            Update::Error(msg) => self.note(ItemKind::Error, &msg),
        }
    }

    /// After an approval is answered the thread is working again (unless more are open).
    pub fn after_decision(&self) {
        let tid = self.thread_id();
        let open = self.inner.store.pending_approvals(Some(tid)).map(|v| v.len()).unwrap_or(0);
        if open == 0 && self.turn.is_some() {
            self.inner.set_status(tid, ThreadStatus::Working, None);
        } else {
            self.inner.touch(tid);
        }
    }

    /// The CLI went away. Fail the running turn with what stderr said.
    pub fn process_gone(&mut self, stderr: &str) {
        if self.turn.is_some() {
            let msg = if stderr.trim().is_empty() {
                "The CLI exited before the turn finished.".to_string()
            } else {
                format!("The CLI exited: {}", tail_chars(stderr, 600))
            };
            self.apply(Update::TurnDone { status: TurnStatus::Failed, error: Some(msg) });
        } else {
            for approval in self.inner.store.expire_approvals(self.thread_id()).unwrap_or_default() {
                self.inner.emit(WorkEvent::ApprovalResolved { approval });
            }
        }
    }
}

fn data_str(v: &Value) -> String {
    if v.is_null() { String::new() } else { v.to_string() }
}

/// The last `n` characters of `s`.
pub fn tail_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n { s.to_string() } else { format!("…{}", s.chars().skip(count - n).collect::<String>()) }
}

/// The first `n` characters of `s`.
pub fn head_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}
