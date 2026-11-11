//! XP and levels.
//!
//! XP is an append-only ledger (`xp_event`). Each source (a task, a habit log, a day's journal)
//! has a *target* amount; [`set_award`] adds the difference between the target and what that
//! source already earned. Reopening a done task therefore adds a negative row instead of
//! editing history, and calling it twice never double-counts.
//!
//! The amounts below are starting defaults, kept in one place so the Sunday review can tune them.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::ids;
use crate::model::HabitLogStatus;
use crate::store::{ChangeOp, Store, Tx};

/// XP for a done task, multiplied by its `xp_weight`.
pub const TASK_XP_PER_WEIGHT: i64 = 10;
/// XP for writing in the journal on a day (once per day).
pub const JOURNAL_XP: i64 = 10;

/// XP for a habit log: the habit's full XP when done, half (rounded up) for the floor version.
pub fn habit_xp(base: i64, status: HabitLogStatus) -> i64 {
    match status {
        HabitLogStatus::Done => base,
        HabitLogStatus::Floor => (base + 1) / 2,
        HabitLogStatus::Missed | HabitLogStatus::Excused => 0,
    }
}

/// Total XP needed to reach `level`. Level 1 starts at 0; each level costs 100 more than the
/// one before: L2 = 100, L3 = 300, L4 = 600, L5 = 1000.
pub fn xp_for_level(level: u32) -> i64 {
    let l = i64::from(level.max(1));
    50 * l * (l - 1)
}

/// Levels never stop; the avatar evolves through stages. A new stage every
/// `LEVELS_PER_STAGE` levels, up to `STAGES` (the final form). Tunable here.
pub const LEVELS_PER_STAGE: u32 = 3;
pub const STAGES: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub level: u32,
    /// Level plus progress into it, e.g. 7.42. Smooth input for animations.
    pub level_exact: f64,
    /// Avatar stage 1..=STAGES, derived from the level. The UI morphs the character per stage.
    pub stage: u32,
    /// 0.0..1.0 through the current stage (1.0 once the final stage is reached).
    pub stage_progress: f64,
    /// 0.0..1.0 across the whole journey from level 1 to the final stage. Drives gradual
    /// effects like the companion fading in.
    pub evolution: f64,
    pub total_xp: i64,
    /// XP earned inside the current level.
    pub into_level: i64,
    /// XP the current level spans (next threshold minus this one).
    pub level_size: i64,
    /// XP still needed for the next level.
    pub to_next: i64,
    /// into_level / level_size, 0.0..1.0.
    pub progress: f64,
}

/// The level for a total XP. Negative totals stay at level 1.
pub fn level_for(total_xp: i64) -> Level {
    let mut level = 1u32;
    while xp_for_level(level + 1) <= total_xp {
        level += 1;
    }
    let start = xp_for_level(level);
    let next = xp_for_level(level + 1);
    let into = (total_xp - start).max(0);
    let progress = into as f64 / (next - start) as f64;
    let level_exact = f64::from(level) + progress;
    let (stage, stage_progress, evolution) = stage_for(level_exact);
    Level {
        level,
        level_exact,
        stage,
        stage_progress,
        evolution,
        total_xp,
        into_level: into,
        level_size: next - start,
        to_next: next - total_xp.max(start),
        progress,
    }
}

/// Stage, progress through it, and overall evolution for a fractional level.
pub fn stage_for(level_exact: f64) -> (u32, f64, f64) {
    let per = f64::from(LEVELS_PER_STAGE);
    let span = per * f64::from(STAGES - 1); // levels from stage 1 to the final stage
    let climbed = (level_exact - 1.0).max(0.0);
    let evolution = (climbed / span).min(1.0);
    let stage_index = (climbed / per).floor().min(f64::from(STAGES - 1));
    let stage = stage_index as u32 + 1;
    let stage_progress = if stage == STAGES {
        1.0
    } else {
        (climbed / per - stage_index).clamp(0.0, 1.0)
    };
    (stage, stage_progress, evolution)
}

