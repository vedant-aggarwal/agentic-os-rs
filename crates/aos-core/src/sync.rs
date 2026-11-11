//! Incremental sync over `change_log`.
//!
//! Pull: a client sends the last version it has; it gets every row changed after that, each
//! with the row's current contents, plus the newest version to send next time.
//!
//! Push: a client's outbox is a list of tool calls, each with its own `op_id`. The server
//! applies them through the tool registry and remembers the result per `op_id`
//! ([`Store::applied_op`] / [`Store::record_applied_op`]), so resending is safe.

use std::collections::HashMap;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Result, invalid};
use crate::store::Store;
use crate::time;

/// Tables that clients mirror. Auth tables (`device`, `pairing_code`) never leave the server.
pub const SYNCED_TABLES: &[&str] = &[
    "pillar",
    "area",
    "project",
    "task",
    "habit",
    "habit_log",
    "xp_event",
    "mood",
    "journal_entry",
    "alarm",
    "goal",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub version: i64,
    pub entity: String,
    pub id: String,
    /// `upsert` or `delete`.
    pub op: String,
    pub at: String,
    /// Who made the change, so a client can skip echoes of its own writes.
    pub actor: Option<String>,
    /// The row as it is now; `None` when it was deleted.
    pub row: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeBatch {
    pub changes: Vec<Change>,
    /// Send this as `since` next time.
    pub latest: i64,
    /// More changes are waiting; ask again right away.
    pub more: bool,
}

/// A bare change notice (no row), for the live event bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeNotice {
    pub version: i64,
    pub entity: String,
    pub id: String,
    pub op: String,
    pub actor: Option<String>,
}

