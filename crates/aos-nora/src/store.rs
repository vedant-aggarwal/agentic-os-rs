//! Nora's SQLite tables, in the main Agentic OS database with a `nora_` prefix (like `work_`).
//!
//! One connection behind a mutex; every call is short and never held across an `.await`.
//! Times are Unix milliseconds.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS nora_msg (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    engine      TEXT NOT NULL,              -- user | claude | grok | system
    source      TEXT NOT NULL DEFAULT 'app',-- app | grok | cli | mcp | heartbeat | scheduler ...
    kind        TEXT NOT NULL DEFAULT 'message', -- message | ack | progress | final | note | alarm | error
    text        TEXT NOT NULL,
    at          INTEGER NOT NULL,
    turn_id     TEXT,
    external_id TEXT UNIQUE                 -- e.g. grok:<chat>:<entry id>, for dedupe
);
CREATE INDEX IF NOT EXISTS nora_msg_at ON nora_msg(at, id);

CREATE TABLE IF NOT EXISTS nora_turn (
    id          TEXT PRIMARY KEY,
    engine      TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'chat',  -- chat | heartbeat | consolidation
    status      TEXT NOT NULL,                 -- running | done | failed
    upto_msg    INTEGER NOT NULL DEFAULT 0,    -- newest message the turn saw
    summary     TEXT NOT NULL DEFAULT '',
    tools       TEXT NOT NULL DEFAULT '[]',
    error       TEXT,
    cost_usd    REAL,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER
);

CREATE TABLE IF NOT EXISTS nora_grok_outbox (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL,                 -- summary | ask | message
    text        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'queued',-- queued | sent | failed
    error       TEXT,
    created_at  INTEGER NOT NULL,
    sent_at     INTEGER
);

CREATE TABLE IF NOT EXISTS nora_memory (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    text        TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'fact',
    importance  INTEGER NOT NULL DEFAULT 3,
    status      TEXT NOT NULL DEFAULT 'active',
    source      TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    last_used   INTEGER
);

