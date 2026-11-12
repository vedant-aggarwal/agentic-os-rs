//! Boards, cards, dreams and images.
//!
//! A board is a canvas. A card (`board_item`) is only layout: x, y, w, h, z, rotation. What the
//! card shows is referenced: a text, image or link block in `mc_node`, a goal in `mc_goal`, or a
//! nested board (a subpage). On a `viz` board every card is a dream and carries its path: why,
//! obstacle, if-then plan, first step. A dream with no obstacle or first step is a "seed".

use std::path::PathBuf;

use rusqlite::params;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{log_event, one_of, origin_of, patch_row, rank_last, resolve, row, rows, s};
use crate::error::{Result, invalid, not_found};
use crate::store::{ChangeOp, Store, Tx};
use crate::{ids, paths, time};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewNode {
    pub id: Option<String>,
    pub space: String,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    pub style: Option<String>,
    #[serde(default)]
    pub text: String,
    pub context: Option<String>,
    pub url: Option<String>,
    pub asset_id: Option<String>,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub local_only: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewBoard {
    pub id: Option<String>,
    pub title: String,
    pub kind: Option<String>,
    pub parent_board_id: Option<String>,
    pub accent: Option<String>,
    #[serde(default)]
    pub sensitive: bool,
}

/// A card to place on a board. `kind`: image (give `asset_id`), text, link (give `url`), goal
/// (give `goal_id`) or board (a subpage; `caption` becomes its title).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewItem {
    pub id: Option<String>,
    pub board_id: String,
    pub kind: String,
    #[serde(default)]
    pub caption: String,
    pub context: Option<String>,
    pub url: Option<String>,
    pub asset_id: Option<String>,
    pub goal_id: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
    pub color: Option<String>,
    #[serde(default)]
    pub sensitive: bool,
    /// Dream path (viz boards).
    #[serde(default)]
    pub path: DreamPath,
}

#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
pub struct DreamPath {
    pub why: Option<String>,
    pub outcome: Option<String>,
    pub obstacle: Option<String>,
    pub if_then: Option<String>,
    pub first_step: Option<String>,
}

impl DreamPath {
    fn is_empty(&self) -> bool {
        [&self.why, &self.outcome, &self.obstacle, &self.if_then, &self.first_step].iter().all(|v| v.is_none())
    }
}

/// One card's new place, for a drag or resize commit from the canvas.
#[derive(Debug, Clone, Deserialize)]
pub struct Move {
    pub id: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
    pub z: Option<i64>,
    pub rot: Option<f64>,
}

const NODE_COLS: &[&str] = &["text", "context", "url", "style", "closed", "sensitive", "sync_scope", "asset_id", "meta"];
const ITEM_COLS: &[&str] = &["x", "y", "w", "h", "z", "rot", "color", "sensitive", "play_order"];
const PATH_FIELDS: &[&str] = &["why", "outcome", "obstacle", "if_then", "first_step"];

/// Stage from the path: seed until obstacle and first step exist, rooted when all four are filled.
fn stage_for(why: bool, obstacle: bool, if_then: bool, first_step: bool) -> &'static str {
    if why && obstacle && if_then && first_step { "rooted" } else { "seed" }
}

fn filled(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.trim().is_empty())
}

impl Store {
    // -----------------------------------------------------------------------------------------
    // Nodes

    pub fn mc_add_node(&self, by: &str, n: NewNode) -> Result<Value> {
        let id = self.write(by, |tx| insert_node(tx, &n))?;
        self.mc_node(&id)
    }

    pub fn mc_node(&self, id: &str) -> Result<Value> {
        self.read(|c| {
            let id = resolve(c, "mc_node", id)?;
            row(c, "SELECT * FROM mc_node WHERE id = ?1", &[s(&id)], "mc_node", &id)
        })
    }

