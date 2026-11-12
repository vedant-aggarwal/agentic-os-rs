//! Nora's proposals (decision 2026-11-12: no morning approval deck).
//!
//! Nora drafts a change to his words, goals, rules or identity as a `mc_proposal` row
//! (`pending`). She raises it in conversation, usually the 21:00 close-the-day call or chat
//! (`raised`). It is applied only when he agrees there (`accepted`), or dropped with his reason
//! (`rejected`, never re-proposed). Unraised proposals expire after 7 days.
//!
//! Applying an `edit` to text he wrote supersedes the old row instead of overwriting it.

use rusqlite::params;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::boards::{self, NewItem, NewNode};
use super::life::{self, NewDrop, NewEvidence, NewGoal, NewRule, NewStatement};
use super::{one_of, patch_row, resolve, row, rows, s};
use crate::error::{Result, invalid};
use crate::store::{ChangeOp, Store, Tx};
use crate::ids;

pub const TARGETS: &[&str] = &["goal", "step", "statement", "rule", "drop", "dream", "node", "card", "evidence"];

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewProposal {
    /// goal | step | statement | rule | drop | dream | node | card | evidence
    pub target_kind: String,
    pub target_id: Option<String>,
    /// add | edit | close | retire | link
    pub op: String,
    #[serde(default)]
    pub payload: Value,
    /// His words that led to it (journal or call).
    pub quote: Option<String>,
    pub reason: Option<String>,
    pub journal_id: Option<String>,
}

impl Store {
    pub fn mc_propose(&self, by: &str, p: NewProposal) -> Result<Value> {
        one_of("target_kind", &p.target_kind, TARGETS)?;
        one_of("op", &p.op, &["add", "edit", "close", "retire", "link"])?;
        if p.op != "add" && p.op != "link" && p.target_id.is_none() {
            return Err(invalid("edit, close and retire need target_id"));
        }
        if !p.payload.is_object() && !(p.op == "close" || p.op == "retire") {
            return Err(invalid("payload must be an object with the fields to add or change"));
        }
        let key = format!("{}|{}|{}|{}", p.target_kind, p.op, p.target_id.as_deref().unwrap_or(""), p.payload);
        self.write(by, |tx| {
            // Never re-propose what he already turned down.
            let refused: i64 = tx.query_row(
                "SELECT count(*) FROM mc_proposal WHERE status = 'rejected' AND target_kind || '|' || op || '|' || coalesce(target_id, '') || '|' || payload = ?1",
                [&key],
                |r| r.get(0),
            )?;
            if refused > 0 {
                return Err(invalid("he already said no to this exact change; don't propose it again"));
            }
            tx.execute("UPDATE mc_proposal SET status = 'expired' WHERE status = 'pending' AND created_at < datetime('now', '-7 days')", [])?;
            let id = ids::new_id();
            tx.execute(
                "INSERT INTO mc_proposal (id, journal_id, target_kind, target_id, op, payload, quote, reason, by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![id, p.journal_id, p.target_kind, p.target_id, p.op, if p.payload.is_null() { "{}".into() } else { p.payload.to_string() }, p.quote, p.reason, tx.actor(), tx.now()],
            )?;
            tx.record("mc_proposal", &id, ChangeOp::Upsert)?;
            row(tx, "SELECT * FROM mc_proposal WHERE id = ?1", &[s(&id)], "mc_proposal", &id)
        })
    }

    /// Proposals by status (`pending` and `raised` when `None`).
    pub fn mc_proposals(&self, status: Option<&str>) -> Result<Vec<Value>> {
        self.read(|c| match status {
            Some(st) => rows(c, "SELECT * FROM mc_proposal WHERE status = ?1 ORDER BY created_at DESC LIMIT 50", &[s(st)]),
            None => rows(c, "SELECT * FROM mc_proposal WHERE status IN ('pending', 'raised') ORDER BY created_at DESC LIMIT 50", &[]),
        })
    }

    /// Mark a proposal as raised in conversation (she asked him).
    pub fn mc_raise(&self, by: &str, id: &str) -> Result<Value> {
        self.write(by, |tx| {
            let id = resolve(tx, "mc_proposal", id)?;
            tx.execute("UPDATE mc_proposal SET status = 'raised', raised_at = ?2 WHERE id = ?1 AND status = 'pending'", params![id, tx.now()])?;
            tx.record("mc_proposal", &id, ChangeOp::Upsert)?;
            row(tx, "SELECT * FROM mc_proposal WHERE id = ?1", &[s(&id)], "mc_proposal", &id)
        })
    }

