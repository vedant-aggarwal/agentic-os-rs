//! Read T3 Code's projects and threads (read only). T3 keeps its state in SQLite under
//! `~/.t3/userdata/`: `statev2.sqlite` (orchestration v2) or the older `state.sqlite`, table
//! `projection_projects`. Threads and their messages come from the v2 projection tables
//! (`orchestration_v2_projection_threads`, `..._messages`, `..._runs`); the older file has none,
//! so it lists no threads. Nothing here ever writes to T3's database.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct T3Project {
    pub id: String,
    pub title: String,
    pub folder: String,
}

/// A T3 Code thread, for the Work tab's read-only list. `project_id` is T3's id; the
/// supervisor maps it to the Agentic OS project.
#[derive(Debug, Clone, Serialize)]
pub struct T3Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    /// T3's provider instance (`claudeAgent`, `codex`, ...).
    pub provider: String,
    /// Status of the latest run (`running`, `completed`, `failed`, ...); empty before the first.
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One chat message of a T3 thread (user or assistant text only; tool calls are left out).
#[derive(Debug, Clone, Serialize)]
pub struct T3Message {
    pub role: String,
    pub text: String,
    pub created_at: i64,
}

fn open(path: &Path) -> Result<Connection> {
    // Read only, and never create the file. T3 may be running (WAL), which is fine for readers.
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    Ok(conn)
}

fn has_v2(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='orchestration_v2_projection_threads'",
        [],
        |_| Ok(()),
    )
    .is_ok()
}

/// ISO-8601 text column -> unix milliseconds, in SQL.
fn ms(col: &str) -> String {
    format!("CAST(ROUND((julianday({col}) - 2440587.5) * 86400000) AS INTEGER)")
}

fn thread_select() -> String {
    format!(
        "SELECT t.thread_id, t.project_id, t.title, COALESCE(t.provider_instance_id, t.default_provider, ''),
                COALESCE((SELECT r.status FROM orchestration_v2_projection_runs r
                          WHERE r.thread_id = t.thread_id ORDER BY r.ordinal DESC LIMIT 1), ''),
                {}, {}
         FROM orchestration_v2_projection_threads t",
        ms("t.created_at"),
        ms("t.updated_at")
    )
}

fn thread_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<T3Thread> {
    Ok(T3Thread {
        id: r.get(0)?,
        project_id: r.get(1)?,
        title: r.get(2)?,
        provider: r.get(3)?,
        status: r.get(4)?,
        created_at: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
        updated_at: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
    })
}

