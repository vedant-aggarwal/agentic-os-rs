//! The session supervisor: one long-lived [`Work`] in the server.
//!
//! Every public method is synchronous and quick (SQLite writes plus a channel send), so the
//! registry's sync tool handlers, server functions and HTTP routes can all call it. Sessions run
//! as Tokio tasks on the runtime captured at [`Work::start`], one per active thread, each owning
//! its CLI process (see `claude.rs`, `codex.rs`). Every change goes out on one broadcast bus of
//! [`WorkEvent`]s.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc};
use tracing::{info, warn};

use crate::config::WorkConfig;
use crate::error::{Result, WorkError};
use crate::session::{Ctx, SessionCmd, Update};
use crate::store::{Store, new_id, now_ms};
use crate::types::*;
use crate::{claude, codex, diff, t3import};

pub struct Inner {
    pub store: Store,
    pub config: WorkConfig,
    bus: broadcast::Sender<WorkEvent>,
    sessions: Mutex<HashMap<String, mpsc::UnboundedSender<SessionCmd>>>,
    handle: Handle,
    /// When T3's project list was last read (re-read at most once a minute).
    t3_checked: Mutex<Option<std::time::Instant>>,
}

impl Inner {
    pub fn emit(&self, event: WorkEvent) {
        let _ = self.bus.send(event);
    }

    /// Change a thread's status and tell everyone.
    pub fn set_status(&self, thread_id: &str, status: ThreadStatus, last_error: Option<&str>) {
        if let Err(e) = self.store.set_thread_status(thread_id, status, last_error) {
            warn!("set status: {e}");
        }
        self.touch(thread_id);
    }

    /// Re-send a thread's summary (pending count, preview...).
    pub fn touch(&self, thread_id: &str) {
        if let Ok(row) = self.store.thread(thread_id) {
            self.emit(WorkEvent::ThreadUpserted { thread: row.summary });
        }
    }

    /// Called by a session task as it ends.
    pub fn session_ended(&self, thread_id: &str) {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(thread_id);
        info!(thread = thread_id, "session ended");
    }
}

/// Cheap to clone.
#[derive(Clone)]
pub struct Work {
    inner: Arc<Inner>,
}

static GLOBAL: std::sync::OnceLock<Work> = std::sync::OnceLock::new();

/// What `work.status` returns.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Overview {
    pub working: Vec<ThreadSummary>,
    pub needs_you: Vec<ThreadSummary>,
    pub recent: Vec<ThreadSummary>,
    pub approvals: Vec<Approval>,
    pub claude_cli: bool,
    pub codex_cli: bool,
}

impl Work {
    /// Open the tables, recover from a restart, import T3 projects. Call inside a Tokio runtime.
    pub fn start(config: WorkConfig) -> Result<Work> {
        let store = Store::open(&config.db_path)?;
        let recovered = store.recover_after_restart()?;
        if recovered > 0 {
            info!(recovered, "threads marked idle after restart");
        }
        let (bus, _) = broadcast::channel(1024);
        let work = Work {
            inner: Arc::new(Inner {
                store,
                config,
                bus,
                sessions: Mutex::new(HashMap::new()),
                handle: Handle::current(),
                t3_checked: Mutex::new(None),
            }),
        };
        match work.import_t3() {
            Ok(n) if n > 0 => info!(n, "imported projects from T3 Code"),
            Ok(_) => {}
            Err(e) => warn!("T3 import skipped: {e}"),
        }
        info!(
            claude = ?work.inner.config.claude_bin.as_deref().map(crate::config::tilde),
            codex = ?work.inner.config.codex_bin.as_deref().map(crate::config::tilde),
            "work supervisor ready"
        );
        Ok(work)
    }

    /// Make this the process-wide supervisor (for Dioxus server functions). First call wins.
    pub fn install_global(&self) {
        let _ = GLOBAL.set(self.clone());
    }

    pub fn global() -> Option<Work> {
        GLOBAL.get().cloned()
    }

    pub fn config(&self) -> &WorkConfig {
        &self.inner.config
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WorkEvent> {
        self.inner.bus.subscribe()
    }

    // ------------------------------------------------------------ projects

    pub fn projects(&self) -> Result<Vec<Project>> {
        self.refresh_t3();
        self.inner.store.projects()
    }

    /// Pick up projects added in T3 Code since the last look (at most once a minute).
    fn refresh_t3(&self) {
        if self.inner.config.t3_db.is_none() {
            return;
        }
        {
            let mut last = self.inner.t3_checked.lock().unwrap_or_else(|e| e.into_inner());
            if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(60)) {
                return;
            }
            *last = Some(std::time::Instant::now());
        }
        if let Err(e) = self.import_t3() {
            warn!("T3 import skipped: {e}");
        }
    }

