//! Mission Control: the Visualization board, Version I want to be, and New Me
//! (research/mission-control.md, design/specs/boards.md).
//!
//! - One tree of text blocks (`mc_node`) holds every caption, page title and synced line, so a
//!   later Craft sync is block to block. Layout (`board_item`), dream paths, goal measures, rules
//!   stats, evidence and proposals live in typed tables that never leave the app.
//! - Reads return JSON rows (`serde_json::Value`): the app, the tools and Nora all consume JSON.
//! - Owner text is never overwritten by Nora. She drafts a [`proposals`] row, raises it in
//!   conversation, and it is applied only when the owner agrees there. Accepting an edit of owner
//!   text supersedes the old row (kept, closed, pointing at the new one) instead of rewriting it.
//! - Each owner statement carries `verified_at`: the day the owner last said it is still true.
//!
//! Submodules: [`boards`] (boards, cards, dreams, images), [`life`] (goals, identity, rules,
//! data drop, evidence, progress), [`proposals`], [`craft`] (the sync seam, not wired yet).

pub mod boards;
pub mod craft;
pub mod life;
pub mod proposals;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, params_from_iter};
use serde_json::{Map, Value, json};

use crate::error::{Result, invalid, not_found};
use crate::store::{ChangeOp, Tx};

/// The Mission Control tables. Idempotent (`IF NOT EXISTS`), run on every open.
pub const SCHEMA: &str = include_str!("../../migrations/mission.sql");

pub fn ensure_schema(c: &Connection) -> Result<()> {
    c.execute_batch(SCHEMA)?;
    Ok(())
}

pub const SPACES: &[&str] = &["viz", "ver", "newme"];
pub const SECTIONS: &[&str] = &["physical", "mental", "monetary", "social", "other"];
pub const HORIZONS: &[&str] = &["short", "mid", "long"];

pub(crate) fn one_of(field: &str, v: &str, allowed: &[&str]) -> Result<()> {
    if allowed.contains(&v) {
        Ok(())
    } else {
        Err(invalid(format!("{field} must be one of {}, not '{v}'", allowed.join(", "))))
    }
}

/// Who wrote a row, from the actor: anything Nora sends is `nora`, the rest is the owner.
pub(crate) fn origin_of(actor: &str) -> &'static str {
    if actor.to_ascii_lowercase().contains("nora") { "nora" } else { "owner" }
}

// ---------------------------------------------------------------------------------------------
// Fractional ranks: inserting never renumbers siblings.

const LO: u8 = b'a';

/// A rank strictly between `a` and `b` (`""` = before everything, `None` = after everything).
/// Ranks never end in `a`, so plain text order equals rank order.
pub fn rank_between(a: &str, b: Option<&str>) -> String {
    let (a, mut b) = (a.as_bytes(), b.map(str::as_bytes));
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let da = a.get(i).map(|c| (c - LO) as i32).unwrap_or(0);
        let db = match b {
            None => 26,
            Some(b) => b.get(i).map(|c| (c - LO) as i32).unwrap_or(0),
        };
        if da == db {
            out.push(LO + da as u8);
            i += 1;
            if i > 64 {
                break;
            }
            continue;
        }
        if db - da > 1 {
            out.push(LO + ((da + db) / 2) as u8);
            break;
        }
        out.push(LO + da as u8);
        b = None;
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| "n".into())
}

/// The rank after the last sibling matched by `sql` (which selects `max(rank)`).
pub(crate) fn rank_last(c: &Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<String> {
    let last: Option<String> = c.query_row(sql, args, |r| r.get(0))?;
    Ok(rank_between(last.as_deref().unwrap_or(""), None))
}

// ---------------------------------------------------------------------------------------------
// JSON rows

fn cell(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
        ValueRef::Blob(_) => Value::Null,
    }
}