impl Store {
    /// The newest `change_log` version (0 when empty).
    pub fn latest_version(&self) -> Result<i64> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM change_log",
                [],
                |r| r.get(0),
            )?)
        })
    }

    /// Change notices after `since`, oldest first, at most `limit`.
    pub fn change_notices(&self, since: i64, limit: usize) -> Result<Vec<ChangeNotice>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT version, entity, row_id, op, actor FROM change_log
                 WHERE version > ?1 ORDER BY version LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![since, limit as i64], |r| {
                Ok(ChangeNotice {
                    version: r.get(0)?,
                    entity: r.get(1)?,
                    id: r.get(2)?,
                    op: r.get(3)?,
                    actor: r.get(4)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Rows changed after `since`. Several changes to one row collapse into its latest version.
    pub fn changes_since(&self, since: i64, limit: usize) -> Result<ChangeBatch> {
        let limit = limit.clamp(1, 5000);
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT version, entity, row_id, op, at, actor FROM change_log
                 WHERE version > ?1 ORDER BY version LIMIT ?2",
            )?;
            let raw = stmt
                .query_map(params![since, (limit + 1) as i64], |r| {
                    Ok(Change {
                        version: r.get(0)?,
                        entity: r.get(1)?,
                        id: r.get(2)?,
                        op: r.get(3)?,
                        at: r.get(4)?,
                        actor: r.get(5)?,
                        row: None,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let more = raw.len() > limit;
            let raw: Vec<Change> = raw.into_iter().take(limit).collect();
            let latest = raw.last().map_or(since, |c| c.version);

            // Keep only the last change per row.
            let mut last: HashMap<(String, String), usize> = HashMap::new();
            for (i, ch) in raw.iter().enumerate() {
                last.insert((ch.entity.clone(), ch.id.clone()), i);
            }
            let mut changes = Vec::with_capacity(last.len());
            for (i, mut ch) in raw.into_iter().enumerate() {
                if last.get(&(ch.entity.clone(), ch.id.clone())) != Some(&i) {
                    continue;
                }
                if SYNCED_TABLES.contains(&ch.entity.as_str()) {
                    ch.row = row_json(c, &ch.entity, &ch.id)?;
                    if ch.row.is_none() {
                        ch.op = "delete".into();
                    }
                    changes.push(ch);
                }
            }
            Ok(ChangeBatch {
                changes,
                latest,
                more,
            })
        })
    }

    /// One synced row as JSON, or `None` when it doesn't exist.
    pub fn row_json(&self, entity: &str, id: &str) -> Result<Option<Value>> {
        self.read(|c| row_json(c, entity, id))
    }

    /// The stored result of an outbox op already applied.
    pub fn applied_op(&self, op_id: &str) -> Result<Option<Value>> {
        self.read(|c| {
            let raw: Option<String> = c
                .query_row(
                    "SELECT result FROM applied_op WHERE op_id = ?1",
                    [op_id],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(raw.map(|s| serde_json::from_str(&s)).transpose()?)
        })
    }

    pub fn record_applied_op(
        &self,
        op_id: &str,
        device_id: Option<&str>,
        tool: &str,
        result: &Value,
    ) -> Result<()> {
        if op_id.trim().is_empty() {
            return Err(invalid("op_id is empty"));
        }
        let by = device_id.unwrap_or("server");
        self.write(by, |tx| {
            tx.execute(
                "INSERT OR IGNORE INTO applied_op (op_id, device_id, tool, result, at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    op_id,
                    device_id,
                    tool,
                    result.to_string(),
                    time::ts(time::now())
                ],
            )?;
            Ok(())
        })
    }
}

fn row_json(c: &Connection, entity: &str, id: &str) -> Result<Option<Value>> {
    if !SYNCED_TABLES.contains(&entity) {
        return Err(invalid(format!("'{entity}' is not a synced table")));
    }
    // `entity` is checked against the allow-list above, so formatting it into SQL is safe.
    let mut stmt = c.prepare(&format!("SELECT * FROM {entity} WHERE id = ?1"))?;
    let names: Vec<String> = stmt.column_names().map(String::from).collect();
    let row = stmt
        .query_row([id], |r| {
            let mut obj = Map::new();
            for (i, name) in names.iter().enumerate() {
                let v = match r.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => Value::from(n),
                    ValueRef::Real(f) => Value::from(f),
                    ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
                    ValueRef::Blob(_) => continue,
                };
                obj.insert(name.clone(), v);
            }
            Ok(Value::Object(obj))
        })
        .optional()?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::NewTask;

    #[test]
    fn pull_collapses_and_pages() {
        let store = Store::open_temporary().unwrap();
        let start = store.latest_version().unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let t = store
            .add_task(
                "phone",
                NewTask {
                    title: "Sync me".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store.complete_task("phone", &t.id, None, day).unwrap();

        let batch = store.changes_since(start, 100).unwrap();
        let task_changes: Vec<_> = batch
            .changes
            .iter()
            .filter(|c| c.entity == "task")
            .collect();
        assert_eq!(task_changes.len(), 1, "two writes to one row collapse");
        let row = task_changes[0].row.as_ref().unwrap();
        assert_eq!(row["status"], "done");
        assert_eq!(task_changes[0].actor.as_deref(), Some("phone"));
        assert!(!batch.more);
        assert_eq!(batch.latest, store.latest_version().unwrap());

        let page = store.changes_since(start, 1).unwrap();
        assert!(page.more);
        assert_eq!(page.latest, start + 1);
        assert!(
            store
                .changes_since(batch.latest, 100)
                .unwrap()
                .changes
                .is_empty()
        );
    }

    #[test]
    fn applied_ops_are_remembered() {
        let store = Store::open_temporary().unwrap();
        assert!(store.applied_op("op1").unwrap().is_none());
        store
            .record_applied_op(
                "op1",
                Some("dev"),
                "task_add",
                &serde_json::json!({"ok": true}),
            )
            .unwrap();
        assert_eq!(store.applied_op("op1").unwrap().unwrap()["ok"], true);
        assert!(store.row_json("device", "x").is_err());
    }
}