    /// Children of a node (or the top level of a space), in rank order. Superseded and
    /// deleted rows are left out unless `history` is set.
    pub fn mc_list_nodes(&self, space: Option<&str>, parent: Option<&str>, history: bool) -> Result<Vec<Value>> {
        self.read(|c| {
            let mut sql = "SELECT * FROM mc_node WHERE 1=1".to_string();
            let mut args = Vec::new();
            if let Some(sp) = space {
                args.push(s(sp));
                sql.push_str(&format!(" AND space = ?{}", args.len()));
            }
            match parent {
                Some(p) => {
                    args.push(s(resolve(c, "mc_node", p)?));
                    sql.push_str(&format!(" AND parent_id = ?{}", args.len()));
                }
                None => sql.push_str(" AND parent_id IS NULL"),
            }
            if !history {
                sql.push_str(" AND deleted_at IS NULL AND superseded_by IS NULL");
            }
            sql.push_str(" ORDER BY rank");
            rows(c, &sql, &args)
        })
    }

    /// Change a node in place. For a meaning change of owner text, use [`Store::mc_supersede_node`].
    pub fn mc_update_node(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        let id = self.write(by, |tx| {
            let id = resolve(tx, "mc_node", id)?;
            let before = row(tx, "SELECT text, context FROM mc_node WHERE id = ?1", &[s(&id)], "mc_node", &id)?;
            patch_row(tx, "mc_node", &id, NODE_COLS, patch)?;
            if patch.contains_key("text") || patch.contains_key("context") {
                log_event(tx, "mc_node", &id, &before, &Value::Object(patch.clone()))?;
            }
            Ok(id)
        })?;
        self.mc_node(&id)
    }

    /// The supersede rule: a new node takes the old one's place (same parent, right after it);
    /// the old one is closed and points at the new one. Nothing he wrote is lost.
    pub fn mc_supersede_node(&self, by: &str, id: &str, text: &str) -> Result<Value> {
        let new_id = self.write(by, |tx| supersede_node(tx, id, text))?;
        self.mc_node(&new_id)
    }

