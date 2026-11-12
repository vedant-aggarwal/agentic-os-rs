//! Links between items (`[[Title]]`), and from items to boards, goals, projects and tasks.
//! One edge table; backlinks are the same rows read from the other end.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{Result, invalid};
use crate::mission::{rows, s};
use crate::store::{ChangeOp, Store, Tx};
use crate::ids;

pub const TARGETS: &[&str] = &["mind_item", "board", "mc_goal", "dream", "project", "task", "journal"];
pub const RELS: &[&str] = &["mentions", "about", "part_of", "learned_from", "supersedes", "contradicts", "supports"];

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewLink {
    pub from_id: String,
    /// mind_item (default), board, mc_goal, dream, project, task, journal.
    pub to_kind: Option<String>,
    pub to_id: Option<String>,
    /// For a mind item: link by title instead of id (resolves later if it doesn't exist yet).
    pub to_title: Option<String>,
    pub rel: Option<String>,
}

/// The `[[...]]` targets in a text, in order, without duplicates. `[[Title|label]]` links to
/// `Title`.
pub fn wiki_targets(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        let target = inner.split('|').next().unwrap_or("").trim();
        if !target.is_empty() && !target.contains('\n') && target.chars().count() <= 120 && !out.iter().any(|t| t.eq_ignore_ascii_case(target)) {
            out.push(target.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

/// The newest live item with this title (case-insensitive), other than `not`.
fn find_by_title(c: &Connection, title: &str, not: &str) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT id FROM mind_item WHERE lower(title) = lower(?1) AND id != ?2 AND deleted_at IS NULL
         ORDER BY superseded_by IS NULL DESC, saved_at DESC LIMIT 1",
        params![title.trim(), not],
        |r| r.get(0),
    )
    .optional()?)
}

/// Store an edge for every `[[...]]` in `text`, resolved to an item when one has that title.
pub(crate) fn link_wiki(tx: &Tx<'_>, from: &str, text: &str, by: &str) -> Result<()> {
    for t in wiki_targets(text) {
        let exists: Option<String> = tx
            .query_row(
                "SELECT id FROM mind_edge WHERE from_id = ?1 AND to_kind = 'mind_item' AND lower(to_title) = lower(?2)",
                params![from, t],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_some() {
            continue;
        }
        let to = find_by_title(tx, &t, from)?;
        tx.execute(
            "INSERT INTO mind_edge (id, from_id, to_kind, to_id, to_title, rel, by, created_at) VALUES (?1, ?2, 'mind_item', ?3, ?4, 'mentions', ?5, ?6)",
            params![ids::new_id(), from, to, t, by, tx.now()],
        )?;
    }
    Ok(())
}

/// When an item gets a title, links written earlier to that title now point at it.
pub(crate) fn resolve_dangling(tx: &Tx<'_>, id: &str) -> Result<()> {
    let title: Option<String> = tx.query_row("SELECT title FROM mind_item WHERE id = ?1", [id], |r| r.get(0))?;
    if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
        tx.execute(
            "UPDATE mind_edge SET to_id = ?1 WHERE to_kind = 'mind_item' AND to_id IS NULL AND lower(to_title) = lower(?2) AND from_id != ?1",
            params![id, t.trim()],
        )?;
    }
    Ok(())
}

/// Links out of an item, with what they point at (a removed item shows `removed: true`).
pub(crate) fn outgoing(c: &Connection, id: &str) -> Result<Vec<Value>> {
    rows(
        c,
        "SELECT e.id, e.to_kind, e.to_id, e.to_title, e.rel, e.by, e.status,
            CASE e.to_kind
              WHEN 'mind_item' THEN (SELECT coalesce(m.title, substr(m.raw, 1, 80)) FROM mind_item m WHERE m.id = e.to_id)
              WHEN 'board' THEN (SELECT b.title FROM board b WHERE b.id = e.to_id)
              WHEN 'mc_goal' THEN (SELECT g.title FROM mc_goal g WHERE g.id = e.to_id)
              ELSE NULL END AS label,
            CASE WHEN e.to_kind = 'mind_item' THEN (SELECT m.kind FROM mind_item m WHERE m.id = e.to_id) END AS to_item_kind,
            CASE WHEN e.to_kind = 'mind_item' AND e.to_id IS NOT NULL
                 THEN (SELECT m.deleted_at IS NOT NULL FROM mind_item m WHERE m.id = e.to_id) ELSE 0 END AS removed
         FROM mind_edge e WHERE e.from_id = ?1 AND e.status != 'rejected' ORDER BY e.created_at",
        &[s(id)],
    )
}