CREATE TABLE IF NOT EXISTS nora_short (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    text        TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'now',
    status      TEXT NOT NULL DEFAULT 'active',
    source      TEXT,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS nora_episode (
    day         TEXT PRIMARY KEY,
    summary     TEXT NOT NULL,
    at          INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS nora_heartbeat (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    at          INTEGER NOT NULL,
    decision    TEXT NOT NULL,                 -- skip | none | note | message | alarm | call
    reason      TEXT NOT NULL,
    text        TEXT NOT NULL DEFAULT '',
    model       TEXT NOT NULL DEFAULT 'rules',
    sig         TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS nora_call (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    at          INTEGER NOT NULL,
    reason      TEXT NOT NULL,
    source      TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'ringing', -- ringing | answered | missed
    updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS nora_kv (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL
);
"#;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Msg {
    pub id: i64,
    pub engine: String,
    pub source: String,
    pub kind: String,
    pub text: String,
    pub at: i64,
    pub turn_id: Option<String>,
    pub external_id: Option<String>,
}

/// A message to append.
#[derive(Debug, Clone, Default)]
pub struct NewMsg {
    pub engine: String,
    pub source: String,
    pub kind: String,
    pub text: String,
    pub at: Option<i64>,
    pub turn_id: Option<String>,
    pub external_id: Option<String>,
}

impl NewMsg {
    pub fn new(engine: &str, source: &str, kind: &str, text: impl Into<String>) -> Self {
        NewMsg { engine: engine.into(), source: source.into(), kind: kind.into(), text: text.into(), ..Default::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnRow {
    pub id: String,
    pub engine: String,
    pub kind: String,
    pub status: String,
    pub upto_msg: i64,
    pub summary: String,
    pub tools: Vec<String>,
    pub error: Option<String>,
    pub cost_usd: Option<f64>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboxItem {
    pub id: i64,
    pub kind: String,
    pub text: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub id: i64,
    pub at: i64,
    pub decision: String,
    pub reason: String,
    pub text: String,
    pub model: String,
    pub sig: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub id: i64,
    pub at: i64,
    pub reason: String,
    pub source: String,
    pub status: String,
}

const MSG_COLS: &str = "id, engine, source, kind, text, at, turn_id, external_id";

fn msg_row(r: &Row<'_>) -> rusqlite::Result<Msg> {
    Ok(Msg {
        id: r.get(0)?,
        engine: r.get(1)?,
        source: r.get(2)?,
        kind: r.get(3)?,
        text: r.get(4)?,
        at: r.get(5)?,
        turn_id: r.get(6)?,
        external_id: r.get(7)?,
    })
}

const TURN_COLS: &str = "id, engine, kind, status, upto_msg, summary, tools, error, cost_usd, started_at, ended_at";

fn turn_row(r: &Row<'_>) -> rusqlite::Result<TurnRow> {
    let tools: String = r.get(6)?;
    Ok(TurnRow {
        id: r.get(0)?,
        engine: r.get(1)?,
        kind: r.get(2)?,
        status: r.get(3)?,
        upto_msg: r.get(4)?,
        summary: r.get(5)?,
        tools: serde_json::from_str(&tools).unwrap_or_default(),
        error: r.get(7)?,
        cost_usd: r.get(8)?,
        started_at: r.get(9)?,
        ended_at: r.get(10)?,
    })
}

pub struct NoraStore {
    conn: Mutex<Connection>,
}

impl NoraStore {
    pub fn open(path: &Path) -> Result<NoraStore> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(NoraStore { conn: Mutex::new(conn) })
    }

    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // --- thread ---------------------------------------------------------------------------

    /// Append a message. Returns `None` when `external_id` was already stored (dedupe).
    pub fn append(&self, m: &NewMsg) -> Result<Option<Msg>> {
        let c = self.conn();
        let at = m.at.unwrap_or_else(now_ms);
        let n = c.execute(
            "INSERT OR IGNORE INTO nora_msg (engine, source, kind, text, at, turn_id, external_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![m.engine, m.source, m.kind, m.text, at, m.turn_id, m.external_id],
        )?;
        if n == 0 {
            return Ok(None);
        }
        let id = c.last_insert_rowid();
        Ok(Some(c.query_row(&format!("SELECT {MSG_COLS} FROM nora_msg WHERE id = ?1"), [id], msg_row)?))
    }

    pub fn set_msg_kind(&self, id: i64, kind: &str) -> Result<Option<Msg>> {
        let c = self.conn();
        c.execute("UPDATE nora_msg SET kind = ?2 WHERE id = ?1", params![id, kind])?;
        Ok(c.query_row(&format!("SELECT {MSG_COLS} FROM nora_msg WHERE id = ?1"), [id], msg_row).optional()?)
    }

    pub fn has_external(&self, external_id: &str) -> Result<bool> {
        let c = self.conn();
        Ok(c.query_row("SELECT 1 FROM nora_msg WHERE external_id = ?1", [external_id], |_| Ok(())).optional()?.is_some())
    }

    /// The newest `limit` messages (optionally before message `before`), oldest first.
    pub fn recent(&self, limit: usize, before: Option<i64>) -> Result<Vec<Msg>> {
        let c = self.conn();
        let mut stmt = c.prepare(&format!(
            "SELECT {MSG_COLS} FROM nora_msg WHERE (?1 IS NULL OR id < ?1) ORDER BY at DESC, id DESC LIMIT ?2"
        ))?;
        let mut rows = stmt.query_map(params![before, limit as i64], msg_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Messages in a time range, or matching words, oldest first (for `thread_detail`).
    pub fn search(&self, query: Option<&str>, from: Option<i64>, to: Option<i64>, limit: usize) -> Result<Vec<Msg>> {
        let c = self.conn();
        let like = query.map(|q| format!("%{}%", q.trim()));
        let mut stmt = c.prepare(&format!(
            "SELECT {MSG_COLS} FROM nora_msg WHERE (?1 IS NULL OR text LIKE ?1) AND (?2 IS NULL OR at >= ?2) AND (?3 IS NULL OR at <= ?3)
             ORDER BY at DESC, id DESC LIMIT ?4"
        ))?;
        let mut rows = stmt.query_map(params![like, from, to, limit as i64], msg_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub fn msg(&self, id: i64) -> Result<Option<Msg>> {
        let c = self.conn();
        Ok(c.query_row(&format!("SELECT {MSG_COLS} FROM nora_msg WHERE id = ?1"), [id], msg_row).optional()?)
    }

    /// His messages for Claude after message `after`, oldest first. Messages he typed in Grok Bot
    /// (Grok answers those) and ones he sent to Grok (`to_grok`) are left out.
    pub fn user_msgs_after(&self, after: i64) -> Result<Vec<Msg>> {
        let c = self.conn();
        let mut stmt = c.prepare(&format!(
            "SELECT {MSG_COLS} FROM nora_msg WHERE id > ?1 AND engine = 'user' AND kind = 'message' AND source != 'grok' ORDER BY id"
        ))?;
        Ok(stmt.query_map([after], msg_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn latest_msg_id(&self) -> Result<i64> {
        Ok(self.conn().query_row("SELECT COALESCE(MAX(id), 0) FROM nora_msg", [], |r| r.get(0))?)
    }

    pub fn msgs_between(&self, from: i64, to: i64) -> Result<Vec<Msg>> {
        self.search(None, Some(from), Some(to), 2000)
    }

    // --- turns ----------------------------------------------------------------------------

    pub fn start_turn(&self, id: &str, engine: &str, kind: &str, upto_msg: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO nora_turn (id, engine, kind, status, upto_msg, started_at) VALUES (?1, ?2, ?3, 'running', ?4, ?5)",
            params![id, engine, kind, upto_msg, now_ms()],
        )?;
        Ok(())
    }

    pub fn end_turn(&self, id: &str, status: &str, summary: &str, tools: &[String], error: Option<&str>, cost: Option<f64>) -> Result<Option<TurnRow>> {
        let c = self.conn();
        c.execute(
            "UPDATE nora_turn SET status = ?2, summary = ?3, tools = ?4, error = ?5, cost_usd = ?6, ended_at = ?7 WHERE id = ?1",
            params![id, status, summary, serde_json::to_string(tools)?, error, cost, now_ms()],
        )?;
        Ok(c.query_row(&format!("SELECT {TURN_COLS} FROM nora_turn WHERE id = ?1"), [id], turn_row).optional()?)
    }

    /// The newest message any chat turn has already answered.
    pub fn last_answered(&self) -> Result<i64> {
        Ok(self.conn().query_row(
            "SELECT COALESCE(MAX(upto_msg), 0) FROM nora_turn WHERE kind = 'chat' AND engine = 'claude'",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn turns(&self, limit: usize) -> Result<Vec<TurnRow>> {
        let c = self.conn();
        let mut stmt = c.prepare(&format!("SELECT {TURN_COLS} FROM nora_turn ORDER BY started_at DESC LIMIT ?1"))?;
        Ok(stmt.query_map([limit as i64], turn_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Turns left `running` by a crash or restart.
    pub fn fail_running_turns(&self) -> Result<usize> {
        Ok(self.conn().execute(
            "UPDATE nora_turn SET status = 'failed', error = 'server restarted', ended_at = ?1 WHERE status = 'running'",
            [now_ms()],
        )?)
    }

    // --- Grok outbox ----------------------------------------------------------------------

    pub fn queue_grok(&self, kind: &str, text: &str) -> Result<OutboxItem> {
        let c = self.conn();
        let at = now_ms();
        c.execute("INSERT INTO nora_grok_outbox (kind, text, created_at) VALUES (?1, ?2, ?3)", params![kind, text, at])?;
        Ok(OutboxItem { id: c.last_insert_rowid(), kind: kind.into(), text: text.into(), status: "queued".into(), created_at: at })
    }

    pub fn grok_queue(&self, limit: usize) -> Result<Vec<OutboxItem>> {
        let c = self.conn();
        let mut stmt = c.prepare(
            "SELECT id, kind, text, status, created_at FROM nora_grok_outbox WHERE status = 'queued' ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(OutboxItem { id: r.get(0)?, kind: r.get(1)?, text: r.get(2)?, status: r.get(3)?, created_at: r.get(4)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn mark_grok(&self, id: i64, status: &str, error: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE nora_grok_outbox SET status = ?2, error = ?3, sent_at = ?4 WHERE id = ?1",
            params![id, status, error, now_ms()],
        )?;
        Ok(())
    }

    // --- heartbeat and calls --------------------------------------------------------------

    pub fn add_heartbeat(&self, decision: &str, reason: &str, text: &str, model: &str, sig: &str) -> Result<Heartbeat> {
        let c = self.conn();
        let at = now_ms();
        c.execute(
            "INSERT INTO nora_heartbeat (at, decision, reason, text, model, sig) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![at, decision, reason, text, model, sig],
        )?;
        Ok(Heartbeat { id: c.last_insert_rowid(), at, decision: decision.into(), reason: reason.into(), text: text.into(), model: model.into(), sig: sig.into() })
    }

    pub fn heartbeats(&self, limit: usize) -> Result<Vec<Heartbeat>> {
        let c = self.conn();
        let mut stmt = c.prepare("SELECT id, at, decision, reason, text, model, sig FROM nora_heartbeat ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(Heartbeat { id: r.get(0)?, at: r.get(1)?, decision: r.get(2)?, reason: r.get(3)?, text: r.get(4)?, model: r.get(5)?, sig: r.get(6)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The signature of the last heartbeat that actually asked the model.
    pub fn last_model_sig(&self) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT sig FROM nora_heartbeat WHERE model != 'rules' ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .optional()?)
    }

    pub fn add_call(&self, reason: &str, source: &str) -> Result<Call> {
        let c = self.conn();
        let at = now_ms();
        c.execute(
            "INSERT INTO nora_call (at, reason, source, status, updated_at) VALUES (?1, ?2, ?3, 'ringing', ?1)",
            params![at, reason, source],
        )?;
        Ok(Call { id: c.last_insert_rowid(), at, reason: reason.into(), source: source.into(), status: "ringing".into() })
    }

    /// Calls still ringing that started before `cutoff`; marks them missed (ring once).
    pub fn take_missed_calls(&self, cutoff: i64) -> Result<Vec<Call>> {
        let c = self.conn();
        let mut stmt = c.prepare("SELECT id, at, reason, source, status FROM nora_call WHERE status = 'ringing' AND at < ?1")?;
        let calls = stmt
            .query_map([cutoff], |r| Ok(Call { id: r.get(0)?, at: r.get(1)?, reason: r.get(2)?, source: r.get(3)?, status: "missed".into() }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        c.execute("UPDATE nora_call SET status = 'missed', updated_at = ?2 WHERE status = 'ringing' AND at < ?1", params![cutoff, now_ms()])?;
        Ok(calls)
    }

    pub fn ringing_call(&self) -> Result<Option<Call>> {
        Ok(self
            .conn()
            .query_row("SELECT id, at, reason, source, status FROM nora_call WHERE status = 'ringing' LIMIT 1", [], |r| {
                Ok(Call { id: r.get(0)?, at: r.get(1)?, reason: r.get(2)?, source: r.get(3)?, status: r.get(4)? })
            })
            .optional()?)
    }

    // --- key/value (settings and cursors) -------------------------------------------------

    pub fn get_kv(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn().query_row("SELECT value FROM nora_kv WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_kv(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO nora_kv (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn temp_store() -> (NoraStore, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("aos-nora-test-{}.db", aos_core::ids::new_id()));
    (NoraStore::open(&path).unwrap(), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_dedupes_by_external_id() {
        let (s, _p) = temp_store();
        let mut m = NewMsg::new("grok", "grok", "message", "hi");
        m.external_id = Some("grok:x:1".into());
        assert!(s.append(&m).unwrap().is_some());
        assert!(s.append(&m).unwrap().is_none());
        s.append(&NewMsg::new("user", "app", "message", "yo")).unwrap();
        let recent = s.recent(10, None).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1].text, "yo");
        assert_eq!(s.user_msgs_after(0).unwrap().len(), 1);
    }

    #[test]
    fn missed_calls_ring_once() {
        let (s, _p) = temp_store();
        s.add_call("check in", "heartbeat").unwrap();
        assert!(s.ringing_call().unwrap().is_some());
        let missed = s.take_missed_calls(now_ms() + 1).unwrap();
        assert_eq!(missed.len(), 1);
        assert!(s.take_missed_calls(now_ms() + 1).unwrap().is_empty());
    }
}