    /// "Still true": stamp today's date on a node, goal, statement or rule.
    pub fn mc_verify(&self, by: &str, kind: &str, id: &str) -> Result<String> {
        let table: &'static str = match kind {
            "node" | "mc_node" => "mc_node",
            "goal" | "step" => "mc_goal",
            "statement" => "identity_statement",
            "rule" => "mc_rule",
            other => return Err(invalid(format!("can't verify a '{other}'"))),
        };
        let day = time::local_today().format("%Y-%m-%d").to_string();
        self.write(by, |tx| {
            let id = resolve(tx, table, id)?;
            let mut p = Map::new();
            p.insert("verified_at".into(), json!(day));
            patch_row(tx, table, &id, &["verified_at"], &p)?;
            Ok(day.clone())
        })
    }

    // -----------------------------------------------------------------------------------------
    // Boards

    pub fn mc_add_board(&self, by: &str, b: NewBoard) -> Result<Value> {
        let id = self.write(by, |tx| insert_board(tx, &b))?;
        self.mc_board_row(&id)
    }

    fn mc_board_row(&self, id: &str) -> Result<Value> {
        self.read(|c| row(c, BOARD_SELECT_ONE, &[s(id)], "board", id))
    }

    /// Boards under a parent (top level when `None`), each with its card count and a cover.
    pub fn mc_list_boards(&self, parent: Option<&str>) -> Result<Vec<Value>> {
        self.read(|c| {
            let (cond, args) = match parent {
                Some(p) => ("b.parent_board_id = ?1", vec![s(resolve(c, "board", p)?)]),
                None => ("b.parent_board_id IS NULL", vec![]),
            };
            rows(c, &format!("{BOARD_SELECT} WHERE {cond} AND b.deleted_at IS NULL AND b.archived = 0 ORDER BY b.rank"), &args)
        })
    }

    /// A board with its cards (content, dream path and goal joined in) and the way back up.
    pub fn mc_board(&self, id: &str) -> Result<Value> {
        self.read(|c| {
            let id = resolve(c, "board", id)?;
            let board = row(c, BOARD_SELECT_ONE, &[s(&id)], "board", &id)?;
            let items = rows(c, ITEM_SELECT, &[s(&id)])?;
            let mut crumbs = Vec::new();
            let mut up: Option<String> = board["parent_board_id"].as_str().map(String::from);
            while let Some(p) = up.take() {
                if crumbs.len() > 20 {
                    break;
                }
                let b = row(c, "SELECT id, title, parent_board_id FROM board WHERE id = ?1", &[s(&p)], "board", &p)?;
                up = b["parent_board_id"].as_str().map(String::from);
                crumbs.insert(0, json!({ "id": b["id"], "title": b["title"] }));
            }
            // A nested board is a dream too: its path lives on its page block.
            let path = match board["node_id"].as_str() {
                Some(n) => rows(c, "SELECT * FROM dream WHERE node_id = ?1", &[s(n)])?.into_iter().next().unwrap_or(Value::Null),
                None => Value::Null,
            };
            Ok(json!({ "board": board, "items": items, "breadcrumb": crumbs, "path": path }))
        })
    }

    pub fn mc_update_board(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        let id = self.write(by, |tx| {
            let id = resolve(tx, "board", id)?;
            let mut p = patch.clone();
            if let Some(cam) = p.get("camera").filter(|v| v.is_object()).cloned() {
                p.insert("camera".into(), json!(cam.to_string()));
            }
            if let Some(l) = p.get("layout").and_then(Value::as_str) {
                one_of("layout", l, &["grid", "canvas", "story"])?;
            }
            // The title lives on the page block too, so Craft sees the rename.
            if let Some(t) = p.get("title").and_then(Value::as_str) {
                tx.execute("UPDATE mc_node SET text = ?2, updated_at = ?3 WHERE id = (SELECT node_id FROM board WHERE id = ?1)", params![id, t, tx.now()])?;
            }
            patch_row(tx, "board", &id, &["title", "accent", "cover_asset", "layout", "camera", "sensitive", "archived"], &p)?;
            Ok(id)
        })?;
        self.mc_board_row(&id)
    }

    // -----------------------------------------------------------------------------------------
    // Cards

    pub fn mc_add_item(&self, by: &str, n: NewItem) -> Result<Value> {
        let id = self.write(by, |tx| insert_item(tx, &n))?;
        self.mc_item(&id)
    }

    pub fn mc_item(&self, id: &str) -> Result<Value> {
        self.read(|c| {
            let id = resolve(c, "board_item", id)?;
            let sql = ITEM_SELECT.replace("WHERE i.board_id = ?1", "WHERE i.id = ?1");
            row(c, &sql, &[s(&id)], "board_item", &id)
        })
    }

    /// Commit drags and resizes from the canvas in one transaction.
    pub fn mc_move_items(&self, by: &str, moves: &[Move]) -> Result<usize> {
        self.write(by, |tx| {
            for m in moves {
                let id = resolve(tx, "board_item", &m.id)?;
                let n = tx.execute(
                    "UPDATE board_item SET x = coalesce(?2, x), y = coalesce(?3, y), w = coalesce(?4, w), h = coalesce(?5, h),
                     z = coalesce(?6, z), rot = coalesce(?7, rot), version = version + 1, updated_at = ?8 WHERE id = ?1",
                    params![id, m.x, m.y, m.w.map(|w| w.max(40.0)), m.h.map(|h| h.max(40.0)), m.z, m.rot, tx.now()],
                )?;
                if n == 0 {
                    return Err(not_found("board_item", &m.id));
                }
                tx.record("board_item", &id, ChangeOp::Upsert)?;
            }
            Ok(moves.len())
        })
    }

    /// Change a card: `caption`, `context`, `url` go to its block; `why`, `outcome`, `obstacle`,
    /// `if_then`, `first_step`, `stage`, `closed_reason` to its dream path; layout fields to the card.
    pub fn mc_update_item(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        let id = self.write(by, |tx| {
            let id = resolve(tx, "board_item", id)?;
            let it = row(tx, "SELECT kind, ref_kind, ref_id FROM board_item WHERE id = ?1", &[s(&id)], "board_item", &id)?;
            let (mut node_p, mut item_p, mut path_p) = (Map::new(), Map::new(), Map::new());
            for (k, v) in patch {
                match k.as_str() {
                    "caption" => {
                        node_p.insert("text".into(), v.clone());
                    }
                    "context" | "url" => {
                        node_p.insert(k.clone(), v.clone());
                    }
                    "stage" | "closed_reason" => {
                        path_p.insert(k.clone(), v.clone());
                    }
                    k2 if PATH_FIELDS.contains(&k2) => {
                        path_p.insert(k.clone(), v.clone());
                    }
                    _ => {
                        item_p.insert(k.clone(), v.clone());
                    }
                }
            }
            patch_row(tx, "board_item", &id, ITEM_COLS, &item_p)?;
            let node_id = card_node(tx, &it)?;
            if !node_p.is_empty() {
                let nid = node_id.clone().ok_or_else(|| invalid("this card has no text block"))?;
                let before = row(tx, "SELECT text, context, url FROM mc_node WHERE id = ?1", &[s(&nid)], "mc_node", &nid)?;
                patch_row(tx, "mc_node", &nid, NODE_COLS, &node_p)?;
                log_event(tx, "mc_node", &nid, &before, &Value::Object(node_p.clone()))?;
                // A subpage card's caption is the nested board's title.
                if it["kind"] == "board"
                    && let Some(t) = node_p.get("text").and_then(Value::as_str)
                {
                    tx.execute("UPDATE board SET title = ?2, updated_at = ?3 WHERE id = ?1", params![it["ref_id"].as_str(), t, tx.now()])?;
                }
            }
            if !path_p.is_empty() {
                let nid = node_id.ok_or_else(|| invalid("this card can't carry a path"))?;
                set_path(tx, &nid, &path_p)?;
            }
            Ok(id)
        })?;
        self.mc_item(&id)
    }

    /// Take a card off its board. The block stays (soft delete, undo by clearing `deleted_at`).
    pub fn mc_remove_item(&self, by: &str, id: &str) -> Result<()> {
        self.write(by, |tx| {
            let id = resolve(tx, "board_item", id)?;
            tx.execute("UPDATE board_item SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1", params![id, tx.now()])?;
            tx.record("board_item", &id, ChangeOp::Delete)?;
            Ok(())
        })
    }

    // -----------------------------------------------------------------------------------------
    // Images

    /// Store image bytes in the media folder (deduplicated by hash) and return the asset row.
    pub fn mc_add_asset(&self, by: &str, bytes: &[u8], mime: &str, source_url: Option<&str>) -> Result<Value> {
        let ext = match mime {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/png" => "png",
            "image/webp" => "webp",
            "image/gif" => "gif",
            "image/heic" => "heic",
            "image/avif" => "avif",
            other => return Err(invalid(format!("not an image type we keep: {other}"))),
        };
        if bytes.is_empty() {
            return Err(invalid("empty image"));
        }
        let sha = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();
        if let Some(found) = self.read(|c| Ok(rows(c, "SELECT * FROM asset WHERE sha256 = ?1", &[s(&sha)])?.into_iter().next()))? {
            return Ok(found);
        }
        let dir = paths::media_dir();
        std::fs::create_dir_all(&dir)?;
        let file = format!("{}.{ext}", &sha[..32]);
        std::fs::write(dir.join(&file), bytes)?;
        let id = self.write(by, |tx| {
            let id = ids::new_id();
            tx.execute(
                "INSERT INTO asset (id, sha256, mime, bytes, file, source_url, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, sha, mime, bytes.len() as i64, file, source_url, tx.now()],
            )?;
            tx.record("asset", &id, ChangeOp::Upsert)?;
            Ok(id)
        })?;
        self.read(|c| row(c, "SELECT * FROM asset WHERE id = ?1", &[s(&id)], "asset", &id))
    }

    /// The file and type of an asset, for serving it.
    pub fn mc_asset_file(&self, id: &str) -> Result<(PathBuf, String)> {
        let (file, mime): (String, String) = self.read(|c| {
            c.query_row("SELECT file, mime FROM asset WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|_| not_found("asset", id))
        })?;
        Ok((paths::media_dir().join(file), mime))
    }
}

