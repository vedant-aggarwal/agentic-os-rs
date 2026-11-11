//! Alarms and reminders. Phase A stores them; the scheduler that rings them comes in phase C.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};

use crate::error::{Result, invalid, not_found};
use crate::model::{Alarm, AlarmKind, AlarmStatus};
use crate::store::{ChangeOp, Store, resolve_prefix};
use crate::{ids, time};

const COLS: &str = "id, at, label, kind, status, task_id";

fn alarm_from_row(r: &Row<'_>) -> rusqlite::Result<Alarm> {
    Ok(Alarm {
        id: r.get(0)?,
        at: r.get(1)?,
        label: r.get(2)?,
        kind: r.get(3)?,
        status: r.get(4)?,
        task_id: r.get(5)?,
    })
}

impl Store {
    pub fn set_alarm(
        &self,
        by: &str,
        at: DateTime<Utc>,
        label: &str,
        kind: AlarmKind,
        task_id: Option<String>,
    ) -> Result<Alarm> {
        let label = label.trim();
        if label.is_empty() {
            return Err(invalid("alarm label is empty"));
        }
        self.write(by, |tx| {
            let id = ids::new_id();
            tx.execute(
                "INSERT INTO alarm (id, at, label, kind, status, task_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'scheduled', ?5, ?6, ?6)",
                params![id, time::ts(at), label, kind, task_id, tx.now()],
            )?;
            tx.record("alarm", &id, ChangeOp::Upsert)?;
            Ok(tx.query_row(
                &format!("SELECT {COLS} FROM alarm WHERE id = ?1"),
                [&id],
                alarm_from_row,
            )?)
        })
    }

    /// Scheduled alarms from `now` on (or every alarm when `all`), soonest first.
    pub fn list_alarms(&self, now: DateTime<Utc>, all: bool) -> Result<Vec<Alarm>> {
        self.read(|c| {
            let sql = if all {
                format!("SELECT {COLS} FROM alarm ORDER BY at DESC LIMIT 200")
            } else {
                format!(
                    "SELECT {COLS} FROM alarm WHERE status = 'scheduled' AND at >= ?1 ORDER BY at"
                )
            };
            let mut stmt = c.prepare(&sql)?;
            let rows = if all {
                stmt.query_map([], alarm_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            } else {
                stmt.query_map([time::ts(now)], alarm_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            Ok(rows)
        })
    }

    /// Scheduled alarms whose time has come (for the scheduler).
    pub fn due_alarms(&self, now: DateTime<Utc>) -> Result<Vec<Alarm>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLS} FROM alarm WHERE status = 'scheduled' AND at <= ?1 ORDER BY at"
            ))?;
            let rows = stmt.query_map([time::ts(now)], alarm_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn cancel_alarm(&self, by: &str, key: &str) -> Result<Alarm> {
        self.set_alarm_status(by, key, AlarmStatus::Cancelled)
    }

    pub fn mark_alarm_fired(&self, by: &str, id: &str) -> Result<Alarm> {
        self.set_alarm_status(by, id, AlarmStatus::Fired)
    }

    fn set_alarm_status(&self, by: &str, key: &str, status: AlarmStatus) -> Result<Alarm> {
        self.write(by, |tx| {
            let id = resolve_prefix(tx, "alarm", key)?;
            tx.execute(
                "UPDATE alarm SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, status, tx.now()],
            )?;
            tx.record("alarm", &id, ChangeOp::Upsert)?;
            tx.query_row(
                &format!("SELECT {COLS} FROM alarm WHERE id = ?1"),
                [&id],
                alarm_from_row,
            )
            .optional()?
            .ok_or_else(|| not_found("alarm", key))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn set_list_cancel() {
        let store = Store::open_temporary().unwrap();
        let now = time::now();
        let a = store
            .set_alarm(
                "t",
                now + Duration::minutes(10),
                "Gym",
                AlarmKind::Alarm,
                None,
            )
            .unwrap();
        store
            .set_alarm(
                "t",
                now - Duration::minutes(10),
                "Old",
                AlarmKind::Reminder,
                None,
            )
            .unwrap();
        let upcoming = store.list_alarms(now, false).unwrap();
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].label, "Gym");
        assert_eq!(store.due_alarms(now).unwrap().len(), 1);

        let c = store.cancel_alarm("t", &a.id[..5]).unwrap();
        assert_eq!(c.status, AlarmStatus::Cancelled);
        assert!(store.list_alarms(now, false).unwrap().is_empty());
        assert_eq!(store.list_alarms(now, true).unwrap().len(), 2);
    }
}
