//! Goals: the concrete thing each pillar is working toward (a laptop to earn, a bike to buy, a
//! body to build), with specs, the emotional "why", pictures or 3D model refs, a price and how
//! much is earned so far.
//!
//! Money is integer **paise** (₹1 = 100 paise) so totals are exact. Progress is
//! `earned / target` when there is a price, or a manual 0..1 value when there isn't.
//! Personal goals come from the local seed file, never from the repo.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid, not_found};
use crate::ids;
use crate::store::{ChangeOp, Store, Tx, resolve_prefix};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub pillar_id: Option<String>,
    pub area_id: Option<String>,
    pub name: String,
    /// Specs or features, one line each.
    pub specs: Vec<String>,
    /// Why it matters, in his words.
    pub why: Option<String>,
    /// Image paths, URLs or 3D model refs.
    pub images: Vec<String>,
    pub target_paise: Option<i64>,
    pub earned_paise: i64,
    /// Still to earn (never negative); `None` without a price.
    pub remaining_paise: Option<i64>,
    /// 0.0..1.0
    pub progress: f64,
    /// `active`, `reached` or `dropped`.
    pub status: String,
    pub sort: i64,
    pub reached_at: Option<String>,
}

/// Input for [`Store::add_goal`] and seed files. `id` is optional (seed files use readable ids).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NewGoal {
    pub id: Option<String>,
    pub pillar_id: Option<String>,
    pub area_id: Option<String>,
    pub name: String,
    pub specs: Vec<String>,
    pub why: Option<String>,
    pub images: Vec<String>,
    pub target_paise: Option<i64>,
    /// Starting amount; only used when the goal is first created.
    pub earned_paise: i64,
    /// Manual progress for goals without a price.
    pub progress: Option<f64>,
    pub sort: i64,
}

/// Changes for [`Store::update_goal`]. `None` leaves a field alone; `Some(None)` clears it.
#[derive(Debug, Clone, Default)]
pub struct GoalPatch {
    pub name: Option<String>,
    pub pillar_id: Option<Option<String>>,
    pub area_id: Option<Option<String>>,
    pub specs: Option<Vec<String>>,
    pub why: Option<Option<String>>,
    pub images: Option<Vec<String>>,
    pub target_paise: Option<Option<i64>>,
    pub progress: Option<Option<f64>>,
    /// `active`, `reached` or `dropped`.
    pub status: Option<String>,
}

const COLS: &str = "id, pillar_id, area_id, name, specs, why, images, target_paise, earned_paise, \
    progress, status, sort, reached_at";

