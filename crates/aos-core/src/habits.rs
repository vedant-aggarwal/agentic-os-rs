//! Habits and their daily logs. Streak rules live in [`crate::streak`].

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, invalid, not_found};
use crate::model::{Habit, HabitLog, HabitLogStatus, HabitView};
use crate::store::{ChangeOp, Store, Tx};
use crate::streak::{self, Cadence};
use crate::{ids, xp};

/// Input for [`Store::add_habit`].
#[derive(Debug, Clone)]
pub struct NewHabit {
    /// Optional fixed id (seed files use readable ids like `journal`).
    pub id: Option<String>,
    pub name: String,
    pub area_id: Option<String>,
    pub cue: Option<String>,
    pub normal: Option<String>,
    pub floor: Option<String>,
    pub cadence: Cadence,
    pub xp: i64,
    pub anchor: bool,
    pub start_date: NaiveDate,
}

/// A log result with the habit's streak after it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HabitLogResult {
    pub log: HabitLog,
    pub streak: streak::StreakInfo,
    pub xp_delta: i64,
}

const HABIT_COLS: &str =
    "id, area_id, name, cue, normal, floor, cadence, xp, anchor, archived, start_date, sort";

fn habit_from_row(r: &Row<'_>) -> rusqlite::Result<Habit> {
    let cadence: String = r.get("cadence")?;
    Ok(Habit {
        id: r.get("id")?,
        area_id: r.get("area_id")?,
        name: r.get("name")?,
        cue: r.get("cue")?,
        normal: r.get("normal")?,
        floor: r.get("floor")?,
        cadence: cadence.parse().unwrap_or(Cadence::Daily),
        xp: r.get("xp")?,
        anchor: r.get("anchor")?,
        archived: r.get("archived")?,
        start_date: r.get("start_date")?,
        sort: r.get("sort")?,
    })
}

fn get_habit(c: &Connection, id: &str) -> Result<Habit> {
    c.query_row(
        &format!("SELECT {HABIT_COLS} FROM habit WHERE id = ?1"),
        [id],
        habit_from_row,
    )
    .optional()?
    .ok_or_else(|| not_found("habit", id))
}