// ---------------------------------------------------------------------------------------------
// Inside a transaction (shared with proposals)

pub(crate) fn insert_node(tx: &Tx<'_>, n: &NewNode) -> Result<String> {
    one_of("space", &n.space, super::SPACES)?;
    let kind = n.kind.clone().unwrap_or_else(|| "text".into());
    one_of("kind", &kind, &["page", "text", "image", "link", "divider", "opaque"])?;
    let parent = n.parent_id.as_deref().map(|p| resolve(tx, "mc_node", p)).transpose()?;
    let rank = match &parent {
        Some(p) => rank_last(tx, "SELECT max(rank) FROM mc_node WHERE parent_id = ?1", &[p])?,
        None => rank_last(tx, "SELECT max(rank) FROM mc_node WHERE parent_id IS NULL AND space = ?1", &[&n.space])?,
    };
    let id = n.id.clone().unwrap_or_else(ids::new_id);
    let style = n.style.clone().unwrap_or_else(|| if kind == "page" { "card".into() } else { "body".into() });
    tx.execute(
        "INSERT INTO mc_node (id, space, parent_id, kind, style, text, context, url, rank, asset_id, origin, sync_scope, sensitive, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
        params![
            id, n.space, parent, kind, style, n.text, n.context, n.url, rank, n.asset_id,
            origin_of(tx.actor()), if n.local_only { "local" } else { "craft" }, n.sensitive as i64, tx.now()
        ],
    )?;
    tx.record("mc_node", &id, ChangeOp::Upsert)?;
    Ok(id)
}