/// Bring the XP a source has earned up (or down) to `target`. Returns the change.
pub(crate) fn set_award(
    tx: &Tx<'_>,
    source: &str,
    source_id: &str,
    area_id: Option<&str>,
    target: i64,
    date: NaiveDate,
) -> Result<i64> {
    let earned: i64 = tx.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM xp_event WHERE source = ?1 AND source_id = ?2",
        params![source, source_id],
        |r| r.get(0),
    )?;
    let delta = target - earned;
    if delta != 0 {
        let id = ids::new_id();
        tx.execute(
            "INSERT INTO xp_event (id, source, source_id, area_id, amount, note, date, at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)",
            params![id, source, source_id, area_id, delta, date, tx.now()],
        )?;
        tx.record("xp_event", &id, ChangeOp::Upsert)?;
    }
    Ok(delta)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaXp {
    pub id: String,
    pub name: String,
    pub pillar_id: Option<String>,
    pub xp: i64,
    pub level: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PillarXp {
    pub id: String,
    pub name: String,
    pub xp: i64,
    pub level: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XpSummary {
    pub overall: Level,
    pub today: i64,
    /// The last 7 days including today.
    pub week: i64,
    pub pillars: Vec<PillarXp>,
    pub areas: Vec<AreaXp>,
    /// XP with no area (for example a habit without one).
    pub unassigned: i64,
}

impl Store {
    /// Overall level, per-pillar and per-area XP, and today's and this week's gains.
    pub fn xp_summary(&self, today: NaiveDate) -> Result<XpSummary> {
        self.read(|c| {
            let total: i64 =
                c.query_row("SELECT COALESCE(SUM(amount), 0) FROM xp_event", [], |r| {
                    r.get(0)
                })?;
            let sum_since = |from: NaiveDate| -> Result<i64> {
                Ok(c.query_row(
                    "SELECT COALESCE(SUM(amount), 0) FROM xp_event WHERE date >= ?1 AND date <= ?2",
                    params![from, today],
                    |r| r.get(0),
                )?)
            };
            let today_xp = sum_since(today)?;
            let week_xp = sum_since(today - Duration::days(6))?;

            let mut by_area: HashMap<Option<String>, i64> = HashMap::new();
            let mut stmt =
                c.prepare("SELECT area_id, SUM(amount) FROM xp_event GROUP BY area_id")?;
            for row in stmt.query_map([], |r| {
                Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
            })? {
                let (area, xp) = row?;
                by_area.insert(area, xp);
            }

            let mut areas = Vec::new();
            let mut stmt = c.prepare("SELECT id, name, pillar_id FROM area ORDER BY sort, name")?;
            for row in stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })? {
                let (id, name, pillar_id) = row?;
                let xp = by_area.remove(&Some(id.clone())).unwrap_or(0);
                areas.push(AreaXp {
                    level: level_for(xp).level,
                    id,
                    name,
                    pillar_id,
                    xp,
                });
            }
            // XP whose area was deleted, or that never had one.
            let unassigned: i64 = by_area.values().sum();

            let mut pillars = Vec::new();
            let mut stmt = c.prepare("SELECT id, name FROM pillar ORDER BY sort, name")?;
            for row in
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            {
                let (id, name) = row?;
                let xp = areas
                    .iter()
                    .filter(|a| a.pillar_id.as_deref() == Some(id.as_str()))
                    .map(|a| a.xp)
                    .sum();
                pillars.push(PillarXp {
                    level: level_for(xp).level,
                    id,
                    name,
                    xp,
                });
            }

            Ok(XpSummary {
                overall: level_for(total),
                today: today_xp,
                week: week_xp,
                pillars,
                areas,
                unassigned,
            })
        })
    }

    /// XP a single source has earned so far.
    pub fn xp_earned(&self, source: &str, source_id: &str) -> Result<i64> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT SUM(amount) FROM xp_event WHERE source = ?1 AND source_id = ?2",
                params![source, source_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .unwrap_or(0))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_thresholds() {
        assert_eq!(xp_for_level(1), 0);
        assert_eq!(xp_for_level(2), 100);
        assert_eq!(xp_for_level(3), 300);
        assert_eq!(xp_for_level(4), 600);
        assert_eq!(xp_for_level(5), 1000);
    }

    #[test]
    fn level_for_totals() {
        let l = level_for(0);
        assert_eq!(
            (l.level, l.into_level, l.to_next, l.level_size),
            (1, 0, 100, 100)
        );
        let l = level_for(99);
        assert_eq!((l.level, l.to_next), (1, 1));
        let l = level_for(100);
        assert_eq!(
            (l.level, l.into_level, l.to_next, l.level_size),
            (2, 0, 200, 200)
        );
        let l = level_for(450);
        assert_eq!((l.level, l.into_level, l.to_next), (3, 150, 150));
        assert!((l.progress - 0.5).abs() < 1e-9);
        assert_eq!(level_for(-20).level, 1);
    }

    #[test]
    fn stages_follow_levels() {
        let l = level_for(0);
        assert_eq!((l.stage, l.evolution), (1, 0.0));
        // Levels 1-3 are stage 1, 4-6 stage 2...
        assert_eq!(level_for(xp_for_level(3)).stage, 1);
        assert_eq!(level_for(xp_for_level(4)).stage, 2);
        let l = level_for(xp_for_level(5) + (xp_for_level(6) - xp_for_level(5)) / 2);
        assert_eq!(l.stage, 2);
        assert!((l.level_exact - 5.5).abs() < 1e-9);
        assert!((l.stage_progress - 0.5).abs() < 1e-9);
        // The final stage caps; levels keep going.
        let top = 1 + LEVELS_PER_STAGE * (STAGES - 1);
        let l = level_for(xp_for_level(top));
        assert_eq!((l.stage, l.evolution, l.stage_progress), (STAGES, 1.0, 1.0));
        let l = level_for(xp_for_level(top + 10));
        assert_eq!((l.level, l.stage), (top + 10, STAGES));
        // Evolution only ever grows with XP.
        let mut prev = -1.0;
        for xp in (0..200_000).step_by(997) {
            let e = level_for(xp).evolution;
            assert!(e >= prev);
            prev = e;
        }
    }

    #[test]
    fn habit_xp_rules() {
        assert_eq!(habit_xp(5, HabitLogStatus::Done), 5);
        assert_eq!(habit_xp(5, HabitLogStatus::Floor), 3);
        assert_eq!(habit_xp(5, HabitLogStatus::Missed), 0);
        assert_eq!(habit_xp(5, HabitLogStatus::Excused), 0);
    }

    #[test]
    fn set_award_is_idempotent_and_reversible() {
        let store = Store::open_temporary().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let delta = store
            .write("t", |tx| {
                set_award(tx, "task", "t1", Some("fitness"), 20, day)
            })
            .unwrap();
        assert_eq!(delta, 20);
        let again = store
            .write("t", |tx| {
                set_award(tx, "task", "t1", Some("fitness"), 20, day)
            })
            .unwrap();
        assert_eq!(again, 0);
        let undo = store
            .write("t", |tx| {
                set_award(tx, "task", "t1", Some("fitness"), 0, day)
            })
            .unwrap();
        assert_eq!(undo, -20);
        assert_eq!(store.xp_earned("task", "t1").unwrap(), 0);
    }

    #[test]
    fn summary_rolls_areas_into_pillars() {
        let store = Store::open_temporary().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        store
            .write("t", |tx| {
                set_award(tx, "task", "a", Some("fitness"), 60, day)?;
                set_award(tx, "task", "b", Some("health"), 50, day - Duration::days(3))?;
                set_award(
                    tx,
                    "task",
                    "c",
                    Some("reading"),
                    30,
                    day - Duration::days(10),
                )?;
                set_award(tx, "habit", "d", None, 5, day)?;
                Ok(())
            })
            .unwrap();
        let s = store.xp_summary(day).unwrap();
        assert_eq!(s.overall.total_xp, 145);
        assert_eq!(s.overall.level, 2);
        assert_eq!(s.today, 65);
        assert_eq!(s.week, 115);
        assert_eq!(s.unassigned, 5);
        let body = s.pillars.iter().find(|p| p.id == "body").unwrap();
        assert_eq!((body.xp, body.level), (110, 2));
        let mind = s.pillars.iter().find(|p| p.id == "mind").unwrap();
        assert_eq!(mind.xp, 30);
    }
}