    /// T3 Code's own threads, read only, newest first. `project` is an Agentic OS project key;
    /// each row's `project_id` is mapped to the Agentic OS project (null when it isn't imported).
    pub fn t3_threads(&self, project: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
        let Some(path) = self.inner.config.t3_db.clone() else { return Ok(Vec::new()) };
        let map = self.inner.store.t3_project_map()?;
        let t3_project = match project.filter(|s| !s.is_empty()) {
            Some(key) => {
                let p = self.inner.store.resolve_project(key)?;
                match map.iter().find(|(_, aos)| **aos == p.id) {
                    Some((t3, _)) => Some(t3.clone()),
                    None => return Ok(Vec::new()),
                }
            }
            None => None,
        };
        let rows = t3import::read_threads(&path, t3_project.as_deref(), limit.clamp(1, 500))?;
        Ok(rows.into_iter().map(|t| t3_json(&t, &map)).collect())
    }

    /// One T3 thread with its last messages (read only).
    pub fn t3_thread(&self, id: &str) -> Result<serde_json::Value> {
        let path = self.inner.config.t3_db.clone().ok_or_else(|| WorkError::NotFound("T3 Code database".into()))?;
        let map = self.inner.store.t3_project_map()?;
        let (t, msgs) = t3import::read_thread(&path, id, 200)?.ok_or_else(|| WorkError::NotFound(format!("T3 thread '{id}'")))?;
        let mut v = t3_json(&t, &map);
        v["messages"] = serde_json::json!(msgs);
        Ok(v)
    }

    pub fn add_project(&self, name: &str, folder: &str) -> Result<Project> {
        if !std::path::Path::new(folder.trim()).is_dir() {
            return Err(WorkError::Invalid(format!("not a folder: {folder}")));
        }
        let (p, created) = self.inner.store.upsert_project(name, folder, "manual", None)?;
        if created {
            self.inner.emit(WorkEvent::ProjectUpserted { project: p.clone() });
        }
        Ok(p)
    }

    /// Import T3 Code's project list (read only). Returns how many were new.
    pub fn import_t3(&self) -> Result<usize> {
        let Some(path) = self.inner.config.t3_db.clone() else { return Ok(0) };
        let mut n = 0;
        for p in t3import::read_projects(&path)? {
            if !std::path::Path::new(&p.folder).is_dir() {
                continue;
            }
            let (project, created) = self.inner.store.upsert_project(&p.title, &p.folder, "t3", Some(&p.id))?;
            if created {
                n += 1;
                self.inner.emit(WorkEvent::ProjectUpserted { project });
            }
        }
        Ok(n)
    }

    fn project_for(&self, project: Option<&str>, folder: Option<&str>) -> Result<Project> {
        match (project.filter(|s| !s.trim().is_empty()), folder.filter(|s| !s.trim().is_empty())) {
            (Some(key), _) => self.inner.store.resolve_project(key),
            (None, Some(folder)) => self.add_project("", folder),
            (None, None) => Err(WorkError::Invalid("give a project or a folder".into())),
        }
    }

    // ------------------------------------------------------------ threads

    pub fn threads(&self, project: Option<&str>) -> Result<Vec<ThreadSummary>> {
        let pid = match project.filter(|s| !s.is_empty()) {
            Some(key) => Some(self.inner.store.resolve_project(key)?.id),
            None => None,
        };
        self.inner.store.threads(pid.as_deref(), 500)
    }

    pub fn thread(&self, id: &str) -> Result<ThreadDetail> {
        self.inner.store.detail(id)
    }