pub(crate) fn supersede_node(tx: &Tx<'_>, id: &str, text: &str) -> Result<String> {
    let id = resolve(tx, "mc_node", id)?;
    let old = row(tx, "SELECT * FROM mc_node WHERE id = ?1", &[s(&id)], "mc_node", &id)?;
    if !old["superseded_by"].is_null() {
        return Err(invalid("this block was already replaced; edit the newer one"));
    }
    let new_id = ids::new_id();
    // Right after the old block among its siblings.
    let next: Option<String> = tx.query_row(
        "SELECT min(rank) FROM mc_node WHERE parent_id IS ?1 AND space = ?2 AND rank > ?3",
        params![old["parent_id"].as_str(), old["space"].as_str(), old["rank"].as_str()],
        |r| r.get(0),
    )?;
    let rank = super::rank_between(old["rank"].as_str().unwrap_or(""), next.as_deref());
    tx.execute(
        "INSERT INTO mc_node (id, space, parent_id, kind, style, text, context, url, rank, asset_id, origin, sync_scope, sensitive, meta, created_at, updated_at)
         SELECT ?2, space, parent_id, kind, style, ?3, context, url, ?4, asset_id, ?5, sync_scope, sensitive, meta, ?6, ?6 FROM mc_node WHERE id = ?1",
        params![id, new_id, text, rank, origin_of(tx.actor()), tx.now()],
    )?;
    tx.execute("UPDATE mc_node SET superseded_by = ?2, closed = 1, updated_at = ?3 WHERE id = ?1", params![id, new_id, tx.now()])?;
    // Cards and dreams follow the new block.
    tx.execute("UPDATE board_item SET ref_id = ?2 WHERE ref_kind = 'mc_node' AND ref_id = ?1", params![id, new_id])?;
    tx.execute("UPDATE dream SET node_id = ?2 WHERE node_id = ?1", params![id, new_id])?;
    log_event(tx, "mc_node", &id, &json!({ "text": old["text"] }), &json!({ "superseded_by": new_id, "text": text }))?;
    tx.record("mc_node", &id, ChangeOp::Upsert)?;
    tx.record("mc_node", &new_id, ChangeOp::Upsert)?;
    Ok(new_id)
}