    /// His answer. `accept` applies the change (with `edited` fields laid over the payload when
    /// he changed it while agreeing); otherwise it is rejected with his reason.
    pub fn mc_decide(&self, by: &str, id: &str, accept: bool, reason: Option<&str>, edited: Option<&Map<String, Value>>) -> Result<Value> {
        self.write(by, |tx| {
            let id = resolve(tx, "mc_proposal", id)?;
            let p = row(tx, "SELECT * FROM mc_proposal WHERE id = ?1", &[s(&id)], "mc_proposal", &id)?;
            if !matches!(p["status"].as_str(), Some("pending" | "raised")) {
                return Err(invalid(format!("this proposal is already {}", p["status"].as_str().unwrap_or("closed"))));
            }
            if !accept {
                tx.execute("UPDATE mc_proposal SET status = 'rejected', decided_at = ?2, reject_reason = ?3 WHERE id = ?1", params![id, tx.now(), reason])?;
                tx.record("mc_proposal", &id, ChangeOp::Upsert)?;
                return row(tx, "SELECT * FROM mc_proposal WHERE id = ?1", &[s(&id)], "mc_proposal", &id);
            }
            let mut payload = p["payload"].as_object().cloned().unwrap_or_default();
            if let Some(e) = edited {
                for (k, v) in e {
                    payload.insert(k.clone(), v.clone());
                }
            }
            let result = apply(tx, p["target_kind"].as_str().unwrap_or(""), p["op"].as_str().unwrap_or(""), p["target_id"].as_str(), &payload)?;
            tx.execute("UPDATE mc_proposal SET status = 'accepted', decided_at = ?2, result_id = ?3 WHERE id = ?1", params![id, tx.now(), result])?;
            tx.record("mc_proposal", &id, ChangeOp::Upsert)?;
            row(tx, "SELECT * FROM mc_proposal WHERE id = ?1", &[s(&id)], "mc_proposal", &id)
        })
    }
}

fn from<T: serde::de::DeserializeOwned>(p: &Map<String, Value>) -> Result<T> {
    Ok(serde_json::from_value(Value::Object(p.clone()))?)
}

/// Apply one accepted change. Returns the id of the row it made or changed.
fn apply(tx: &Tx<'_>, kind: &str, op: &str, target: Option<&str>, p: &Map<String, Value>) -> Result<String> {
    let target = || target.ok_or_else(|| invalid("needs target_id"));
    let owner_text = |table: &'static str, id: &str| -> Result<bool> {
        let origin: String = tx.query_row(&format!("SELECT origin FROM {table} WHERE id = ?1"), [id], |r| r.get(0))?;
        Ok(origin != "nora")
    };
    match (kind, op) {
        ("goal" | "step", "add") => life::insert_goal(tx, &from::<NewGoal>(p)?),
        ("goal" | "step", "edit") => life::update_goal(tx, target()?, p),
        ("goal" | "step", "close" | "retire") => {
            let mut q = Map::new();
            q.insert("status".into(), p.get("status").cloned().unwrap_or(json!(if op == "retire" { "dropped" } else { "done" })));
            q.insert("closed_reason".into(), p.get("closed_reason").or(p.get("reason")).cloned().unwrap_or(Value::Null));
            life::update_goal(tx, target()?, &q)
        }
        ("statement", "add") => life::insert_statement(tx, &from::<NewStatement>(p)?),
        ("rule", "add") => life::insert_rule(tx, &from::<NewRule>(p)?),
        ("drop", "add") => life::insert_drop(tx, &from::<NewDrop>(p)?),
        ("statement" | "rule", "edit") => {
            let table = if kind == "rule" { "mc_rule" } else { "identity_statement" };
            let id = resolve(tx, table, target()?)?;
            let mut rest = p.clone();
            let mut out = id.clone();
            if let Some(text) = rest.remove("text").and_then(|t| t.as_str().map(String::from)) {
                out = if owner_text(table, &id)? {
                    life::supersede_text(tx, table, &id, &text)?
                } else {
                    let mut q = Map::new();
                    q.insert("text".into(), json!(text));
                    patch_row(tx, table, &id, &["text"], &q)?;
                    id.clone()
                };
            }
            let cols = if kind == "rule" { life::RULE_COLS } else { life::STATEMENT_COLS };
            patch_row(tx, table, &out, cols, &rest)?;
            Ok(out)
        }
        ("statement" | "rule", "retire" | "close") => {
            let table = if kind == "rule" { "mc_rule" } else { "identity_statement" };
            let id = resolve(tx, table, target()?)?;
            let mut q = Map::new();
            q.insert("status".into(), json!("retired"));
            patch_row(tx, table, &id, &["status"], &q)?;
            Ok(id)
        }
        ("drop", "edit" | "close") => {
            let id = resolve(tx, "drop_entry", target()?)?;
            let status = p.get("status").and_then(Value::as_str).unwrap_or(if op == "close" { "confirmed" } else { "open" });
            tx.execute("UPDATE drop_entry SET status = ?2, text = coalesce(?3, text) WHERE id = ?1", params![id, status, p.get("text").and_then(Value::as_str)])?;
            tx.record("drop_entry", &id, ChangeOp::Upsert)?;
            Ok(id)
        }
        ("dream", "edit" | "close") => {
            let id = resolve(tx, "mc_node", target()?)?;
            let mut q = p.clone();
            if op == "close" && !q.contains_key("stage") {
                q.insert("stage".into(), json!("reached"));
            }
            boards::set_path(tx, &id, &q)?;
            Ok(id)
        }
        ("node", "add") => boards::insert_node(tx, &from::<NewNode>(p)?),
        ("node", "edit") => {
            let id = resolve(tx, "mc_node", target()?)?;
            match p.get("text").and_then(Value::as_str) {
                Some(t) if owner_text("mc_node", &id)? => boards::supersede_node(tx, &id, t),
                _ => {
                    patch_row(tx, "mc_node", &id, &["text", "context"], p)?;
                    Ok(id)
                }
            }
        }
        ("card", "add") => boards::insert_item(tx, &from::<NewItem>(p)?),
        (_, "link") | ("evidence", _) => {
            let v = life::insert_evidence(tx, &from::<NewEvidence>(p)?)?;
            Ok(v["to_id"].as_str().unwrap_or_default().to_string())
        }
        (k, o) => Err(invalid(format!("can't {o} a {k}"))),
    }
}