/// Run a query and return each row as a JSON object keyed by column name. Columns named
/// `meta`, `payload`, `data`, `sources` or `camera` holding JSON text are parsed.
pub(crate) fn rows(c: &Connection, sql: &str, args: &[SqlValue]) -> Result<Vec<Value>> {
    let mut stmt = c.prepare(sql)?;
    let names: Vec<String> = stmt.column_names().map(|s| s.to_string()).collect();
    let out = stmt
        .query_map(params_from_iter(args.iter()), |r| {
            let mut m = Map::new();
            for (i, n) in names.iter().enumerate() {
                let mut v = cell(r.get_ref(i)?);
                if matches!(n.as_str(), "meta" | "payload" | "data" | "sources" | "camera")
                    && let Some(s) = v.as_str()
                {
                    v = serde_json::from_str(s).unwrap_or(v);
                }
                m.insert(n.clone(), v);
            }
            Ok(Value::Object(m))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(out)
}

pub(crate) fn row(c: &Connection, sql: &str, args: &[SqlValue], kind: &'static str, id: &str) -> Result<Value> {
    rows(c, sql, args)?.into_iter().next().ok_or_else(|| not_found(kind, id))
}

pub(crate) fn s(v: impl Into<String>) -> SqlValue {
    SqlValue::Text(v.into())
}

fn sql_of(v: &Value) -> Result<SqlValue> {
    Ok(match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => SqlValue::Integer(i),
            None => SqlValue::Real(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    })
}

/// Update whitelisted columns of one row from a JSON object. Unknown keys are refused so a
/// typo never passes silently. Records the change and returns the number of columns set.
pub(crate) fn patch_row(tx: &Tx<'_>, table: &'static str, id: &str, allowed: &[&str], patch: &Map<String, Value>) -> Result<usize> {
    if patch.is_empty() {
        return Ok(0);
    }
    let mut sets = Vec::new();
    let mut args = Vec::new();
    for (k, v) in patch {
        if !allowed.contains(&k.as_str()) {
            return Err(invalid(format!("{table}: '{k}' can't be changed here (allowed: {})", allowed.join(", "))));
        }
        args.push(sql_of(v)?);
        sets.push(format!("{k} = ?{}", args.len()));
    }
    args.push(s(tx.now()));
    sets.push(format!("updated_at = ?{}", args.len()));
    args.push(s(id));
    let n = tx.execute(
        &format!("UPDATE {table} SET {} WHERE id = ?{}", sets.join(", "), args.len()),
        params_from_iter(args.iter()),
    )?;
    if n == 0 {
        return Err(not_found(table, id));
    }
    tx.record(table, id, ChangeOp::Upsert)?;
    Ok(sets.len() - 1)
}

/// Append to the undo log.
pub(crate) fn log_event(tx: &Tx<'_>, kind: &str, id: &str, before: &Value, after: &Value) -> Result<()> {
    tx.execute(
        "INSERT INTO mc_event (id, at, actor, target_kind, target_id, before, after) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![crate::ids::new_id(), tx.now(), tx.actor(), kind, id, before.to_string(), after.to_string()],
    )?;
    Ok(())
}

/// Resolve an id or unique prefix in one of the Mission tables.
pub(crate) fn resolve(c: &Connection, table: &'static str, key: &str) -> Result<String> {
    crate::store::resolve_prefix(c, table, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_order_and_never_collide() {
        let first = rank_between("", None);
        assert_eq!(first, "n");
        let after = rank_between(&first, None);
        assert!(after > first);
        let mid = rank_between(&first, Some(&after));
        assert!(mid > first && mid < after, "{first} {mid} {after}");
        let before = rank_between("", Some(&first));
        assert!(before < first && !before.ends_with('a'));
        // Repeated inserts between two neighbours stay ordered.
        let (mut lo, hi) = (first.clone(), after.clone());
        for _ in 0..40 {
            let m = rank_between(&lo, Some(&hi));
            assert!(m > lo && m < hi);
            lo = m;
        }
    }
}

#[cfg(test)]
mod flow_tests;