fn json_list(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Progress from money when there's a price, else the manual value.
pub fn goal_progress(target: Option<i64>, earned: i64, manual: Option<f64>) -> f64 {
    match target {
        Some(t) if t > 0 => (earned as f64 / t as f64).clamp(0.0, 1.0),
        _ => manual.unwrap_or(0.0).clamp(0.0, 1.0),
    }
}

fn goal_from_row(r: &Row<'_>) -> rusqlite::Result<Goal> {
    let target: Option<i64> = r.get("target_paise")?;
    let earned: i64 = r.get("earned_paise")?;
    let manual: Option<f64> = r.get("progress")?;
    Ok(Goal {
        id: r.get("id")?,
        pillar_id: r.get("pillar_id")?,
        area_id: r.get("area_id")?,
        name: r.get("name")?,
        specs: json_list(r.get("specs")?),
        why: r.get("why")?,
        images: json_list(r.get("images")?),
        target_paise: target,
        earned_paise: earned,
        remaining_paise: target.map(|t| (t - earned).max(0)),
        progress: goal_progress(target, earned, manual),
        status: r.get("status")?,
        sort: r.get("sort")?,
        reached_at: r.get("reached_at")?,
    })
}

fn get_goal(c: &Connection, id: &str) -> Result<Goal> {
    c.query_row(
        &format!("SELECT {COLS} FROM goal WHERE id = ?1"),
        [id],
        goal_from_row,
    )
    .optional()?
    .ok_or_else(|| not_found("goal", id))
}

fn check(target: Option<i64>, progress: Option<f64>) -> Result<()> {
    if target.is_some_and(|t| t < 0) {
        return Err(invalid("target_paise can't be negative"));
    }
    if progress.is_some_and(|p| !(0.0..=1.0).contains(&p)) {
        return Err(invalid("progress must be 0..1"));
    }
    Ok(())
}

/// Insert or update by id. An existing goal keeps its earned amount, so re-applying a seed
/// never wipes progress.
pub(crate) fn upsert_goal(tx: &Tx<'_>, g: &NewGoal) -> Result<String> {
    if g.name.trim().is_empty() {
        return Err(invalid("goal name is empty"));
    }
    check(g.target_paise, g.progress)?;
    let id = g.id.clone().unwrap_or_else(ids::new_id);
    tx.execute(
        "INSERT INTO goal (id, pillar_id, area_id, name, specs, why, images, target_paise,
             earned_paise, progress, sort, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
         ON CONFLICT(id) DO UPDATE SET pillar_id = excluded.pillar_id, area_id = excluded.area_id,
             name = excluded.name, specs = excluded.specs, why = excluded.why,
             images = excluded.images, target_paise = excluded.target_paise,
             progress = COALESCE(excluded.progress, goal.progress), sort = excluded.sort,
             updated_at = excluded.updated_at",
        params![
            id,
            g.pillar_id,
            g.area_id,
            g.name.trim(),
            serde_json::to_string(&g.specs)?,
            g.why,
            serde_json::to_string(&g.images)?,
            g.target_paise,
            g.earned_paise,
            g.progress,
            g.sort,
            tx.now()
        ],
    )?;
    tx.record("goal", &id, ChangeOp::Upsert)?;
    Ok(id)
}

/// Marks a priced goal reached once it is fully earned.
fn settle(tx: &Tx<'_>, id: &str) -> Result<()> {
    tx.execute(
        "UPDATE goal SET status = 'reached', reached_at = COALESCE(reached_at, ?2)
         WHERE id = ?1 AND status = 'active' AND target_paise > 0 AND earned_paise >= target_paise",
        params![id, tx.now()],
    )?;
    Ok(())
}

impl Store {
    pub fn add_goal(&self, by: &str, g: NewGoal) -> Result<Goal> {
        self.write(by, |tx| {
            let id = upsert_goal(tx, &g)?;
            settle(tx, &id)?;
            get_goal(tx, &id)
        })
    }

    /// Goals in pillar order; closed ones only when asked.
    pub fn list_goals(&self, include_closed: bool) -> Result<Vec<Goal>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLS} FROM goal WHERE status = 'active' OR ?1
                 ORDER BY (SELECT sort FROM pillar WHERE pillar.id = goal.pillar_id), sort, name"
            ))?;
            let rows = stmt.query_map([include_closed], goal_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// By exact id, unique id prefix, or name (case-insensitive).
    pub fn resolve_goal(&self, key: &str) -> Result<Goal> {
        self.read(|c| {
            let by_name: Option<String> = c
                .query_row(
                    "SELECT id FROM goal WHERE lower(name) = lower(?1) LIMIT 1",
                    [key.trim()],
                    |r| r.get(0),
                )
                .optional()?;
            let id = match by_name {
                Some(id) => id,
                None => resolve_prefix(c, "goal", key)?,
            };
            get_goal(c, &id)
        })
    }

    pub fn update_goal(&self, by: &str, id: &str, p: GoalPatch) -> Result<Goal> {
        self.write(by, |tx| {
            let g = get_goal(tx, id)?;
            let manual: Option<f64> =
                tx.query_row("SELECT progress FROM goal WHERE id = ?1", [id], |r| {
                    r.get(0)
                })?;
            let target = p.target_paise.unwrap_or(g.target_paise);
            let progress = p.progress.unwrap_or(manual);
            check(target, progress)?;
            let status = p.status.unwrap_or(g.status.clone());
            if !matches!(status.as_str(), "active" | "reached" | "dropped") {
                return Err(invalid("status must be active, reached or dropped"));
            }
            let reached_at = match status.as_str() {
                "reached" => g.reached_at.clone().or_else(|| Some(tx.now().to_string())),
                _ => None,
            };
            tx.execute(
                "UPDATE goal SET name = ?2, pillar_id = ?3, area_id = ?4, specs = ?5, why = ?6,
                     images = ?7, target_paise = ?8, progress = ?9, status = ?10, reached_at = ?11,
                     updated_at = ?12
                 WHERE id = ?1",
                params![
                    id,
                    p.name
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty())
                        .unwrap_or(g.name),
                    p.pillar_id.unwrap_or(g.pillar_id),
                    p.area_id.unwrap_or(g.area_id),
                    serde_json::to_string(&p.specs.unwrap_or(g.specs))?,
                    p.why.unwrap_or(g.why),
                    serde_json::to_string(&p.images.unwrap_or(g.images))?,
                    target,
                    progress,
                    status,
                    reached_at,
                    tx.now()
                ],
            )?;
            tx.record("goal", id, ChangeOp::Upsert)?;
            settle(tx, id)?;
            get_goal(tx, id)
        })
    }

    /// Add money earned toward a goal (negative to correct a mistake).
    pub fn earn_toward_goal(&self, by: &str, id: &str, amount_paise: i64) -> Result<Goal> {
        if amount_paise == 0 {
            return Err(invalid("amount is zero"));
        }
        self.write(by, |tx| {
            let g = get_goal(tx, id)?;
            if g.earned_paise + amount_paise < 0 {
                return Err(invalid("earned amount can't go below zero"));
            }
            tx.execute(
                "UPDATE goal SET earned_paise = earned_paise + ?2, updated_at = ?3 WHERE id = ?1",
                params![id, amount_paise, tx.now()],
            )?;
            tx.record("goal", id, ChangeOp::Upsert)?;
            settle(tx, id)?;
            get_goal(tx, id)
        })
    }
}

