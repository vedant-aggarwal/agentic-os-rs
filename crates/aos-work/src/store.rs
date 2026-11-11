//! SQLite persistence for projects, threads, turns, items and approvals.
//!
//! One connection behind a mutex: writes are small and local, and every call is short, so the
//! lock is never held across an `.await`. WAL, `busy_timeout`, foreign keys on (tech-stack §3.15).
//! Tables are prefixed `work_` so they can live in the main Agentic OS database later.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, WorkError};
use crate::types::*;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS work_project (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    folder      TEXT NOT NULL UNIQUE,
    source      TEXT NOT NULL DEFAULT 'manual',
    external_id TEXT,
    archived    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS work_thread (
    id                  TEXT PRIMARY KEY,
    project_id          TEXT NOT NULL REFERENCES work_project(id),
    title               TEXT NOT NULL,
    provider            TEXT NOT NULL,
    model               TEXT NOT NULL DEFAULT '',
    runtime_mode        TEXT NOT NULL,
    interaction_mode    TEXT NOT NULL,
    status              TEXT NOT NULL,
    provider_session_id TEXT,
    preview             TEXT NOT NULL DEFAULT '',
    last_error          TEXT,
    origin              TEXT,
    plan                TEXT NOT NULL DEFAULT '[]',
    archived            INTEGER NOT NULL DEFAULT 0,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS work_thread_project ON work_thread(project_id, updated_at);

CREATE TABLE IF NOT EXISTS work_turn (
    id               TEXT PRIMARY KEY,
    thread_id        TEXT NOT NULL REFERENCES work_thread(id),
    seq              INTEGER NOT NULL,
    status           TEXT NOT NULL,
    provider_turn_id TEXT,
    error            TEXT,
    started_at       INTEGER NOT NULL,
    completed_at     INTEGER
);
CREATE INDEX IF NOT EXISTS work_turn_thread ON work_turn(thread_id, seq);

CREATE TABLE IF NOT EXISTS work_item (
    id               TEXT PRIMARY KEY,
    thread_id        TEXT NOT NULL REFERENCES work_thread(id),
    turn_id          TEXT,
    seq              INTEGER NOT NULL,
    kind             TEXT NOT NULL,
    status           TEXT NOT NULL,
    title            TEXT NOT NULL DEFAULT '',
    text             TEXT NOT NULL DEFAULT '',
    data             TEXT NOT NULL DEFAULT '{}',
    provider_item_id TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS work_item_thread ON work_item(thread_id, seq);
CREATE UNIQUE INDEX IF NOT EXISTS work_item_provider ON work_item(thread_id, provider_item_id);

CREATE TABLE IF NOT EXISTS work_approval (
    id                  TEXT PRIMARY KEY,
    thread_id           TEXT NOT NULL REFERENCES work_thread(id),
    turn_id             TEXT,
    kind                TEXT NOT NULL,
    state               TEXT NOT NULL,
    title               TEXT NOT NULL,
    detail              TEXT NOT NULL DEFAULT '',
    provider_request_id TEXT NOT NULL,
    created_at          INTEGER NOT NULL,
    decided_at          INTEGER
);
CREATE INDEX IF NOT EXISTS work_approval_thread ON work_approval(thread_id, state);
"#;

pub fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Time-ordered ids (UUIDv7), so sorting by id sorts by creation.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

/// Thread fields the supervisor needs to start a session.
#[derive(Debug, Clone)]
pub struct ThreadRow {
    pub summary: ThreadSummary,
    pub provider_session_id: Option<String>,
    pub folder: String,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
        )?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves the connection usable; keep going.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// After a restart no CLI is running: busy threads become idle with a note, open approvals
    /// expire, running turns are marked interrupted. Returns how many threads were touched.
    pub fn recover_after_restart(&self) -> Result<usize> {
        let db = self.db();
        let now = now_ms();
        db.execute("UPDATE work_approval SET state='expired', decided_at=?1 WHERE state='pending'", [now])?;
        db.execute(
            "UPDATE work_turn SET status='interrupted', error='server restarted', completed_at=?1 WHERE status='running'",
            [now],
        )?;
        db.execute("UPDATE work_item SET status='failed', updated_at=?1 WHERE status='running'", [now])?;
        let n = db.execute(
            "UPDATE work_thread SET status='idle', last_error='Stopped when the server restarted. Send a message to resume.', updated_at=?1
             WHERE status IN ('starting','working','needs_you')",
            [now],
        )?;
        Ok(n)
    }

    // ---------------------------------------------------------------- projects

    pub fn projects(&self) -> Result<Vec<Project>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id, name, folder, source, created_at FROM work_project WHERE archived=0 ORDER BY lower(name)",
        )?;
        let rows = st.query_map([], project_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Find a project by id, exact name (case-insensitive) or folder.
    pub fn resolve_project(&self, key: &str) -> Result<Project> {
        let key = key.trim();
        let db = self.db();
        let found = db
            .query_row(
                "SELECT id, name, folder, source, created_at FROM work_project
                 WHERE archived=0 AND (id=?1 OR lower(name)=lower(?1) OR lower(folder)=lower(?1)) LIMIT 1",
                [key],
                project_row,
            )
            .optional()?;
        found.ok_or_else(|| WorkError::NotFound(format!("project '{key}'")))
    }

    /// Add a folder as a project, or return the existing one for that folder.
    pub fn upsert_project(&self, name: &str, folder: &str, source: &str, external_id: Option<&str>) -> Result<(Project, bool)> {
        let folder = normalize_folder(folder);
        let db = self.db();
        if let Some(p) = db
            .query_row(
                "SELECT id, name, folder, source, created_at FROM work_project WHERE lower(folder)=lower(?1)",
                [&folder],
                project_row,
            )
            .optional()?
        {
            db.execute("UPDATE work_project SET archived=0 WHERE id=?1", [&p.id])?;
            return Ok((p, false));
        }
        let name = if name.trim().is_empty() { folder_name(&folder) } else { name.trim().to_string() };
        let p = Project { id: new_id(), name, folder, source: source.to_string(), created_at: now_ms() };
        db.execute(
            "INSERT INTO work_project (id, name, folder, source, external_id, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![p.id, p.name, p.folder, p.source, external_id, p.created_at],
        )?;
        Ok((p, true))
    }

    // ---------------------------------------------------------------- threads

    pub fn insert_thread(&self, t: &ThreadSummary) -> Result<()> {
        self.db().execute(
            "INSERT INTO work_thread (id, project_id, title, provider, model, runtime_mode, interaction_mode, status,
               preview, last_error, origin, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                t.id,
                t.project_id,
                t.title,
                t.provider.as_str(),
                t.model,
                t.runtime_mode.as_str(),
                t.interaction_mode.as_str(),
                t.status.as_str(),
                t.preview,
                t.last_error,
                t.origin,
                t.created_at,
                t.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn threads(&self, project_id: Option<&str>, limit: usize) -> Result<Vec<ThreadSummary>> {
        let db = self.db();
        let sql = format!(
            "{THREAD_SELECT} WHERE t.archived=0 AND (?1 IS NULL OR t.project_id=?1) ORDER BY t.updated_at DESC LIMIT ?2"
        );
        let mut st = db.prepare(&sql)?;
        let rows = st
            .query_map(params![project_id, limit as i64], |r| thread_row(r).map(|t| t.summary))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn thread(&self, id: &str) -> Result<ThreadRow> {
        let db = self.db();
        let sql = format!("{THREAD_SELECT} WHERE t.id=?1 OR t.id LIKE ?1 || '%' ORDER BY t.id LIMIT 2");
        let mut st = db.prepare(&sql)?;
        let mut rows = st.query_map([id.trim()], thread_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        match rows.len() {
            0 => Err(WorkError::NotFound(format!("thread '{id}'"))),
            1 => Ok(rows.remove(0)),
            _ if rows[0].summary.id == id => Ok(rows.remove(0)),
            _ => Err(WorkError::Invalid(format!("thread id '{id}' matches more than one thread"))),
        }
    }

    pub fn set_thread_status(&self, id: &str, status: ThreadStatus, last_error: Option<&str>) -> Result<()> {
        self.db().execute(
            "UPDATE work_thread SET status=?2, last_error=?3, updated_at=?4 WHERE id=?1",
            params![id, status.as_str(), last_error, now_ms()],
        )?;
        Ok(())
    }

    pub fn set_session_id(&self, id: &str, session_id: &str) -> Result<()> {
        self.db().execute("UPDATE work_thread SET provider_session_id=?2 WHERE id=?1", params![id, session_id])?;
        Ok(())
    }

    pub fn set_preview(&self, id: &str, preview: &str) -> Result<()> {
        self.db().execute(
            "UPDATE work_thread SET preview=?2, updated_at=?3 WHERE id=?1",
            params![id, one_line(preview, 140), now_ms()],
        )?;
        Ok(())
    }

    pub fn set_plan(&self, id: &str, steps: &[PlanStep]) -> Result<()> {
        let json = serde_json::to_string(steps)?;
        self.db().execute("UPDATE work_thread SET plan=?2 WHERE id=?1", params![id, json])?;
        Ok(())
    }

    pub fn set_runtime_mode(&self, id: &str, mode: RuntimeMode, interaction: InteractionMode) -> Result<()> {
        self.db().execute(
            "UPDATE work_thread SET runtime_mode=?2, interaction_mode=?3 WHERE id=?1",
            params![id, mode.as_str(), interaction.as_str()],
        )?;
        Ok(())
    }

    pub fn detail(&self, id: &str) -> Result<ThreadDetail> {
        let row = self.thread(id)?;
        let id = row.summary.id.clone();
        let project = self.resolve_project(&row.summary.project_id).ok();
        let db = self.db();
        let turns = db
            .prepare(
                "SELECT id, thread_id, seq, status, error, started_at, completed_at FROM work_turn WHERE thread_id=?1 ORDER BY seq",
            )?
            .query_map([&id], |r| {
                Ok(Turn {
                    id: r.get(0)?,
                    thread_id: r.get(1)?,
                    seq: r.get(2)?,
                    status: TurnStatus::parse(&r.get::<_, String>(3)?),
                    error: r.get(4)?,
                    started_at: r.get(5)?,
                    completed_at: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let items = db
            .prepare(&format!("{ITEM_SELECT} WHERE thread_id=?1 ORDER BY seq"))?
            .query_map([&id], item_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let approvals = db
            .prepare(&format!("{APPROVAL_SELECT} WHERE thread_id=?1 ORDER BY created_at"))?
            .query_map([&id], approval_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let plan_json: String = db.query_row("SELECT plan FROM work_thread WHERE id=?1", [&id], |r| r.get(0))?;
        let plan = serde_json::from_str(&plan_json).unwrap_or_default();
        Ok(ThreadDetail { thread: row.summary, project, turns, items, approvals, plan })
    }

    // ---------------------------------------------------------------- turns

    pub fn start_turn(&self, thread_id: &str) -> Result<Turn> {
        let db = self.db();
        let seq: i64 =
            db.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM work_turn WHERE thread_id=?1", [thread_id], |r| r.get(0))?;
        let turn = Turn {
            id: new_id(),
            thread_id: thread_id.to_string(),
            seq,
            status: TurnStatus::Running,
            error: None,
            started_at: now_ms(),
            completed_at: None,
        };
        db.execute(
            "INSERT INTO work_turn (id, thread_id, seq, status, started_at) VALUES (?1,?2,?3,'running',?4)",
            params![turn.id, turn.thread_id, turn.seq, turn.started_at],
        )?;
        Ok(turn)
    }

    pub fn set_provider_turn_id(&self, turn_id: &str, provider_turn_id: &str) -> Result<()> {
        self.db().execute("UPDATE work_turn SET provider_turn_id=?2 WHERE id=?1", params![turn_id, provider_turn_id])?;
        Ok(())
    }

    pub fn finish_turn(&self, turn_id: &str, status: TurnStatus, error: Option<&str>) -> Result<Turn> {
        let db = self.db();
        let now = now_ms();
        db.execute(
            "UPDATE work_turn SET status=?2, error=?3, completed_at=?4 WHERE id=?1",
            params![turn_id, status.as_str(), error, now],
        )?;
        // Anything still streaming in this turn is over.
        let item_status = if status == TurnStatus::Completed { "done" } else { "failed" };
        db.execute(
            "UPDATE work_item SET status=?2, updated_at=?3 WHERE turn_id=?1 AND status='running'",
            params![turn_id, item_status, now],
        )?;
        let turn = db.query_row(
            "SELECT id, thread_id, seq, status, error, started_at, completed_at FROM work_turn WHERE id=?1",
            [turn_id],
            |r| {
                Ok(Turn {
                    id: r.get(0)?,
                    thread_id: r.get(1)?,
                    seq: r.get(2)?,
                    status: TurnStatus::parse(&r.get::<_, String>(3)?),
                    error: r.get(4)?,
                    started_at: r.get(5)?,
                    completed_at: r.get(6)?,
                })
            },
        )?;
        Ok(turn)
    }

    // ---------------------------------------------------------------- items

    /// Insert an item, or update it when `provider_item_id` was seen before in this thread.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_item(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
        provider_item_id: Option<&str>,
        kind: ItemKind,
        status: ItemStatus,
        title: &str,
        text: &str,
        data: &str,
    ) -> Result<Item> {
        let db = self.db();
        let now = now_ms();
        let existing: Option<String> = match provider_item_id {
            Some(pid) => db
                .query_row(
                    "SELECT id FROM work_item WHERE thread_id=?1 AND provider_item_id=?2",
                    params![thread_id, pid],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        let id = match existing {
            Some(id) => {
                db.execute(
                    "UPDATE work_item SET status=?2,
                        title=CASE WHEN ?3='' THEN title ELSE ?3 END,
                        text=CASE WHEN ?4='' THEN text ELSE ?4 END,
                        data=CASE WHEN ?5='' OR ?5='{}' THEN data ELSE ?5 END,
                        updated_at=?6 WHERE id=?1",
                    params![id, status.as_str(), title, text, data, now],
                )?;
                id
            }
            None => {
                let id = new_id();
                let seq: i64 = db.query_row(
                    "SELECT COALESCE(MAX(seq),0)+1 FROM work_item WHERE thread_id=?1",
                    [thread_id],
                    |r| r.get(0),
                )?;
                db.execute(
                    "INSERT INTO work_item (id, thread_id, turn_id, seq, kind, status, title, text, data, provider_item_id, created_at, updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
                    params![
                        id,
                        thread_id,
                        turn_id,
                        seq,
                        kind.as_str(),
                        status.as_str(),
                        title,
                        text,
                        if data.is_empty() { "{}" } else { data },
                        provider_item_id,
                        now
                    ],
                )?;
                id
            }
        };
        let item = db.query_row(&format!("{ITEM_SELECT} WHERE id=?1"), [&id], item_row)?;
        Ok(item)
    }

    /// Append streamed text. Returns our item id.
    pub fn append_item_text(&self, thread_id: &str, provider_item_id: &str, delta: &str) -> Result<Option<String>> {
        let db = self.db();
        let id: Option<String> = db
            .query_row(
                "SELECT id FROM work_item WHERE thread_id=?1 AND provider_item_id=?2",
                params![thread_id, provider_item_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = &id {
            db.execute(
                "UPDATE work_item SET text = text || ?2, updated_at=?3 WHERE id=?1",
                params![id, delta, now_ms()],
            )?;
        }
        Ok(id)
    }

    pub fn item_by_provider_id(&self, thread_id: &str, provider_item_id: &str) -> Result<Option<Item>> {
        let db = self.db();
        Ok(db
            .query_row(
                &format!("{ITEM_SELECT} WHERE thread_id=?1 AND provider_item_id=?2"),
                params![thread_id, provider_item_id],
                item_row,
            )
            .optional()?)
    }

    // ---------------------------------------------------------------- approvals

    pub fn insert_approval(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
        kind: ApprovalKind,
        title: &str,
        detail: &str,
        provider_request_id: &str,
    ) -> Result<Approval> {
        let a = Approval {
            id: new_id(),
            thread_id: thread_id.to_string(),
            turn_id: turn_id.map(String::from),
            kind,
            state: ApprovalState::Pending,
            title: title.to_string(),
            detail: detail.to_string(),
            created_at: now_ms(),
            decided_at: None,
        };
        self.db().execute(
            "INSERT INTO work_approval (id, thread_id, turn_id, kind, state, title, detail, provider_request_id, created_at)
             VALUES (?1,?2,?3,?4,'pending',?5,?6,?7,?8)",
            params![a.id, a.thread_id, a.turn_id, kind.as_str(), a.title, a.detail, provider_request_id, a.created_at],
        )?;
        Ok(a)
    }

    /// The approval and the provider's request id (to answer the CLI).
    pub fn approval(&self, id: &str) -> Result<(Approval, String)> {
        let db = self.db();
        db.query_row(
            &format!("SELECT {APPROVAL_COLS}, provider_request_id FROM work_approval WHERE id=?1 OR id LIKE ?1 || '%' LIMIT 1"),
            [id.trim()],
            |r| Ok((approval_row(r)?, r.get(9)?)),
        )
        .optional()?
        .ok_or_else(|| WorkError::NotFound(format!("approval '{id}'")))
    }

    pub fn decide_approval(&self, id: &str, state: ApprovalState) -> Result<Approval> {
        let db = self.db();
        db.execute(
            "UPDATE work_approval SET state=?2, decided_at=?3 WHERE id=?1",
            params![id, state.as_str(), now_ms()],
        )?;
        Ok(db.query_row(&format!("{APPROVAL_SELECT} WHERE id=?1"), [id], approval_row)?)
    }

    /// Expire every open approval of a thread (session ended). Returns the expired ones.
    pub fn expire_approvals(&self, thread_id: &str) -> Result<Vec<Approval>> {
        let db = self.db();
        let ids: Vec<String> = db
            .prepare("SELECT id FROM work_approval WHERE thread_id=?1 AND state='pending'")?
            .query_map([thread_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(db);
        ids.iter().map(|id| self.decide_approval(id, ApprovalState::Expired)).collect()
    }

    pub fn pending_approvals(&self, thread_id: Option<&str>) -> Result<Vec<Approval>> {
        let db = self.db();
        let rows = db
            .prepare(&format!(
                "{APPROVAL_SELECT} WHERE state='pending' AND (?1 IS NULL OR thread_id=?1) ORDER BY created_at"
            ))?
            .query_map(params![thread_id], approval_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

const THREAD_SELECT: &str = "SELECT t.id, t.project_id, t.title, t.provider, t.model, t.runtime_mode, t.interaction_mode,
    t.status, t.preview, t.last_error, t.origin, t.created_at, t.updated_at, t.provider_session_id, p.folder,
    (SELECT COUNT(*) FROM work_approval a WHERE a.thread_id=t.id AND a.state='pending')
    FROM work_thread t JOIN work_project p ON p.id=t.project_id";

fn thread_row(r: &Row<'_>) -> rusqlite::Result<ThreadRow> {
    Ok(ThreadRow {
        summary: ThreadSummary {
            id: r.get(0)?,
            project_id: r.get(1)?,
            title: r.get(2)?,
            provider: Provider::parse(&r.get::<_, String>(3)?).unwrap_or_default(),
            model: r.get(4)?,
            runtime_mode: RuntimeMode::parse(&r.get::<_, String>(5)?).unwrap_or_default(),
            interaction_mode: InteractionMode::parse(&r.get::<_, String>(6)?).unwrap_or_default(),
            status: ThreadStatus::parse(&r.get::<_, String>(7)?).unwrap_or_default(),
            preview: r.get(8)?,
            last_error: r.get(9)?,
            origin: r.get(10)?,
            created_at: r.get(11)?,
            updated_at: r.get(12)?,
            pending_approvals: r.get::<_, i64>(15)? as u32,
        },
        provider_session_id: r.get(13)?,
        folder: r.get(14)?,
    })
}

fn project_row(r: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project { id: r.get(0)?, name: r.get(1)?, folder: r.get(2)?, source: r.get(3)?, created_at: r.get(4)? })
}

const ITEM_SELECT: &str =
    "SELECT id, thread_id, turn_id, seq, kind, status, title, text, data, created_at, updated_at FROM work_item";

fn item_row(r: &Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        turn_id: r.get(2)?,
        seq: r.get(3)?,
        kind: ItemKind::parse(&r.get::<_, String>(4)?),
        status: ItemStatus::parse(&r.get::<_, String>(5)?),
        title: r.get(6)?,
        text: r.get(7)?,
        data: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

const APPROVAL_COLS: &str = "id, thread_id, turn_id, kind, state, title, detail, created_at, decided_at";
const APPROVAL_SELECT: &str =
    "SELECT id, thread_id, turn_id, kind, state, title, detail, created_at, decided_at FROM work_approval";

fn approval_row(r: &Row<'_>) -> rusqlite::Result<Approval> {
    Ok(Approval {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        turn_id: r.get(2)?,
        kind: ApprovalKind::parse(&r.get::<_, String>(3)?),
        state: ApprovalState::parse(&r.get::<_, String>(4)?),
        title: r.get(5)?,
        detail: r.get(6)?,
        created_at: r.get(7)?,
        decided_at: r.get(8)?,
    })
}

/// Trim trailing separators so `C:\x\` and `C:\x` are the same project.
pub fn normalize_folder(folder: &str) -> String {
    let f = folder.trim().trim_matches('"');
    let trimmed = f.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed.ends_with(':') { f.to_string() } else { trimmed.to_string() }
}

pub fn folder_name(folder: &str) -> String {
    folder.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(folder).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(store: &Store, project: &Project) -> ThreadSummary {
        let now = now_ms();
        let t = ThreadSummary {
            id: new_id(),
            project_id: project.id.clone(),
            title: "Reply PONG".into(),
            provider: Provider::Claude,
            model: "haiku".into(),
            runtime_mode: RuntimeMode::Supervised,
            interaction_mode: InteractionMode::Default,
            status: ThreadStatus::Idle,
            pending_approvals: 0,
            preview: String::new(),
            last_error: None,
            origin: None,
            created_at: now,
            updated_at: now,
        };
        store.insert_thread(&t).unwrap();
        t
    }

    #[test]
    fn project_upsert_dedupes_by_folder() {
        let s = Store::open_in_memory().unwrap();
        let (a, new_a) = s.upsert_project("", "/tmp/scratch/", "manual", None).unwrap();
        let (b, new_b) = s.upsert_project("Other", "/tmp/scratch", "t3", None).unwrap();
        assert!(new_a && !new_b);
        assert_eq!(a.id, b.id);
        assert_eq!(a.name, "scratch");
        assert_eq!(s.resolve_project("SCRATCH").unwrap().id, a.id);
    }

    #[test]
    fn items_stream_and_finish() {
        let s = Store::open_in_memory().unwrap();
        let (p, _) = s.upsert_project("p", "/x", "manual", None).unwrap();
        let t = thread(&s, &p);
        let turn = s.start_turn(&t.id).unwrap();
        assert_eq!(turn.seq, 1);
        let item = s
            .upsert_item(&t.id, Some(&turn.id), Some("m1"), ItemKind::AssistantMessage, ItemStatus::Running, "", "", "")
            .unwrap();
        s.append_item_text(&t.id, "m1", "PO").unwrap();
        s.append_item_text(&t.id, "m1", "NG").unwrap();
        let again = s.item_by_provider_id(&t.id, "m1").unwrap().unwrap();
        assert_eq!(again.id, item.id);
        assert_eq!(again.text, "PONG");
        let done = s.finish_turn(&turn.id, TurnStatus::Completed, None).unwrap();
        assert_eq!(done.status, TurnStatus::Completed);
        let d = s.detail(&t.id).unwrap();
        assert_eq!(d.items.len(), 1);
        assert_eq!(d.items[0].status, ItemStatus::Done);
    }

    #[test]
    fn approvals_count_and_expire() {
        let s = Store::open_in_memory().unwrap();
        let (p, _) = s.upsert_project("p", "/x", "manual", None).unwrap();
        let t = thread(&s, &p);
        let a = s.insert_approval(&t.id, None, ApprovalKind::Command, "Run ls", "ls", "req-1").unwrap();
        assert_eq!(s.thread(&t.id).unwrap().summary.pending_approvals, 1);
        let (got, req) = s.approval(&a.id).unwrap();
        assert_eq!(req, "req-1");
        assert_eq!(got.state, ApprovalState::Pending);
        let expired = s.expire_approvals(&t.id).unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(s.thread(&t.id).unwrap().summary.pending_approvals, 0);
    }

    #[test]
    fn restart_recovery() {
        let s = Store::open_in_memory().unwrap();
        let (p, _) = s.upsert_project("p", "/x", "manual", None).unwrap();
        let t = thread(&s, &p);
        s.set_thread_status(&t.id, ThreadStatus::Working, None).unwrap();
        s.start_turn(&t.id).unwrap();
        assert_eq!(s.recover_after_restart().unwrap(), 1);
        let d = s.detail(&t.id).unwrap();
        assert_eq!(d.thread.status, ThreadStatus::Idle);
        assert_eq!(d.turns[0].status, TurnStatus::Interrupted);
    }
}