    pub fn create_thread(&self, req: NewThread) -> Result<ThreadSummary> {
        let project = self.project_for(req.project.as_deref(), req.folder.as_deref())?;
        let provider = match req.provider.as_deref().filter(|s| !s.is_empty()) {
            Some(p) => Provider::parse(p).ok_or_else(|| WorkError::Invalid(format!("unknown provider '{p}'")))?,
            None => Provider::Claude,
        };
        let runtime_mode = match req.runtime_mode.as_deref().filter(|s| !s.is_empty()) {
            Some(m) => RuntimeMode::parse(m).ok_or_else(|| WorkError::Invalid(format!("unknown runtime mode '{m}'")))?,
            None => RuntimeMode::Supervised,
        };
        let interaction_mode = match req.interaction_mode.as_deref().filter(|s| !s.is_empty()) {
            Some(m) => InteractionMode::parse(m).ok_or_else(|| WorkError::Invalid(format!("unknown mode '{m}'")))?,
            None => InteractionMode::Default,
        };
        let title = req
            .title
            .as_deref()
            .map(|t| one_line(t, 80))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                let t = one_line(&req.prompt, 60);
                if t.is_empty() { "New thread".to_string() } else { t }
            });
        let now = now_ms();
        let summary = ThreadSummary {
            id: new_id(),
            project_id: project.id,
            title,
            provider,
            model: req.model.unwrap_or_default().trim().to_string(),
            runtime_mode,
            interaction_mode,
            status: ThreadStatus::Idle,
            pending_approvals: 0,
            preview: String::new(),
            last_error: None,
            origin: req.origin.filter(|s| !s.is_empty()),
            created_at: now,
            updated_at: now,
        };
        self.inner.store.insert_thread(&summary)?;
        self.inner.emit(WorkEvent::ThreadUpserted { thread: summary.clone() });
        if !req.prompt.trim().is_empty() {
            return self.send(&summary.id, &req.prompt);
        }
        Ok(summary)
    }

    /// Send a message. Starts (or resumes) the CLI if needed; queues if a turn is running.
    pub fn send(&self, thread_id: &str, text: &str) -> Result<ThreadSummary> {
        if text.trim().is_empty() {
            return Err(WorkError::Invalid("empty message".into()));
        }
        let row = self.inner.store.thread(thread_id)?;
        let id = row.summary.id.clone();
        let tx = self.session(&id)?;
        if tx.send(SessionCmd::Send { text: text.to_string() }).is_err() {
            // The task ended between lookup and send: start a fresh one.
            self.inner.session_ended(&id);
            self.session(&id)?
                .send(SessionCmd::Send { text: text.to_string() })
                .map_err(|_| WorkError::Provider("session did not start".into()))?;
        }
        Ok(self.inner.store.thread(&id)?.summary)
    }

    /// The running session for a thread, starting one if needed.
    fn session(&self, thread_id: &str) -> Result<mpsc::UnboundedSender<SessionCmd>> {
        let mut sessions = self.inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = sessions.get(thread_id) {
            if !tx.is_closed() {
                return Ok(tx.clone());
            }
        }
        let row = self.inner.store.thread(thread_id)?;
        let bin = match row.summary.provider {
            Provider::Claude => self.inner.config.claude_bin.clone(),
            Provider::Codex => self.inner.config.codex_bin.clone(),
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let inner = self.inner.clone();
        match bin {
            Some(bin) => {
                self.inner.store.set_thread_status(thread_id, ThreadStatus::Starting, None)?;
                let task = match row.summary.provider {
                    Provider::Claude => self.inner.handle.spawn(claude::run(inner, row, bin, rx)),
                    Provider::Codex => self.inner.handle.spawn(codex::run(inner, row, bin, rx)),
                };
                drop(task);
            }
            None => {
                let var = match row.summary.provider {
                    Provider::Claude => "AOS_CLAUDE_BIN",
                    Provider::Codex => "AOS_CODEX_BIN",
                };
                let msg = format!("{} CLI not found. Install it or set {var}.", row.summary.provider.label());
                self.inner.handle.spawn(async move {
                    let mut rx = rx;
                    let mut ctx = Ctx::new(inner.clone(), row);
                    tokio::task::yield_now().await;
                    claude::fail_queued(&mut ctx, &mut rx, &msg);
                    inner.session_ended(ctx.thread_id());
                });
            }
        }
        sessions.insert(thread_id.to_string(), tx.clone());
        drop(sessions);
        self.inner.touch(thread_id);
        Ok(tx)
    }

    fn running(&self, thread_id: &str) -> Option<mpsc::UnboundedSender<SessionCmd>> {
        let sessions = self.inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.get(thread_id).filter(|tx| !tx.is_closed()).cloned()
    }

    pub fn decide(&self, approval_id: &str, decision: Decision) -> Result<Approval> {
        let (approval, request_id) = self.inner.store.approval(approval_id)?;
        if approval.state != ApprovalState::Pending {
            return Err(WorkError::Invalid(format!("approval is already {}", approval.state.as_str())));
        }
        let Some(tx) = self.running(&approval.thread_id) else {
            let a = self.inner.store.decide_approval(&approval.id, ApprovalState::Expired)?;
            self.inner.emit(WorkEvent::ApprovalResolved { approval: a.clone() });
            self.inner.touch(&a.thread_id);
            return Err(WorkError::Invalid("the session has ended; send a message to resume".into()));
        };
        let state = if decision.approves() { ApprovalState::Approved } else { ApprovalState::Denied };
        let a = self.inner.store.decide_approval(&approval.id, state)?;
        self.inner.emit(WorkEvent::ApprovalResolved { approval: a.clone() });
        let _ = tx.send(SessionCmd::Decide { provider_request_id: request_id, decision });
        Ok(a)
    }

    /// Answer the thread's oldest open approval (for "approve" from voice or the CLI).
    pub fn decide_oldest(&self, thread_id: &str, decision: Decision) -> Result<Approval> {
        let row = self.inner.store.thread(thread_id)?;
        let pending = self.inner.store.pending_approvals(Some(&row.summary.id))?;
        let first = pending.first().ok_or_else(|| WorkError::Invalid("nothing is waiting for approval".into()))?;
        self.decide(&first.id, decision)
    }

    pub fn interrupt(&self, thread_id: &str) -> Result<ThreadSummary> {
        let row = self.inner.store.thread(thread_id)?;
        match self.running(&row.summary.id) {
            Some(tx) => {
                let _ = tx.send(SessionCmd::Interrupt);
            }
            None if row.summary.status.is_active() => {
                self.inner.set_status(&row.summary.id, ThreadStatus::Idle, None);
            }
            None => {}
        }
        Ok(self.inner.store.thread(&row.summary.id)?.summary)
    }

    /// Stop the CLI for a thread (it resumes on the next message).
    pub fn stop(&self, thread_id: &str) -> Result<()> {
        let row = self.inner.store.thread(thread_id)?;
        if let Some(tx) = self.running(&row.summary.id) {
            let _ = tx.send(SessionCmd::Stop);
        }
        Ok(())
    }

    /// Delete a thread and everything under it (its CLI is stopped first). The project folder and
    /// any files the agent changed are left alone.
    pub fn delete_thread(&self, thread_id: &str) -> Result<()> {
        let row = self.inner.store.thread(thread_id)?;
        if let Some(tx) = self.running(&row.summary.id) {
            let _ = tx.send(SessionCmd::Stop);
        }
        self.inner.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&row.summary.id);
        self.inner.store.delete_thread(&row.summary.id)?;
        self.inner.emit(WorkEvent::Resync);
        Ok(())
    }

    /// Stop every session (server shutdown).
    pub fn stop_all(&self) {
        let sessions = self.inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
        for tx in sessions.values() {
            let _ = tx.send(SessionCmd::Stop);
        }
    }

    /// Start one thread per task. Tasks that fail to start are reported, the rest still run.
    pub fn launch(&self, req: LaunchMany) -> Result<Vec<std::result::Result<ThreadSummary, String>>> {
        if req.tasks.is_empty() {
            return Err(WorkError::Invalid("no tasks".into()));
        }
        // Resolve the project once so a bad name fails the whole call early.
        let project = self.project_for(req.project.as_deref(), req.folder.as_deref())?;
        Ok(req
            .tasks
            .into_iter()
            .filter(|t| !t.prompt.trim().is_empty())
            .map(|t| {
                self.create_thread(NewThread {
                    project: Some(project.id.clone()),
                    folder: None,
                    provider: t.provider.or_else(|| req.provider.clone()),
                    model: t.model.or_else(|| req.model.clone()),
                    runtime_mode: req.runtime_mode.clone(),
                    interaction_mode: req.interaction_mode.clone(),
                    title: t.title,
                    prompt: t.prompt,
                    origin: t.origin,
                })
                .map_err(|e| e.to_string())
            })
            .collect())
    }

    pub fn diff(&self, thread_id: &str) -> Result<ThreadDiff> {
        let row = self.inner.store.thread(thread_id)?;
        Ok(diff::folder_diff(&row.summary.id, &row.folder))
    }

    pub fn overview(&self) -> Result<Overview> {
        let threads = self.inner.store.threads(None, 200)?;
        let pick = |f: &dyn Fn(&ThreadSummary) -> bool| threads.iter().filter(|t| f(t)).cloned().collect::<Vec<_>>();
        Ok(Overview {
            working: pick(&|t| matches!(t.status, ThreadStatus::Working | ThreadStatus::Starting)),
            needs_you: pick(&|t| t.status == ThreadStatus::NeedsYou || t.pending_approvals > 0),
            recent: threads.iter().take(10).cloned().collect(),
            approvals: self.inner.store.pending_approvals(None)?,
            claude_cli: self.inner.config.claude_bin.is_some(),
            codex_cli: self.inner.config.codex_bin.is_some(),
        })
    }

    /// For tests: apply an update to a thread as if a session produced it.
    #[doc(hidden)]
    pub fn apply_for_test(&self, thread_id: &str, updates: Vec<Update>) -> Result<()> {
        let row = self.inner.store.thread(thread_id)?;
        let mut ctx = Ctx::new(self.inner.clone(), row);
        ctx.begin_turn("test");
        for u in updates {
            ctx.apply(u);
        }
        Ok(())
    }
}

fn t3_json(t: &t3import::T3Thread, map: &HashMap<String, String>) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "project_id": map.get(&t.project_id),
        "title": t.title,
        "provider": t.provider,
        "status": t.status,
        "created_at": t.created_at,
        "updated_at": t.updated_at,
        "source": "t3",
    })
}