fn logs_for(c: &Connection, habit_id: &str) -> Result<BTreeMap<NaiveDate, HabitLogStatus>> {
    let mut stmt = c.prepare("SELECT date, status FROM habit_log WHERE habit_id = ?1")?;
    let rows = stmt.query_map([habit_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn view(c: &Connection, habit: Habit, day: NaiveDate) -> Result<HabitView> {
    let logs = logs_for(c, &habit.id)?;
    let streak = streak::compute(&habit.cadence, habit.start_date, day, &logs);
    Ok(HabitView {
        due_today: habit.cadence.is_due(day),
        today: logs.get(&day).copied(),
        streak,
        habit,
    })
}

pub(crate) fn upsert_habit(tx: &Tx<'_>, h: &NewHabit) -> Result<String> {
    let name = h.name.trim();
    if name.is_empty() {
        return Err(invalid("habit name is empty"));
    }
    let id = h.id.clone().unwrap_or_else(ids::new_id);
    tx.execute(
        "INSERT INTO habit (id, area_id, name, cue, normal, floor, cadence, xp, anchor, start_date,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
         ON CONFLICT(id) DO UPDATE SET area_id = excluded.area_id, name = excluded.name,
             cue = excluded.cue, normal = excluded.normal, floor = excluded.floor,
             cadence = excluded.cadence, xp = excluded.xp, anchor = excluded.anchor,
             updated_at = excluded.updated_at",
        params![
            id,
            h.area_id,
            name,
            h.cue,
            h.normal,
            h.floor,
            h.cadence.to_string(),
            h.xp,
            h.anchor,
            h.start_date,
            tx.now()
        ],
    )?;
    tx.record("habit", &id, ChangeOp::Upsert)?;
    Ok(id)
}

impl Store {
    pub fn add_habit(&self, by: &str, h: NewHabit) -> Result<Habit> {
        self.write(by, |tx| {
            let id = upsert_habit(tx, &h)?;
            get_habit(tx, &id)
        })
    }

    /// Find a habit by exact id, unique id prefix, or name (case-insensitive).
    pub fn resolve_habit(&self, key: &str) -> Result<Habit> {
        self.read(|c| {
            let by_name: Option<String> = c
                .query_row(
                    "SELECT id FROM habit WHERE lower(name) = lower(?1) AND archived = 0 LIMIT 1",
                    [key.trim()],
                    |r| r.get(0),
                )
                .optional()?;
            let id = match by_name {
                Some(id) => id,
                None => crate::store::resolve_prefix(c, "habit", key)?,
            };
            get_habit(c, &id)
        })
    }

    /// Active habits with their state on `day`.
    pub fn list_habits(&self, day: NaiveDate) -> Result<Vec<HabitView>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {HABIT_COLS} FROM habit WHERE archived = 0
                 ORDER BY anchor DESC, sort, name"
            ))?;
            let habits = stmt
                .query_map([], habit_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            habits.into_iter().map(|h| view(c, h, day)).collect()
        })
    }

    /// Record one day's result. Logging the same day again replaces it.
    pub fn log_habit(
        &self,
        by: &str,
        habit_id: &str,
        day: NaiveDate,
        status: HabitLogStatus,
        note: Option<String>,
        today: NaiveDate,
    ) -> Result<HabitLogResult> {
        if day > today {
            return Err(invalid("can't log a habit for a future day"));
        }
        self.write(by, |tx| {
            let habit = get_habit(tx, habit_id)?;
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM habit_log WHERE habit_id = ?1 AND date = ?2",
                    params![habit.id, day],
                    |r| r.get(0),
                )
                .optional()?;
            let id = existing.unwrap_or_else(ids::new_id);
            tx.execute(
                "INSERT INTO habit_log (id, habit_id, date, status, note, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                 ON CONFLICT(id) DO UPDATE SET status = excluded.status,
                     note = COALESCE(excluded.note, habit_log.note), updated_at = excluded.updated_at",
                params![id, habit.id, day, status, note, tx.now()],
            )?;
            tx.record("habit_log", &id, ChangeOp::Upsert)?;
            let xp_delta = xp::set_award(
                tx,
                "habit",
                &id,
                habit.area_id.as_deref(),
                xp::habit_xp(habit.xp, status),
                day,
            )?;
            let log = tx.query_row(
                "SELECT id, habit_id, date, status, note FROM habit_log WHERE id = ?1",
                [&id],
                |r| {
                    Ok(HabitLog {
                        id: r.get(0)?,
                        habit_id: r.get(1)?,
                        date: r.get(2)?,
                        status: r.get(3)?,
                        note: r.get(4)?,
                    })
                },
            )?;
            let streak = view(tx, habit, today)?.streak;
            Ok(HabitLogResult { log, streak, xp_delta })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streak::StreakState;
    use chrono::Duration;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn habit(store: &Store, start: NaiveDate) -> Habit {
        store
            .add_habit(
                "test",
                NewHabit {
                    id: None,
                    name: "Read".into(),
                    area_id: Some("reading".into()),
                    cue: Some("after dinner".into()),
                    normal: Some("20 pages".into()),
                    floor: Some("1 page".into()),
                    cadence: Cadence::Daily,
                    xp: 5,
                    anchor: false,
                    start_date: start,
                },
            )
            .unwrap()
    }

    #[test]
    fn logging_builds_streak_and_xp() {
        let store = Store::open_temporary().unwrap();
        let start = day("2026-11-01");
        let h = habit(&store, start);
        let today = day("2026-11-05");
        for i in 0..4 {
            let d = start + Duration::days(i);
            store
                .log_habit("test", &h.id, d, HabitLogStatus::Done, None, today)
                .unwrap();
        }
        let r = store
            .log_habit("test", &h.id, today, HabitLogStatus::Floor, None, today)
            .unwrap();
        assert_eq!(r.streak.current, 5);
        assert_eq!(r.xp_delta, 3);
        assert_eq!(store.xp_summary(today).unwrap().overall.total_xp, 4 * 5 + 3);

        // Re-logging today as done tops the XP up instead of adding a second log.
        let r = store
            .log_habit("test", &h.id, today, HabitLogStatus::Done, None, today)
            .unwrap();
        assert_eq!(r.xp_delta, 2);
        assert_eq!(
            store.list_habits(today).unwrap()[0].today,
            Some(HabitLogStatus::Done)
        );
    }

    #[test]
    fn missed_days_bend_then_break() {
        let store = Store::open_temporary().unwrap();
        let start = day("2026-11-01");
        let h = habit(&store, start);
        store
            .log_habit("t", &h.id, start, HabitLogStatus::Done, None, start)
            .unwrap();
        // Day 2 not logged, day 3 is today: bent.
        let v = &store.list_habits(day("2026-11-03")).unwrap()[0];
        assert_eq!(v.streak.state, StreakState::Bent);
        // Day 2 and 3 not logged, day 4 is today: broken.
        let v = &store.list_habits(day("2026-11-04")).unwrap()[0];
        assert_eq!(v.streak.state, StreakState::Broken);
    }

    #[test]
    fn resolve_by_name_and_reject_future() {
        let store = Store::open_temporary().unwrap();
        let today = day("2026-11-05");
        let h = habit(&store, today);
        assert_eq!(store.resolve_habit("read").unwrap().id, h.id);
        assert!(
            store
                .log_habit(
                    "t",
                    &h.id,
                    today + Duration::days(1),
                    HabitLogStatus::Done,
                    None,
                    today
                )
                .is_err()
        );
    }
}