fn insert_board(tx: &Tx<'_>, b: &NewBoard) -> Result<String> {
    if b.title.trim().is_empty() {
        return Err(invalid("a board needs a title"));
    }
    let kind = b.kind.clone().unwrap_or_else(|| "viz".into());
    one_of("kind", &kind, &["free", "viz", "project"])?;
    let parent = b.parent_board_id.as_deref().map(|p| resolve(tx, "board", p)).transpose()?;
    // A viz board is a page block in the Visualization tree (nested under its parent's page).
    let node_id = if kind == "viz" {
        let parent_node: Option<String> = match &parent {
            Some(p) => tx.query_row("SELECT node_id FROM board WHERE id = ?1", [p], |r| r.get(0))?,
            None => None,
        };
        Some(insert_node(tx, &NewNode {
            space: "viz".into(),
            parent_id: parent_node,
            kind: Some("page".into()),
            text: b.title.clone(),
            sensitive: b.sensitive,
            ..Default::default()
        })?)
    } else {
        None
    };
    let rank = match &parent {
        Some(p) => rank_last(tx, "SELECT max(rank) FROM board WHERE parent_board_id = ?1", &[p])?,
        None => rank_last(tx, "SELECT max(rank) FROM board WHERE parent_board_id IS NULL", &[])?,
    };
    let id = b.id.clone().unwrap_or_else(ids::new_id);
    tx.execute(
        "INSERT INTO board (id, title, kind, parent_board_id, node_id, accent, sensitive, rank, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![id, b.title.trim(), kind, parent, node_id, b.accent, b.sensitive as i64, rank, tx.now()],
    )?;
    tx.record("board", &id, ChangeOp::Upsert)?;
    Ok(id)
}