/// Items that link here, with the line around the link as context.
pub(crate) fn backlinks(c: &Connection, id: &str) -> Result<Vec<Value>> {
    let mut out = rows(
        c,
        "SELECT e.id, e.rel, e.to_title, m.id AS from_id, m.kind AS from_kind, coalesce(m.title, substr(m.raw, 1, 80)) AS label, m.raw
         FROM mind_edge e JOIN mind_item m ON m.id = e.from_id
         WHERE e.to_kind = 'mind_item' AND e.to_id = ?1 AND e.status != 'rejected' AND m.deleted_at IS NULL
         ORDER BY m.saved_at DESC",
        &[s(id)],
    )?;
    for b in &mut out {
        let raw = b["raw"].as_str().unwrap_or("").to_string();
        let ctx = b["to_title"]
            .as_str()
            .and_then(|t| raw.lines().find(|l| l.to_lowercase().contains(&format!("[[{}", t.to_lowercase()))))
            .map(|l| l.trim().chars().take(160).collect::<String>());
        b["context"] = json!(ctx);
        b.as_object_mut().map(|o| o.remove("raw"));
    }
    Ok(out)
}

impl Store {
    /// Link an item to another item (by id or title), a board, goal, dream, project, task or
    /// journal entry. Linking twice is a no-op.
    pub fn mind_link(&self, by: &str, l: NewLink) -> Result<Value> {
        let to_kind = l.to_kind.clone().unwrap_or_else(|| "mind_item".into());
        crate::mission::one_of("to_kind", &to_kind, TARGETS)?;
        let rel = l.rel.clone().unwrap_or_else(|| "mentions".into());
        crate::mission::one_of("rel", &rel, RELS)?;
        let who = if by.to_ascii_lowercase().contains("nora") { "nora" } else { "owner" };
        let edge = self.write(by, |tx| {
            let from = crate::store::resolve_prefix(tx, "mind_item", &l.from_id)?;
            let (to_id, to_title) = match to_kind.as_str() {
                "mind_item" => match (&l.to_id, &l.to_title) {
                    (Some(i), _) => (Some(crate::store::resolve_prefix(tx, "mind_item", i)?), None),
                    (None, Some(t)) if !t.trim().is_empty() => (find_by_title(tx, t, &from)?, Some(t.trim().to_string())),
                    _ => return Err(invalid("give to_id or to_title")),
                },
                "board" => (Some(crate::store::resolve_prefix(tx, "board", l.to_id.as_deref().unwrap_or(""))?), None),
                "mc_goal" => (Some(crate::store::resolve_prefix(tx, "mc_goal", l.to_id.as_deref().unwrap_or(""))?), None),
                _ => (Some(l.to_id.clone().filter(|i| !i.trim().is_empty()).ok_or_else(|| invalid("give to_id"))?), None),
            };
            if to_id.as_deref() == Some(from.as_str()) {
                return Err(invalid("an item can't link to itself"));
            }
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM mind_edge WHERE from_id = ?1 AND to_kind = ?2 AND coalesce(to_id, '') = coalesce(?3, '')
                     AND coalesce(lower(to_title), '') = coalesce(lower(?4), '') AND rel = ?5",
                    params![from, to_kind, to_id, to_title, rel],
                    |r| r.get(0),
                )
                .optional()?;
            let id = match existing {
                Some(id) => id,
                None => {
                    let id = ids::new_id();
                    tx.execute(
                        "INSERT INTO mind_edge (id, from_id, to_kind, to_id, to_title, rel, by, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![id, from, to_kind, to_id, to_title, rel, who, tx.now()],
                    )?;
                    id
                }
            };
            tx.record("mind_item", &from, ChangeOp::Upsert)?;
            Ok(id)
        })?;
        self.read(|c| Ok(rows(c, "SELECT * FROM mind_edge WHERE id = ?1", &[s(&edge)])?.into_iter().next().unwrap_or(Value::Null)))
    }

    /// Remove one link.
    pub fn mind_unlink(&self, by: &str, edge_id: &str) -> Result<()> {
        self.write(by, |tx| {
            let from: Option<String> = tx.query_row("SELECT from_id FROM mind_edge WHERE id = ?1", [edge_id], |r| r.get(0)).optional()?;
            let from = from.ok_or_else(|| crate::error::not_found("mind_edge", edge_id))?;
            tx.execute("DELETE FROM mind_edge WHERE id = ?1", [edge_id])?;
            tx.record("mind_item", &from, ChangeOp::Upsert)?;
            Ok(())
        })
    }

    /// Items linked to a board, goal, project or task (for a project's Research tab).
    pub fn mind_linked_to(&self, to_kind: &str, to_id: &str) -> Result<Vec<Value>> {
        self.read(|c| {
            rows(
                c,
                "SELECT m.id, m.kind, coalesce(m.title, substr(m.raw, 1, 80)) AS label, m.thumb_asset_id, e.rel
                 FROM mind_edge e JOIN mind_item m ON m.id = e.from_id
                 WHERE e.to_kind = ?1 AND e.to_id = ?2 AND m.deleted_at IS NULL ORDER BY m.saved_at DESC",
                &[s(to_kind), s(to_id)],
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::wiki_targets;

    #[test]
    fn finds_wiki_targets() {
        assert_eq!(wiki_targets("see [[Deep Work]] and [[deep work|it]] then [[Other]]"), vec!["Deep Work", "Other"]);
        assert!(wiki_targets("no [[ end").is_empty());
        assert!(wiki_targets("[[]]").is_empty());
    }
}