/// `₹5,41,800` style (Indian digit grouping). Paise are shown only when not zero.
pub fn format_inr(paise: i64) -> String {
    let sign = if paise < 0 { "-" } else { "" };
    let p = paise.unsigned_abs();
    let (rupees, rem) = (p / 100, p % 100);
    let digits = rupees.to_string();
    let grouped = if digits.len() <= 3 {
        digits
    } else {
        let (head, tail) = digits.split_at(digits.len() - 3);
        let mut parts: Vec<String> = Vec::new();
        let mut h = head.to_string();
        while h.len() > 2 {
            parts.insert(0, h.split_off(h.len() - 2));
        }
        parts.insert(0, h);
        format!("{},{}", parts.join(","), tail)
    };
    if rem == 0 {
        format!("{sign}₹{grouped}")
    } else {
        format!("{sign}₹{grouped}.{rem:02}")
    }
}

/// Read rupees as typed (`541800`, `₹5,41,800`, `2500.50`, `-200`) into paise.
pub fn parse_inr(input: &str) -> Result<i64> {
    let cleaned: String = input
        .trim()
        .trim_start_matches('₹')
        .trim_start_matches("Rs.")
        .trim_start_matches("Rs")
        .chars()
        .filter(|c| *c != ',' && *c != '_' && !c.is_whitespace())
        .collect();
    let (neg, body) = match cleaned.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, cleaned.as_str()),
    };
    let bad = || invalid(format!("can't read '{input}' as rupees"));
    let (whole, frac) = body.split_once('.').unwrap_or((body, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || frac.len() > 2
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(bad());
    }
    let rupees: i64 = whole.parse().map_err(|_| bad())?;
    let paise: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<2}").parse().map_err(|_| bad())?
    };
    let total = rupees
        .checked_mul(100)
        .and_then(|r| r.checked_add(paise))
        .ok_or_else(bad)?;
    Ok(if neg { -total } else { total })
}

#[cfg(test)]
#[allow(clippy::inconsistent_digit_grouping)] // Indian grouping (lakh, crore) on purpose
mod tests {
    use super::*;

    #[test]
    fn rupee_format_and_parse() {
        assert_eq!(format_inr(541_800_00), "₹5,41,800");
        assert_eq!(format_inr(36_485_600), "₹3,64,856");
        assert_eq!(format_inr(1_23_45_678_90), "₹1,23,45,678.90");
        assert_eq!(format_inr(99_900), "₹999");
        assert_eq!(parse_inr("₹5,41,800").unwrap(), 541_800_00);
        assert_eq!(parse_inr("2500.5").unwrap(), 250_050);
        assert_eq!(parse_inr("-200").unwrap(), -20_000);
        assert!(parse_inr("lots").is_err());
        assert!(parse_inr("1.234").is_err());
    }

    #[test]
    fn earning_moves_progress_and_reaches() {
        let store = Store::open_temporary().unwrap();
        let g = store
            .add_goal(
                "t",
                NewGoal {
                    name: "New laptop".into(),
                    pillar_id: Some("wealth".into()),
                    specs: vec!["16 inch".into()],
                    target_paise: Some(100_000_00),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(g.progress, 0.0);
        let g = store.earn_toward_goal("t", &g.id, 25_000_00).unwrap();
        assert!((g.progress - 0.25).abs() < 1e-9);
        assert_eq!(g.remaining_paise, Some(75_000_00));
        assert!(store.earn_toward_goal("t", &g.id, -30_000_00).is_err());
        let g = store.earn_toward_goal("t", &g.id, 80_000_00).unwrap();
        assert_eq!(
            (g.status.as_str(), g.progress, g.remaining_paise),
            ("reached", 1.0, Some(0))
        );
        assert!(store.list_goals(false).unwrap().is_empty());
        assert_eq!(store.resolve_goal("new LAPTOP").unwrap().id, g.id);
    }

    #[test]
    fn manual_progress_and_patch() {
        let store = Store::open_temporary().unwrap();
        let g = store
            .add_goal(
                "t",
                NewGoal {
                    name: "Run 10k".into(),
                    progress: Some(0.3),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!((g.progress - 0.3).abs() < 1e-9);
        let g = store
            .update_goal(
                "t",
                &g.id,
                GoalPatch {
                    progress: Some(Some(0.6)),
                    why: Some(Some("feel strong".into())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!((g.progress - 0.6).abs() < 1e-9);
        assert_eq!(g.why.as_deref(), Some("feel strong"));
        assert!(
            store
                .update_goal(
                    "t",
                    &g.id,
                    GoalPatch {
                        progress: Some(Some(2.0)),
                        ..Default::default()
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn reseeding_keeps_earned() {
        let store = Store::open_temporary().unwrap();
        let seed = NewGoal {
            id: Some("bike".into()),
            name: "Bike".into(),
            target_paise: Some(1_000_00),
            ..Default::default()
        };
        store.add_goal("t", seed.clone()).unwrap();
        store.earn_toward_goal("t", "bike", 400_00).unwrap();
        let g = store.add_goal("t", seed).unwrap();
        assert_eq!(g.earned_paise, 400_00);
    }
}