pub(crate) fn insert_item(tx: &Tx<'_>, n: &NewItem) -> Result<String> {
    one_of("kind", &n.kind, &["image", "text", "link", "goal", "board"])?;
    let board_id = resolve(tx, "board", &n.board_id)?;
    let (bkind, page): (String, Option<String>) =
        tx.query_row("SELECT kind, node_id FROM board WHERE id = ?1", [&board_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let space = "viz".to_string();
    let (ref_kind, ref_id): (&str, String) = match n.kind.as_str() {
        "goal" => {
            let g = n.goal_id.as_deref().ok_or_else(|| invalid("a goal card needs goal_id"))?;
            ("mc_goal", resolve(tx, "mc_goal", g)?)
        }
        "board" => {
            let title = if n.caption.trim().is_empty() { "Untitled".to_string() } else { n.caption.clone() };
            let child = insert_board(tx, &NewBoard {
                title,
                kind: Some(bkind.clone()),
                parent_board_id: Some(board_id.clone()),
                sensitive: n.sensitive,
                ..Default::default()
            })?;
            ("board", child)
        }
        kind => {
            if kind == "image" && n.asset_id.is_none() && n.url.is_none() {
                return Err(invalid("an image card needs asset_id (upload first) or url"));
            }
            if kind == "link" && n.url.as_deref().is_none_or(|u| u.trim().is_empty()) {
                return Err(invalid("a link card needs url"));
            }
            if kind == "text" && n.caption.trim().is_empty() {
                return Err(invalid("a text card needs text"));
            }
            let nid = insert_node(tx, &NewNode {
                space: space.clone(),
                parent_id: page.clone(),
                kind: Some(kind.into()),
                text: n.caption.clone(),
                context: n.context.clone(),
                url: n.url.clone(),
                asset_id: n.asset_id.clone(),
                sensitive: n.sensitive,
                ..Default::default()
            })?;
            ("mc_node", nid)
        }
    };
    let (dw, dh) = match n.kind.as_str() {
        "image" => (260.0, 340.0),
        "text" => (240.0, 160.0),
        "link" | "goal" => (260.0, 150.0),
        _ => (240.0, 200.0),
    };
    // New cards land to the right of what is there, on one row, unless placed.
    let (max_x, max_z): (Option<f64>, Option<i64>) = tx.query_row(
        "SELECT max(x + w), max(z) FROM board_item WHERE board_id = ?1 AND deleted_at IS NULL",
        [&board_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let id = n.id.clone().unwrap_or_else(ids::new_id);
    let play: i64 = tx.query_row("SELECT count(*) FROM board_item WHERE board_id = ?1", [&board_id], |r| r.get(0))?;
    tx.execute(
        "INSERT INTO board_item (id, board_id, kind, ref_kind, ref_id, x, y, w, h, z, color, play_order, sensitive, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
        params![
            id, board_id, n.kind, ref_kind, ref_id,
            n.x.unwrap_or_else(|| max_x.map(|x| x + 32.0).unwrap_or(0.0)), n.y.unwrap_or(0.0),
            n.w.unwrap_or(dw), n.h.unwrap_or(dh), max_z.unwrap_or(0) + 1, n.color, play, n.sensitive as i64, tx.now()
        ],
    )?;
    tx.record("board_item", &id, ChangeOp::Upsert)?;
    // Every card on a Visualization board is a dream with a path (subpages use their page block).
    if bkind == "viz" && n.kind != "goal" {
        let nid = card_node(tx, &json!({ "kind": n.kind, "ref_kind": ref_kind, "ref_id": ref_id }))?;
        if let Some(nid) = nid {
            let mut p = Map::new();
            if !n.path.is_empty() {
                p = serde_json::to_value(&n.path)?.as_object().cloned().unwrap_or_default();
                p.retain(|_, v| !v.is_null());
            }
            set_path(tx, &nid, &p)?;
        }
    }
    Ok(id)
}

/// The block a card shows: its own node, or the page block of a nested board.
fn card_node(tx: &Tx<'_>, it: &Value) -> Result<Option<String>> {
    Ok(match it["ref_kind"].as_str() {
        Some("mc_node") => it["ref_id"].as_str().map(String::from),
        Some("board") => tx.query_row("SELECT node_id FROM board WHERE id = ?1", [it["ref_id"].as_str()], |r| r.get(0))?,
        _ => None,
    })
}

/// Create or change a dream's path, then recompute its stage (unless reached or dropped).
pub(crate) fn set_path(tx: &Tx<'_>, node_id: &str, p: &Map<String, Value>) -> Result<()> {
    tx.execute("INSERT OR IGNORE INTO dream (node_id, last_touched_at) VALUES (?1, ?2)", params![node_id, tx.now()])?;
    for (k, v) in p {
        let col = k.as_str();
        if !(PATH_FIELDS.contains(&col) || col == "stage" || col == "closed_reason") {
            return Err(invalid(format!("dream: unknown field '{col}'")));
        }
        if col == "stage" {
            one_of("stage", v.as_str().unwrap_or(""), &["seed", "rooted", "moving", "reached", "dropped"])?;
        }
        let val = v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        tx.execute(&format!("UPDATE dream SET {col} = ?2, last_touched_at = ?3 WHERE node_id = ?1"), params![node_id, val, tx.now()])?;
    }
    let d = row(tx, "SELECT * FROM dream WHERE node_id = ?1", &[s(node_id)], "dream", node_id)?;
    if !p.contains_key("stage") && !matches!(d["stage"].as_str(), Some("reached" | "dropped" | "moving")) {
        let st = stage_for(filled(&d["why"]), filled(&d["obstacle"]), filled(&d["if_then"]), filled(&d["first_step"]));
        tx.execute("UPDATE dream SET stage = ?2 WHERE node_id = ?1", params![node_id, st])?;
    }
    tx.record("dream", node_id, ChangeOp::Upsert)?;
    Ok(())
}

const BOARD_SELECT: &str = "SELECT b.*,
    (SELECT count(*) FROM board_item i WHERE i.board_id = b.id AND i.deleted_at IS NULL) AS item_count,
    (SELECT count(*) FROM board c WHERE c.parent_board_id = b.id AND c.deleted_at IS NULL) AS child_count,
    coalesce(b.cover_asset, (SELECT n.asset_id FROM board_item i JOIN mc_node n ON n.id = i.ref_id AND i.ref_kind = 'mc_node'
        WHERE i.board_id = b.id AND i.deleted_at IS NULL AND n.asset_id IS NOT NULL ORDER BY i.z DESC LIMIT 1)) AS cover,
    (SELECT n.url FROM board_item i JOIN mc_node n ON n.id = i.ref_id AND i.ref_kind = 'mc_node'
        WHERE i.board_id = b.id AND i.deleted_at IS NULL AND n.kind = 'image' AND n.asset_id IS NULL AND n.url IS NOT NULL LIMIT 1) AS cover_url
    FROM board b";
const BOARD_SELECT_ONE: &str = "SELECT b.*,
    coalesce(b.cover_asset, (SELECT n.asset_id FROM board_item i JOIN mc_node n ON n.id = i.ref_id AND i.ref_kind = 'mc_node'
        WHERE i.board_id = b.id AND i.deleted_at IS NULL AND n.asset_id IS NOT NULL ORDER BY i.z DESC LIMIT 1)) AS cover,
    (SELECT text FROM mc_node WHERE id = b.node_id) AS page_text
    FROM board b WHERE b.id = ?1";

/// Card rows with what they show. `node_*` from the block (for a subpage: the nested board's
/// page block), the dream path, and for goal cards the goal's title and status.
const ITEM_SELECT: &str = "SELECT i.*,
    coalesce(n.id, bn.id) AS node_id, coalesce(n.kind, 'page') AS node_kind,
    CASE WHEN i.kind = 'goal' THEN g.title ELSE coalesce(n.text, cb.title) END AS caption,
    coalesce(n.context, bn.context) AS context, n.url AS url, n.asset_id AS asset_id,
    coalesce(n.sensitive, bn.sensitive, 0) AS node_sensitive, coalesce(n.verified_at, bn.verified_at) AS verified_at,
    d.why, d.outcome, d.obstacle, d.if_then, d.first_step, d.stage, d.closed_reason, d.last_touched_at,
    g.status AS goal_status, g.horizon AS goal_horizon, g.section AS goal_section,
    (SELECT count(*) FROM board_item ci WHERE ci.board_id = cb.id AND ci.deleted_at IS NULL) AS child_items,
    (SELECT cn.asset_id FROM board_item ci JOIN mc_node cn ON cn.id = ci.ref_id AND ci.ref_kind = 'mc_node'
        WHERE ci.board_id = cb.id AND ci.deleted_at IS NULL AND cn.asset_id IS NOT NULL ORDER BY ci.z DESC LIMIT 1) AS child_cover
  FROM board_item i
  LEFT JOIN mc_node n ON i.ref_kind = 'mc_node' AND n.id = i.ref_id
  LEFT JOIN board cb ON i.ref_kind = 'board' AND cb.id = i.ref_id
  LEFT JOIN mc_node bn ON bn.id = cb.node_id
  LEFT JOIN dream d ON d.node_id = coalesce(n.id, bn.id)
  LEFT JOIN mc_goal g ON i.ref_kind = 'mc_goal' AND g.id = i.ref_id
  WHERE i.board_id = ?1 AND i.deleted_at IS NULL
  ORDER BY i.z";