/// Live threads (not deleted, archived or subagents), newest first, optionally for one T3 project.
pub fn read_threads(path: &Path, project: Option<&str>, limit: usize) -> Result<Vec<T3Thread>> {
    let conn = open(path)?;
    if !has_v2(&conn) {
        return Ok(Vec::new());
    }
    let sql = format!(
        "{} WHERE t.deleted_at IS NULL AND t.archived_at IS NULL
           AND (?1 IS NULL OR t.project_id = ?1)
           AND COALESCE(json_extract(t.payload_json, '$.lineage.relationshipToParent'), '') <> 'subagent'
         ORDER BY t.updated_at DESC LIMIT ?2",
        thread_select()
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st
        .query_map(params![project, limit as i64], thread_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// One thread and its last `limit` messages, oldest first. Long messages are cut at 8000 chars.
pub fn read_thread(path: &Path, id: &str, limit: usize) -> Result<Option<(T3Thread, Vec<T3Message>)>> {
    let conn = open(path)?;
    if !has_v2(&conn) {
        return Ok(None);
    }
    let sql = format!("{} WHERE t.thread_id = ?1 AND t.deleted_at IS NULL", thread_select());
    let Some(thread) = conn.query_row(&sql, [id], thread_row).optional()? else { return Ok(None) };
    let sql = format!(
        "SELECT role, COALESCE(json_extract(payload_json, '$.text'), ''), {}
         FROM orchestration_v2_projection_messages
         WHERE thread_id = ?1 AND role IN ('user', 'assistant')
         ORDER BY created_at DESC, message_id DESC LIMIT ?2",
        ms("created_at")
    );
    let mut st = conn.prepare(&sql)?;
    let mut msgs = st
        .query_map(params![id, limit as i64], |r| {
            let text: String = r.get(1)?;
            let text = if text.chars().count() > 8000 { text.chars().take(8000).collect::<String>() + "…" } else { text };
            Ok(T3Message { role: r.get(0)?, text, created_at: r.get::<_, Option<i64>>(2)?.unwrap_or(0) })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    msgs.retain(|m| !m.text.trim().is_empty());
    msgs.reverse();
    Ok(Some((thread, msgs)))
}

pub fn read_projects(path: &Path) -> Result<Vec<T3Project>> {
    let conn = open(path)?;
    let v2 = "SELECT project_id, title, workspace_root FROM projection_projects
              WHERE deleted_at IS NULL AND archived_at IS NULL ORDER BY updated_at DESC";
    let v1 = "SELECT project_id, title, workspace_root FROM projection_projects
              WHERE deleted_at IS NULL ORDER BY updated_at DESC";
    let mut st = match conn.prepare(v2) {
        Ok(st) => st,
        Err(_) => conn.prepare(v1)?,
    };
    let rows = st
        .query_map([], |r| Ok(T3Project { id: r.get(0)?, title: r.get(1)?, folder: r.get(2)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_v1_shape() {
        let dir = std::env::temp_dir().join(format!("aos-t3-{}", crate::store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("state.sqlite");
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(
                "CREATE TABLE projection_projects (project_id TEXT, title TEXT, workspace_root TEXT,
                   created_at TEXT, updated_at TEXT, deleted_at TEXT);
                 INSERT INTO projection_projects VALUES ('p1', 'One', '/a', '1', '2', NULL);
                 INSERT INTO projection_projects VALUES ('p2', 'Gone', '/b', '1', '1', 'yes');",
            )
            .unwrap();
        }
        let ps = read_projects(&db).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].title, "One");
        // the old file has no v2 thread tables: no threads, no error
        assert!(read_threads(&db, None, 10).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_v2_threads_and_messages() {
        let dir = std::env::temp_dir().join(format!("aos-t3v2-{}", crate::store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("statev2.sqlite");
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(
                r#"CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT PRIMARY KEY, project_id TEXT, title TEXT,
                     default_provider TEXT, created_at TEXT, updated_at TEXT, archived_at TEXT, deleted_at TEXT,
                     payload_json TEXT, provider_instance_id TEXT);
                   CREATE TABLE orchestration_v2_projection_runs (run_id TEXT, thread_id TEXT, ordinal INTEGER, status TEXT);
                   CREATE TABLE orchestration_v2_projection_messages (message_id TEXT, thread_id TEXT, role TEXT,
                     created_at TEXT, payload_json TEXT);
                   INSERT INTO orchestration_v2_projection_threads VALUES
                     ('t1','p1','Fix it','claudeAgent','2026-11-10T10:00:00.000Z','2026-11-12T10:00:00.000Z',NULL,NULL,'{}',NULL),
                     ('t2','p1','Old','codex','2026-11-01T10:00:00.000Z','2026-11-02T10:00:00.000Z','x',NULL,'{}',NULL),
                     ('t3','p2','Sub','claudeAgent','2026-11-10T10:00:00.000Z','2026-11-11T10:00:00.000Z',NULL,NULL,
                      '{"lineage":{"relationshipToParent":"subagent"}}',NULL);
                   INSERT INTO orchestration_v2_projection_runs VALUES ('r1','t1',1,'completed'),('r2','t1',2,'running');
                   INSERT INTO orchestration_v2_projection_messages VALUES
                     ('m1','t1','user','2026-11-12T09:00:00.000Z','{"text":"hello"}'),
                     ('m2','t1','assistant','2026-11-12T09:00:01.000Z','{"text":"hi there"}'),
                     ('m3','t1','system','2026-11-12T09:00:02.000Z','{"text":"x"}');"#,
            )
            .unwrap();
        }
        let ts = read_threads(&db, None, 10).unwrap();
        assert_eq!(ts.len(), 1, "archived and subagent threads are hidden");
        assert_eq!(ts[0].status, "running");
        assert_eq!(ts[0].provider, "claudeAgent");
        assert_eq!(ts[0].updated_at, 1_794_477_600_000);
        assert!(read_threads(&db, Some("p2"), 10).unwrap().is_empty());
        let (t, msgs) = read_thread(&db, "t1", 50).unwrap().unwrap();
        assert_eq!(t.title, "Fix it");
        assert_eq!(msgs.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["hello", "hi there"]);
        assert!(read_thread(&db, "nope", 50).unwrap().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
