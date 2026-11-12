//! Version I want to be (goals by horizon, identity statements) and New Me (rules, the data
//! drop), plus evidence and progress.
//!
//! Progress is measured from actions, never guessed (mission-control.md 2.5):
//! - a step is done or not; a numeric step is current / target;
//! - a parent is the share of its children done;
//! - "votes" are distinct days with evidence in the last 14 days: linked tasks done, habit logs
//!   (done or floor) of a linked habit, done tasks in a linked area, and evidence rows from the
//!   journal or Nora. No streak resets to zero.

use std::collections::{BTreeSet, HashMap};

use chrono::{Duration, NaiveDate};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{log_event, one_of, origin_of, patch_row, rank_last, resolve, row, rows, s};
use crate::error::{Result, invalid};
use crate::store::{ChangeOp, Store, Tx};
use crate::ids;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewGoal {
    pub id: Option<String>,
    /// Parent goal: makes this a step (or a substep of a step).
    pub parent_id: Option<String>,
    pub horizon: Option<String>,
    pub section: Option<String>,
    pub title: String,
    pub why: Option<String>,
    pub dream_node_id: Option<String>,
    pub measure: Option<String>,
    pub target: Option<f64>,
    pub unit: Option<String>,
    pub due_on: Option<String>,
    pub task_area: Option<String>,
    pub habit_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewStatement {
    pub id: Option<String>,
    pub section: Option<String>,
    pub text: String,
    pub old_default: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewRule {
    pub id: Option<String>,
    pub kind: Option<String>,
    pub text: String,
    #[serde(alias = "trigger")]
    pub trigger_when: Option<String>,
    #[serde(alias = "action")]
    pub action_then: Option<String>,
    pub origin_drop_id: Option<String>,
    pub check_mode: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewDrop {
    pub id: Option<String>,
    pub kind: Option<String>,
    pub text: String,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub sources: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewEvidence {
    /// task | habit | journal | checkin | drop | manual
    pub from_kind: String,
    pub from_id: String,
    /// goal | statement | rule | dream | drop
    pub to_kind: String,
    pub to_id: String,
    pub relation: Option<String>,
    pub note: Option<String>,
    /// When it happened (default: a done task's close time, else now).
    pub at: Option<String>,
}

pub const GOAL_COLS: &[&str] = &[
    "title", "why", "horizon", "section", "dream_node_id", "measure", "target", "current", "unit", "due_on",
    "status", "closed_reason", "task_area", "habit_id", "parent_id", "rank",
];
pub const STATEMENT_COLS: &[&str] = &["text", "section", "old_default", "status"];
pub const RULE_COLS: &[&str] = &["text", "kind", "trigger_when", "action_then", "status", "check_mode", "rank"];
pub const DROP_COLS: &[&str] = &["text", "kind", "status", "rule_id", "data", "sources"];

fn day(t: &str) -> String {
    t.get(..10).unwrap_or(t).to_string()
}

impl Store {
    // -----------------------------------------------------------------------------------------
    // Goals

    pub fn mc_add_goal(&self, by: &str, g: NewGoal) -> Result<Value> {
        let id = self.write(by, |tx| insert_goal(tx, &g))?;
        self.read(|c| row(c, "SELECT * FROM mc_goal WHERE id = ?1", &[s(&id)], "mc_goal", &id))
    }

    pub fn mc_update_goal(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        let id = self.write(by, |tx| update_goal(tx, id, patch))?;
        self.read(|c| row(c, "SELECT * FROM mc_goal WHERE id = ?1", &[s(&id)], "mc_goal", &id))
    }

    /// Goals as trees (top level = goals, children = steps, grandchildren = substeps), each
    /// node with `progress` (0..1), `votes_14d`, `last_evidence` and `children`.
    pub fn mc_goal_tree(&self, horizon: Option<&str>, today: NaiveDate) -> Result<Vec<Value>> {
        self.read(|c| {
            let all = rows(c, "SELECT * FROM mc_goal WHERE deleted_at IS NULL ORDER BY rank", &[])?;
            let since = (today - Duration::days(13)).format("%Y-%m-%d").to_string();
            let mut by_parent: HashMap<Option<String>, Vec<Value>> = HashMap::new();
            for g in all {
                by_parent.entry(g["parent_id"].as_str().map(String::from)).or_default().push(g);
            }
            let tops = by_parent.get(&None).cloned().unwrap_or_default();
            let mut out = Vec::new();
            for g in tops {
                if horizon.is_some_and(|h| g["horizon"].as_str() != Some(h)) {
                    continue;
                }
                out.push(build_goal(c, g, &by_parent, &since, 0)?.0);
            }
            Ok(out)
        })
    }

    // -----------------------------------------------------------------------------------------
    // Identity statements

    pub fn mc_add_statement(&self, by: &str, n: NewStatement) -> Result<Value> {
        let id = self.write(by, |tx| insert_statement(tx, &n))?;
        self.read(|c| row(c, "SELECT * FROM identity_statement WHERE id = ?1", &[s(&id)], "identity_statement", &id))
    }

    pub fn mc_update_statement(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        let id = self.write(by, |tx| {
            let id = resolve(tx, "identity_statement", id)?;
            patch_row(tx, "identity_statement", &id, STATEMENT_COLS, patch)?;
            Ok(id)
        })?;
        self.read(|c| row(c, "SELECT * FROM identity_statement WHERE id = ?1", &[s(&id)], "identity_statement", &id))
    }

    /// Active statements with days voted in the last 14 (hard = a task or habit record,
    /// soft = said in the journal or logged by hand) and the latest three pieces of evidence.
    pub fn mc_statements(&self, today: NaiveDate) -> Result<Vec<Value>> {
        let since = (today - Duration::days(13)).format("%Y-%m-%d").to_string();
        self.read(|c| {
            let mut list = rows(c, "SELECT * FROM identity_statement WHERE status != 'retired' AND superseded_by IS NULL ORDER BY created_at", &[])?;
            for st in &mut list {
                let id = st["id"].as_str().unwrap_or_default().to_string();
                let ev = evidence_for(c, "statement", &id)?;
                let mut hard = BTreeSet::new();
                let mut soft = BTreeSet::new();
                for e in &ev {
                    let d = day(e["at"].as_str().unwrap_or(""));
                    if d >= since && e["relation"] != "contradicts" {
                        if matches!(e["from_kind"].as_str(), Some("task" | "habit")) { hard.insert(d) } else { soft.insert(d) };
                    }
                }
                let all: BTreeSet<_> = hard.union(&soft).cloned().collect();
                st["votes_14d"] = json!(all.len());
                st["hard_14d"] = json!(hard.len());
                st["soft_14d"] = json!(soft.len());
                st["last_evidence"] = json!(ev.into_iter().take(3).collect::<Vec<_>>());
            }
            Ok(list)
        })
    }

    // -----------------------------------------------------------------------------------------
    // Rules

    pub fn mc_add_rule(&self, by: &str, n: NewRule) -> Result<Value> {
        let id = self.write(by, |tx| insert_rule(tx, &n))?;
        self.read(|c| row(c, "SELECT * FROM mc_rule WHERE id = ?1", &[s(&id)], "mc_rule", &id))
    }

    pub fn mc_update_rule(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        let id = self.write(by, |tx| {
            let id = resolve(tx, "mc_rule", id)?;
            let before = row(tx, "SELECT text, trigger_when, action_then FROM mc_rule WHERE id = ?1", &[s(&id)], "mc_rule", &id)?;
            if let Some(k) = patch.get("kind").and_then(Value::as_str) {
                one_of("kind", k, &["usool", "understanding", "in_doubt", "trick", "archived"])?;
            }
            patch_row(tx, "mc_rule", &id, RULE_COLS, patch)?;
            log_event(tx, "mc_rule", &id, &before, &Value::Object(patch.clone()))?;
            Ok(id)
        })?;
        self.read(|c| row(c, "SELECT * FROM mc_rule WHERE id = ?1", &[s(&id)], "mc_rule", &id))
    }

    /// Record whether a rule was kept on a day (one check per rule per day; a second call corrects it).
    pub fn mc_check_rule(&self, by: &str, id: &str, date: NaiveDate, kept: bool, note: Option<&str>) -> Result<Value> {
        self.write(by, |tx| {
            let id = resolve(tx, "mc_rule", id)?;
            tx.execute(
                "INSERT INTO mc_rule_check (id, rule_id, date, kept, note, by) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (rule_id, date) DO UPDATE SET kept = excluded.kept, note = excluded.note, by = excluded.by",
                params![ids::new_id(), id, date.format("%Y-%m-%d").to_string(), kept as i64, note, tx.actor()],
            )?;
            tx.record("mc_rule", &id, ChangeOp::Upsert)?;
            Ok(json!({ "rule_id": id, "date": date, "kept": kept }))
        })
    }

    /// Rules (not retired, not superseded) with their kept rate over the last 14 checks and
    /// an `unchecked` flag when a checked rule has had no check for 14 days.
    pub fn mc_rules(&self, today: NaiveDate, kind: Option<&str>) -> Result<Vec<Value>> {
        let since = (today - Duration::days(13)).format("%Y-%m-%d").to_string();
        self.read(|c| {
            let mut list = rows(c, "SELECT * FROM mc_rule WHERE status != 'retired' AND superseded_by IS NULL ORDER BY kind, rank", &[])?;
            if let Some(k) = kind {
                list.retain(|r| r["kind"].as_str() == Some(k));
            }
            for r in &mut list {
                let id = r["id"].as_str().unwrap_or_default().to_string();
                let checks = rows(c, "SELECT date, kept FROM mc_rule_check WHERE rule_id = ?1 ORDER BY date DESC LIMIT 14", &[s(&id)])?;
                let kept = checks.iter().filter(|x| x["kept"] == 1).count();
                r["checks"] = json!(checks.len());
                r["kept"] = json!(kept);
                r["kept_rate"] = if checks.is_empty() { Value::Null } else { json!(kept as f64 / checks.len() as f64) };
                let last = checks.first().and_then(|x| x["date"].as_str()).map(String::from);
                r["last_check"] = json!(last);
                r["unchecked"] = json!(r["check_mode"] != "none" && last.as_deref().is_none_or(|d| d < since.as_str()) && day(r["created_at"].as_str().unwrap_or("")) < since);
            }
            Ok(list)
        })
    }

    // -----------------------------------------------------------------------------------------
    // Data drop

    pub fn mc_add_drop(&self, by: &str, n: NewDrop) -> Result<Value> {
        let id = self.write(by, |tx| insert_drop(tx, &n))?;
        self.read(|c| row(c, "SELECT * FROM drop_entry WHERE id = ?1", &[s(&id)], "drop_entry", &id))
    }

    pub fn mc_update_drop(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        let id = self.write(by, |tx| {
            let id = resolve(tx, "drop_entry", id)?;
            let mut p = patch.clone();
            for k in ["data", "sources"] {
                if let Some(v) = p.get(k).filter(|v| !v.is_string()).cloned() {
                    p.insert(k.into(), json!(v.to_string()));
                }
            }
            // drop_entry has no updated_at column: set fields one by one.
            for (k, v) in &p {
                if !DROP_COLS.contains(&k.as_str()) {
                    return Err(invalid(format!("drop: '{k}' can't be changed")));
                }
                let val = match v {
                    Value::String(x) => rusqlite::types::Value::Text(x.clone()),
                    Value::Null => rusqlite::types::Value::Null,
                    other => rusqlite::types::Value::Text(other.to_string()),
                };
                tx.execute(&format!("UPDATE drop_entry SET {k} = ?2 WHERE id = ?1"), params![id, val])?;
            }
            tx.record("drop_entry", &id, ChangeOp::Upsert)?;
            Ok(id)
        })?;
        self.read(|c| row(c, "SELECT * FROM drop_entry WHERE id = ?1", &[s(&id)], "drop_entry", &id))
    }

    pub fn mc_drops(&self, status: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        self.read(|c| match status {
            Some(st) => rows(c, &format!("SELECT * FROM drop_entry WHERE status = ?1 ORDER BY at DESC LIMIT {limit}"), &[s(st)]),
            None => rows(c, &format!("SELECT * FROM drop_entry WHERE status != 'rejected' ORDER BY at DESC LIMIT {limit}"), &[]),
        })
    }

    // -----------------------------------------------------------------------------------------
    // Evidence

    /// Link a task, habit, journal entry or drop to a goal, statement, rule, dream or drop.
    /// Linking the same pair twice is a no-op.
    pub fn mc_add_evidence(&self, by: &str, e: NewEvidence) -> Result<Value> {
        self.write(by, |tx| insert_evidence(tx, &e))
    }

    pub fn mc_evidence(&self, to_kind: &str, to_id: &str) -> Result<Vec<Value>> {
        self.read(|c| evidence_for(c, to_kind, to_id))
    }

    // -----------------------------------------------------------------------------------------
    // Pulse and Nora's view

    /// The Mission pulse: days with a vote this week, steps done this week, rules kept, plus counts.
    pub fn mc_overview(&self, today: NaiveDate) -> Result<Value> {
        let week = (today - Duration::days(6)).format("%Y-%m-%d").to_string();
        let fortnight = (today - Duration::days(13)).format("%Y-%m-%d").to_string();
        self.read(|c| {
            let q = |sql: &str, a: &str| -> Result<i64> { Ok(c.query_row(sql, [a], |r| r.get(0))?) };
            let vote_days = q(
                "SELECT count(DISTINCT d) FROM (
                    SELECT substr(e.at, 1, 10) AS d FROM evidence e LEFT JOIN task t ON e.from_kind = 'task' AND t.id = e.from_id
                     WHERE e.relation != 'contradicts' AND (e.from_kind != 'task' OR t.status = 'done') AND e.at >= ?1
                    UNION SELECT l.date FROM habit_log l JOIN mc_goal g ON g.habit_id = l.habit_id AND g.deleted_at IS NULL
                     WHERE l.status IN ('done', 'floor') AND l.date >= ?1)",
                &week,
            )?;
            let steps_done = q("SELECT count(*) FROM mc_goal WHERE status = 'done' AND deleted_at IS NULL AND updated_at >= ?1", &week)?;
            let (kept, checks): (i64, i64) = c.query_row(
                "SELECT coalesce(sum(kept), 0), count(*) FROM mc_rule_check WHERE date >= ?1",
                [&fortnight],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let n = |sql: &str| -> Result<i64> { Ok(c.query_row(sql, [], |r| r.get(0))?) };
            Ok(json!({
                "vote_days_7": vote_days,
                "steps_done_7": steps_done,
                "rules_kept_rate": if checks == 0 { Value::Null } else { json!(kept as f64 / checks as f64) },
                "boards": n("SELECT count(*) FROM board WHERE deleted_at IS NULL AND parent_board_id IS NULL")?,
                "dreams": n("SELECT count(*) FROM dream d JOIN mc_node m ON m.id = d.node_id WHERE m.deleted_at IS NULL AND m.kind != 'page'")?,
                "seeds": n("SELECT count(*) FROM dream d JOIN mc_node m ON m.id = d.node_id WHERE m.deleted_at IS NULL AND m.kind != 'page' AND d.stage = 'seed'")?,
                "goals_open": n("SELECT count(*) FROM mc_goal WHERE deleted_at IS NULL AND status IN ('open', 'moving')")?,
                "statements": n("SELECT count(*) FROM identity_statement WHERE status = 'active' AND superseded_by IS NULL")?,
                "rules": n("SELECT count(*) FROM mc_rule WHERE status = 'active' AND superseded_by IS NULL")?,
                "drops_open": n("SELECT count(*) FROM drop_entry WHERE status = 'open'")?,
                "proposals_pending": n("SELECT count(*) FROM mc_proposal WHERE status IN ('pending', 'raised')")?,
            }))
        })
    }

    /// Mission Control as plain text for Nora (calls, the 21:00 close-the-day, chat).
    /// Sensitive items are marked so she never quotes them in a message to anyone else.
    pub fn mc_context(&self, today: NaiveDate, max: usize) -> Result<String> {
        let mut out = Vec::new();
        let o = self.mc_overview(today)?;
        out.push(format!(
            "Mission pulse: {} of 7 days with a vote, {} steps done this week, rules kept {}.",
            o["vote_days_7"], o["steps_done_7"],
            o["rules_kept_rate"].as_f64().map(|r| format!("{:.0}%", r * 100.0)).unwrap_or_else(|| "not checked yet".into())
        ));
        let dreams = self.read(|c| {
            rows(c, "SELECT m.id, m.text, m.sensitive, d.stage, d.why, d.obstacle, d.if_then, d.first_step, d.last_touched_at
                     FROM dream d JOIN mc_node m ON m.id = d.node_id WHERE m.deleted_at IS NULL AND m.superseded_by IS NULL AND m.kind != 'page'
                     AND d.stage NOT IN ('reached', 'dropped') ORDER BY d.last_touched_at LIMIT 12", &[])
        })?;
        if !dreams.is_empty() {
            let lines: Vec<String> = dreams
                .iter()
                .map(|d| {
                    let missing: Vec<&str> = ["why", "obstacle", "if_then", "first_step"].into_iter().filter(|k| !d[*k].as_str().is_some_and(|v| !v.trim().is_empty())).collect();
                    let name = if d["sensitive"] == 1 { "[sensitive, never quote]".to_string() } else { clip(d["text"].as_str().unwrap_or("(image)"), 60) };
                    format!("- dream {} \"{}\" {}{}", short(&d["id"]), name, d["stage"].as_str().unwrap_or("seed"), if missing.is_empty() { String::new() } else { format!(", missing {}", missing.join("/")) })
                })
                .collect();
            out.push(format!("Dreams (stalest first):\n{}", lines.join("\n")));
        }
        let goals = self.mc_goal_tree(None, today)?;
        let mut gl = Vec::new();
        for g in &goals {
            if !matches!(g["status"].as_str(), Some("open" | "moving")) {
                continue;
            }
            gl.push(format!("- {} goal {} \"{}\" {:.0}%, {} vote days/14", g["horizon"].as_str().unwrap_or(""), short(&g["id"]), clip(g["title"].as_str().unwrap_or(""), 70), g["progress"].as_f64().unwrap_or(0.0) * 100.0, g["votes_14d"]));
            for st in g["children"].as_array().into_iter().flatten().filter(|x| matches!(x["status"].as_str(), Some("open" | "moving"))).take(4) {
                gl.push(format!("    step {} \"{}\"", short(&st["id"]), clip(st["title"].as_str().unwrap_or(""), 70)));
            }
        }
        if !gl.is_empty() {
            out.push(format!("Version I want to be (goals and open steps):\n{}", gl.join("\n")));
        }
        let sts = self.mc_statements(today)?;
        if !sts.is_empty() {
            out.push(format!(
                "Identity statements:\n{}",
                sts.iter().map(|x| format!("- {} \"{}\" {} vote days/14", short(&x["id"]), clip(x["text"].as_str().unwrap_or(""), 90), x["votes_14d"])).collect::<Vec<_>>().join("\n")
            ));
        }
        let rules = self.mc_rules(today, None)?;
        if !rules.is_empty() {
            out.push(format!(
                "New Me rules (check kept/broken nightly):\n{}",
                rules.iter().filter(|r| r["status"] == "active").take(20).map(|r| format!("- rule {} [{}] \"{}\"{}", short(&r["id"]), r["kind"].as_str().unwrap_or(""), clip(r["text"].as_str().unwrap_or(""), 90), if r["unchecked"] == true { " (unchecked 14 days)" } else { "" })).collect::<Vec<_>>().join("\n")
            ));
        }
        let drops = self.mc_drops(Some("open"), 6)?;
        if !drops.is_empty() {
            out.push(format!("Open data-drop entries:\n{}", drops.iter().map(|d| format!("- {} {}: {}", short(&d["id"]), d["kind"].as_str().unwrap_or(""), clip(d["text"].as_str().unwrap_or(""), 100))).collect::<Vec<_>>().join("\n")));
        }
        let props = self.mc_proposals(Some("pending"))?;
        if !props.is_empty() {
            out.push(format!(
                "Your drafted proposals not yet raised with him (raise one at a time, apply only on his yes):\n{}",
                props.iter().take(5).map(|p| format!("- proposal {} {} {}: {}", short(&p["id"]), p["op"].as_str().unwrap_or(""), p["target_kind"].as_str().unwrap_or(""), clip(p["reason"].as_str().unwrap_or(""), 100))).collect::<Vec<_>>().join("\n")
            ));
        }
        let text = out.join("\n");
        Ok(clip(&text, max))
    }
}

fn short(v: &Value) -> String {
    v.as_str().map(|s| s.chars().take(6).collect()).unwrap_or_default()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max).collect::<String>()) }
}

/// Evidence rows for a target, newest first, each with a readable `label`.
fn evidence_for(c: &Connection, to_kind: &str, to_id: &str) -> Result<Vec<Value>> {
    rows(c, "SELECT e.*, CASE e.from_kind
                WHEN 'task' THEN (SELECT title || CASE status WHEN 'done' THEN '' ELSE ' (' || status || ')' END FROM task WHERE id = e.from_id)
                WHEN 'habit' THEN (SELECT name FROM habit WHERE id = e.from_id)
                WHEN 'journal' THEN (SELECT substr(text, 1, 120) FROM journal_entry WHERE id = e.from_id)
                WHEN 'drop' THEN (SELECT substr(text, 1, 120) FROM drop_entry WHERE id = e.from_id)
                ELSE e.note END AS label
             FROM evidence e WHERE e.to_kind = ?1 AND e.to_id = ?2 ORDER BY e.at DESC LIMIT 50", &[s(to_kind), s(to_id)])
}

/// Days with evidence since `since` for one goal (own links, area, habit), and the latest items.
fn goal_days(c: &Connection, g: &Value, since: &str) -> Result<(BTreeSet<String>, Vec<Value>)> {
    let id = g["id"].as_str().unwrap_or_default();
    let mut days = BTreeSet::new();
    let ev = evidence_for(c, "goal", id)?;
    let mut recent = Vec::new();
    for e in &ev {
        let done_ok = e["from_kind"] != "task" || !e["label"].as_str().unwrap_or("").ends_with(')');
        if !done_ok || e["relation"] == "contradicts" {
            continue;
        }
        let d = day(e["at"].as_str().unwrap_or(""));
        if d.as_str() >= since {
            days.insert(d);
        }
        recent.push(e.clone());
    }
    if let Some(area) = g["task_area"].as_str() {
        let tasks = rows(c, "SELECT id, title AS label, closed_at AS at, 'task' AS from_kind FROM task WHERE area_id = ?1 AND status = 'done' AND closed_at >= ?2 ORDER BY closed_at DESC LIMIT 20", &[s(area), s(since)])?;
        for t in tasks {
            days.insert(day(t["at"].as_str().unwrap_or("")));
            recent.push(t);
        }
    }
    if let Some(h) = g["habit_id"].as_str() {
        let logs = rows(c, "SELECT l.id, h.name AS label, l.date AS at, 'habit' AS from_kind FROM habit_log l JOIN habit h ON h.id = l.habit_id WHERE l.habit_id = ?1 AND l.status IN ('done', 'floor') AND l.date >= ?2 ORDER BY l.date DESC", &[s(h), s(since)])?;
        for l in logs {
            days.insert(day(l["at"].as_str().unwrap_or("")));
            recent.push(l);
        }
    }
    recent.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    recent.truncate(3);
    Ok((days, recent))
}

/// One goal with its children; returns the node and its vote days.
fn build_goal(c: &Connection, mut g: Value, by_parent: &HashMap<Option<String>, Vec<Value>>, since: &str, depth: usize) -> Result<(Value, BTreeSet<String>)> {
    let id = g["id"].as_str().map(String::from);
    let (mut days, recent) = goal_days(c, &g, since)?;
    let mut children = Vec::new();
    if depth < 4 {
        for ch in by_parent.get(&id).cloned().unwrap_or_default() {
            let (node, d) = build_goal(c, ch, by_parent, since, depth + 1)?;
            days.extend(d);
            children.push(node);
        }
    }
    let status = g["status"].as_str().unwrap_or("open");
    let live: Vec<&Value> = children.iter().filter(|x| x["status"] != "dropped" && x["status"] != "parked").collect();
    let progress = if status == "done" {
        1.0
    } else if let (Some(t), Some(cur)) = (g["target"].as_f64(), g["current"].as_f64()) {
        if t > 0.0 { (cur / t).clamp(0.0, 1.0) } else { 0.0 }
    } else if g["measure"] == "streak14" {
        (days.len() as f64 / g["target"].as_f64().unwrap_or(5.0).max(1.0)).min(1.0)
    } else if !live.is_empty() {
        live.iter().map(|x| x["progress"].as_f64().unwrap_or(0.0)).sum::<f64>() / live.len() as f64
    } else {
        0.0
    };
    g["progress"] = json!(progress);
    g["votes_14d"] = json!(days.len());
    g["steps_total"] = json!(live.len());
    g["steps_done"] = json!(live.iter().filter(|x| x["status"] == "done").count());
    g["last_evidence"] = json!(recent);
    g["children"] = json!(children);
    Ok((g, days))
}

// ---------------------------------------------------------------------------------------------
// Inside a transaction (shared with proposals)

pub(crate) fn insert_goal(tx: &Tx<'_>, g: &NewGoal) -> Result<String> {
    if g.title.trim().is_empty() {
        return Err(invalid("a goal needs a title"));
    }
    let parent = g.parent_id.as_deref().map(|p| resolve(tx, "mc_goal", p)).transpose()?;
    // Steps inherit horizon and section from their parent.
    let (ph, ps): (Option<String>, Option<String>) = match &parent {
        Some(p) => tx.query_row("SELECT horizon, section FROM mc_goal WHERE id = ?1", [p], |r| Ok((r.get(0)?, r.get(1)?)))?,
        None => (None, None),
    };
    let horizon = g.horizon.clone().or(ph).unwrap_or_else(|| "short".into());
    let section = g.section.clone().or(ps).unwrap_or_else(|| "physical".into());
    one_of("horizon", &horizon, super::HORIZONS)?;
    one_of("section", &section, super::SECTIONS)?;
    let measure = g.measure.clone().unwrap_or_else(|| "done".into());
    one_of("measure", &measure, &["done", "count", "number", "streak14"])?;
    let rank = match &parent {
        Some(p) => rank_last(tx, "SELECT max(rank) FROM mc_goal WHERE parent_id = ?1", &[p])?,
        None => rank_last(tx, "SELECT max(rank) FROM mc_goal WHERE parent_id IS NULL", &[])?,
    };
    let id = g.id.clone().unwrap_or_else(ids::new_id);
    tx.execute(
        "INSERT INTO mc_goal (id, parent_id, horizon, section, title, why, dream_node_id, measure, target, unit, due_on, task_area, habit_id, rank, origin, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
        params![
            id, parent, horizon, section, g.title.trim(), g.why, g.dream_node_id, measure, g.target, g.unit, g.due_on,
            g.task_area, g.habit_id, rank, origin_of(tx.actor()), tx.now()
        ],
    )?;
    tx.record("mc_goal", &id, ChangeOp::Upsert)?;
    Ok(id)
}

pub(crate) fn update_goal(tx: &Tx<'_>, id: &str, patch: &Map<String, Value>) -> Result<String> {
    let id = resolve(tx, "mc_goal", id)?;
    for (k, allowed) in [("horizon", super::HORIZONS), ("section", super::SECTIONS), ("status", &["open", "moving", "done", "dropped", "parked"][..])] {
        if let Some(v) = patch.get(k).and_then(Value::as_str) {
            one_of(k, v, allowed)?;
        }
    }
    if patch.contains_key("title") || patch.contains_key("why") {
        let before = row(tx, "SELECT title, why FROM mc_goal WHERE id = ?1", &[s(&id)], "mc_goal", &id)?;
        log_event(tx, "mc_goal", &id, &before, &Value::Object(patch.clone()))?;
    }
    if patch.get("deleted").and_then(Value::as_bool) == Some(true) {
        tx.execute("UPDATE mc_goal SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1", params![id, tx.now()])?;
        tx.record("mc_goal", &id, ChangeOp::Delete)?;
        return Ok(id);
    }
    patch_row(tx, "mc_goal", &id, GOAL_COLS, patch)?;
    Ok(id)
}

pub(crate) fn insert_statement(tx: &Tx<'_>, n: &NewStatement) -> Result<String> {
    if n.text.trim().is_empty() {
        return Err(invalid("a statement needs text"));
    }
    let section = n.section.clone().unwrap_or_else(|| "mental".into());
    one_of("section", &section, super::SECTIONS)?;
    let id = n.id.clone().unwrap_or_else(ids::new_id);
    tx.execute(
        "INSERT INTO identity_statement (id, section, text, old_default, origin, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![id, section, n.text.trim(), n.old_default, origin_of(tx.actor()), tx.now()],
    )?;
    tx.record("identity_statement", &id, ChangeOp::Upsert)?;
    Ok(id)
}

pub(crate) fn insert_rule(tx: &Tx<'_>, n: &NewRule) -> Result<String> {
    if n.text.trim().is_empty() {
        return Err(invalid("a rule needs text"));
    }
    let kind = n.kind.clone().unwrap_or_else(|| "usool".into());
    one_of("kind", &kind, &["usool", "understanding", "in_doubt", "trick", "archived"])?;
    let check = n.check_mode.clone().unwrap_or_else(|| if kind == "usool" { "nightly".into() } else { "none".into() });
    one_of("check_mode", &check, &["nightly", "weekly", "none"])?;
    let rank = rank_last(tx, "SELECT max(rank) FROM mc_rule WHERE kind = ?1", &[&kind])?;
    let id = n.id.clone().unwrap_or_else(ids::new_id);
    tx.execute(
        "INSERT INTO mc_rule (id, kind, text, trigger_when, action_then, origin_drop_id, check_mode, origin, rank, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![id, kind, n.text.trim(), n.trigger_when, n.action_then, n.origin_drop_id, check, origin_of(tx.actor()), rank, tx.now()],
    )?;
    if let Some(d) = &n.origin_drop_id {
        tx.execute("UPDATE drop_entry SET status = 'became_rule', rule_id = ?2 WHERE id = ?1", params![d, id])?;
    }
    tx.record("mc_rule", &id, ChangeOp::Upsert)?;
    Ok(id)
}

pub(crate) fn insert_drop(tx: &Tx<'_>, n: &NewDrop) -> Result<String> {
    if n.text.trim().is_empty() {
        return Err(invalid("a data-drop entry needs text"));
    }
    let kind = n.kind.clone().unwrap_or_else(|| "observation".into());
    one_of("kind", &kind, &["observation", "pattern", "learning", "experiment", "metric"])?;
    let id = n.id.clone().unwrap_or_else(ids::new_id);
    let data = if n.data.is_null() { "{}".into() } else { n.data.to_string() };
    let sources = if n.sources.is_null() { "[]".into() } else { n.sources.to_string() };
    tx.execute(
        "INSERT INTO drop_entry (id, at, kind, text, data, sources, by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, tx.now(), kind, n.text.trim(), data, sources, origin_of(tx.actor())],
    )?;
    tx.record("drop_entry", &id, ChangeOp::Upsert)?;
    Ok(id)
}

pub(crate) fn insert_evidence(tx: &Tx<'_>, e: &NewEvidence) -> Result<Value> {
    one_of("from_kind", &e.from_kind, &["task", "habit", "journal", "checkin", "drop", "manual"])?;
    one_of("to_kind", &e.to_kind, &["goal", "statement", "rule", "dream", "drop"])?;
    let relation = e.relation.clone().unwrap_or_else(|| "advances".into());
    one_of("relation", &relation, &["advances", "supports", "contradicts", "explains"])?;
    let from_id = match e.from_kind.as_str() {
        "task" => resolve(tx, "task", &e.from_id)?,
        "habit" => resolve(tx, "habit", &e.from_id)?,
        "journal" => resolve(tx, "journal_entry", &e.from_id)?,
        "drop" => resolve(tx, "drop_entry", &e.from_id)?,
        _ => e.from_id.clone(),
    };
    let to_id = match e.to_kind.as_str() {
        "goal" => resolve(tx, "mc_goal", &e.to_id)?,
        "statement" => resolve(tx, "identity_statement", &e.to_id)?,
        "rule" => resolve(tx, "mc_rule", &e.to_id)?,
        "dream" => resolve(tx, "mc_node", &e.to_id)?,
        _ => resolve(tx, "drop_entry", &e.to_id)?,
    };
    let at = match (&e.at, e.from_kind.as_str()) {
        (Some(a), _) => a.clone(),
        (None, "task") => tx
            .query_row("SELECT closed_at FROM task WHERE id = ?1", [&from_id], |r| r.get::<_, Option<String>>(0))?
            .unwrap_or_else(|| tx.now().to_string()),
        _ => tx.now().to_string(),
    };
    let id = ids::new_id();
    let n = tx.execute(
        "INSERT OR IGNORE INTO evidence (id, at, from_kind, from_id, to_kind, to_id, relation, note, by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id, at, e.from_kind, from_id, e.to_kind, to_id, relation, e.note, origin_of(tx.actor())],
    )?;
    if n > 0 {
        tx.record("evidence", &id, ChangeOp::Upsert)?;
        // A dream with fresh evidence is moving.
        if e.to_kind == "dream" {
            tx.execute("UPDATE dream SET stage = 'moving', last_touched_at = ?2 WHERE node_id = ?1 AND stage = 'rooted'", params![to_id, tx.now()])?;
        }
    }
    Ok(json!({ "linked": n > 0, "from_kind": e.from_kind, "from_id": from_id, "to_kind": e.to_kind, "to_id": to_id, "at": at }))
}

/// Shared by proposals: supersede an owner's statement or rule (new row, old one points at it).
pub(crate) fn supersede_text(tx: &Tx<'_>, table: &'static str, id: &str, text: &str) -> Result<String> {
    let id = resolve(tx, table, id)?;
    let new_id = ids::new_id();
    let cols = match table {
        "identity_statement" => "section, old_default, status",
        "mc_rule" => "kind, trigger_when, action_then, origin_drop_id, status, check_mode, rank",
        _ => return Err(invalid("can't supersede this kind")),
    };
    tx.execute(
        &format!("INSERT INTO {table} (id, text, origin, created_at, updated_at, {cols}) SELECT ?2, ?3, ?4, ?5, ?5, {cols} FROM {table} WHERE id = ?1"),
        params![id, new_id, text, origin_of(tx.actor()), tx.now()],
    )?;
    tx.execute(&format!("UPDATE {table} SET superseded_by = ?2, updated_at = ?3 WHERE id = ?1"), params![id, new_id, tx.now()])?;
    log_event(tx, table, &id, &json!({}), &json!({ "superseded_by": new_id, "text": text }))?;
    tx.record(table, &id, ChangeOp::Upsert)?;
    tx.record(table, &new_id, ChangeOp::Upsert)?;
    Ok(new_id)
}
