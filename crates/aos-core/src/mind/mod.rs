//! Mind OS: the library of saved things (design/specs/mind-os.md).
//!
//! - Every saved thing is one `mind_item`: a link, image, note, quote, video link, file, or a
//!   Life-brain note (person, book, decision, learning, daily). Boards place references to items.
//! - `raw` holds the owner's words (or the shared text) verbatim and is never rewritten. Claude's
//!   reading (title, description, summary, tags) sits beside it; Nora's additions are dated
//!   notes appended to the item. A change in meaning makes a new item that supersedes the old.
//! - `[[Title]]` in the raw text links to the item with that title; a link to a title that does
//!   not exist yet resolves when such an item is saved. Backlinks are the same edges read back.
//! - Enrichment is a queue (`mind_job`): fetch (links) -> thumb (images) -> tag (Claude, low
//!   priority, never for sensitive items). The workers live in the server and Nora; this module
//!   only hands out jobs and stores their results.
//! - Search is SQLite FTS5 over title, raw text, Claude's reading, tags, page text and source.
//!
//! Submodules: [`links`] (wiki links, edges, backlinks), [`jobs`] (the enrichment queue).

pub mod jobs;
pub mod links;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::{Result, invalid, not_found};
use crate::mission::{patch_row, row, rows, s};
use crate::store::{ChangeOp, Store, Tx};
use crate::ids;

/// The Mind OS tables. Idempotent, run on every open.
pub const SCHEMA: &str = include_str!("../../migrations/mind.sql");

pub fn ensure_schema(c: &Connection) -> Result<()> {
    c.execute_batch(SCHEMA)?;
    Ok(())
}

pub const KINDS: &[&str] = &["link", "image", "note", "quote", "video", "file", "person", "book", "decision", "learning", "daily"];
pub const ORIGINS: &[&str] = &["app", "capture", "voice", "paste", "share", "api", "nora", "board", "import"];

/// A thing to save. Only one of `raw`, `url` or `asset_id` is needed.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewMindItem {
    pub id: Option<String>,
    /// Guessed from the content when missing.
    pub kind: Option<String>,
    /// His words or the shared text, kept verbatim.
    #[serde(default)]
    pub raw: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub asset_id: Option<String>,
    pub origin: Option<String>,
    #[serde(default)]
    pub sensitive: bool,
    pub happened_on: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Image size, when the client knows it.
    pub width: Option<i64>,
    pub height: Option<i64>,
}

/// Filters for the library grid and search.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MindQuery {
    /// Words; `#tag` filters by tag. Empty lists the newest.
    #[serde(default)]
    pub text: String,
    pub kind: Option<String>,
    pub tag: Option<String>,
    #[serde(default)]
    pub inbox: bool,
    #[serde(default)]
    pub archived: bool,
    /// Leave sensitive items out (Safe mode).
    #[serde(default)]
    pub safe: bool,
    pub limit: Option<usize>,
    /// Items saved before this time (paging the grid).
    pub before: Option<String>,
}

const UPDATE_COLS: &[&str] = &["title", "kind", "pinned", "archived", "sensitive", "inbox", "happened_on", "color", "source_url"];

/// Card columns for grids and search results.
const CARD_COLS: &str = "i.id, i.kind, i.title, i.description, substr(i.raw, 1, 400) AS raw, i.source_url, i.source_domain,
    i.site_name, i.image_url, i.asset_id, i.thumb_asset_id, i.width, i.height, i.color, i.status, i.inbox, i.pinned,
    i.sensitive, i.archived, i.superseded_by, i.origin, i.happened_on, i.saved_at,
    (SELECT group_concat(t.tag, ',') FROM mind_tag t WHERE t.item_id = i.id AND t.removed = 0) AS tags";

/// Is this text one http(s) link and nothing else?
pub fn lone_url(text: &str) -> Option<String> {
    let t = text.trim();
    let ok = (t.starts_with("https://") || t.starts_with("http://")) && !t.chars().any(char::is_whitespace) && t.len() > 10;
    ok.then(|| t.to_string())
}

/// `www.example.com` -> `example.com`.
pub fn domain_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split(['/', '?', '#']).next()?.split('@').next_back()?.split(':').next()?;
    let host = host.trim_start_matches("www.").to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Video and reel hosts: a link there is a video card.
pub fn is_video_url(url: &str) -> bool {
    let d = domain_of(url).unwrap_or_default();
    let lower = url.to_ascii_lowercase();
    matches!(d.as_str(), "youtube.com" | "m.youtube.com" | "youtu.be" | "vimeo.com" | "tiktok.com" | "vm.tiktok.com")
        || (d.ends_with("instagram.com") && (lower.contains("/reel") || lower.contains("/tv/")))
        || ((d == "x.com" || d == "twitter.com") && lower.contains("/video/"))
}

/// The kind of a new item from what was given.
pub fn guess_kind(raw: &str, url: Option<&str>, has_asset: bool) -> &'static str {
    if has_asset {
        return "image";
    }
    if let Some(u) = url.map(str::to_string).or_else(|| lone_url(raw)) {
        let path = u.to_ascii_lowercase();
        let path = path.split(['?', '#']).next().unwrap_or("");
        if [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"].iter().any(|e| path.ends_with(e)) {
            return "image";
        }
        return if is_video_url(&u) { "video" } else { "link" };
    }
    let t = raw.trim();
    let quoted = (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\u{201C}') && t.ends_with('\u{201D}'));
    if quoted && t.chars().count() > 2 { "quote" } else { "note" }
}

/// A title from the first line of a note: at most 80 characters, no heading marks.
pub fn first_line_title(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.trim_start_matches('#').trim();
    if line.is_empty() || lone_url(line).is_some() {
        return None;
    }
    let mut t: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        t = t.trim_end().to_string() + "…";
    }
    Some(t)
}

pub(crate) fn clean_tag(t: &str) -> Option<String> {
    let t = t.trim().trim_start_matches('#').trim().to_lowercase();
    let t: String = t.chars().map(|c| if c.is_whitespace() { '-' } else { c }).filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
    let t = t.trim_matches('-').to_string();
    (!t.is_empty() && t.chars().count() <= 40).then_some(t)
}

/// Turn query words into an FTS5 match: every word must appear, as a prefix.
fn fts_query(words: &[String]) -> String {
    words
        .iter()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect::<String>())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Store {
    /// Save a thing to the library. Instant: enrichment runs later through the job queue.
    pub fn mind_save(&self, by: &str, n: NewMindItem) -> Result<Value> {
        let id = self.write(by, |tx| insert_item(tx, &n))?;
        self.mind_get(&id)
    }

    /// One item with tags, Nora's notes, links out, backlinks and the boards it sits on.
    pub fn mind_get(&self, id: &str) -> Result<Value> {
        self.read(|c| {
            let id = crate::store::resolve_prefix(c, "mind_item", id)?;
            let mut item = row(c, "SELECT * FROM mind_item WHERE id = ?1", &[s(&id)], "mind_item", &id)?;
            item["tags"] = json!(tags_of(c, &id)?);
            item["notes"] = json!(rows(c, "SELECT id, text, by, at FROM mind_note WHERE item_id = ?1 ORDER BY at", &[s(&id)])?);
            item["links"] = json!(links::outgoing(c, &id)?);
            item["backlinks"] = json!(links::backlinks(c, &id)?);
            item["boards"] = json!(rows(
                c,
                "SELECT bi.id AS card_id, b.id AS board_id, b.title FROM board_item bi JOIN board b ON b.id = bi.board_id
                 WHERE bi.ref_kind = 'mind_item' AND bi.ref_id = ?1 AND bi.deleted_at IS NULL AND b.deleted_at IS NULL",
                &[s(&id)]
            )?);
            item["jobs"] = json!(rows(c, "SELECT stage, state, tries, error FROM mind_job WHERE item_id = ?1", &[s(&id)])?);
            Ok(item)
        })
    }

    /// The library grid, or search results when `text` is given (best match first, with a
    /// snippet saying why it matched).
    pub fn mind_search(&self, q: &MindQuery) -> Result<Vec<Value>> {
        let limit = q.limit.unwrap_or(60).clamp(1, 500);
        let mut words = Vec::new();
        let mut tags: Vec<String> = q.tag.iter().filter_map(|t| clean_tag(t)).collect();
        for w in q.text.split_whitespace() {
            if let Some(t) = w.strip_prefix('#').and_then(clean_tag) {
                tags.push(t);
            } else {
                words.push(w.to_string());
            }
        }
        let fts = fts_query(&words);
        self.read(|c| {
            let mut sql = if fts.is_empty() {
                format!("SELECT {CARD_COLS}, NULL AS why FROM mind_item i WHERE i.deleted_at IS NULL")
            } else {
                format!(
                    "SELECT {CARD_COLS}, snippet(mind_fts, -1, '', '', '…', 10) AS why
                     FROM mind_fts f JOIN mind_item i ON i.id = f.item_id WHERE mind_fts MATCH ?1 AND i.deleted_at IS NULL"
                )
            };
            let mut args = Vec::new();
            if !fts.is_empty() {
                args.push(s(&fts));
            }
            if let Some(k) = q.kind.as_deref().filter(|k| !k.is_empty()) {
                args.push(s(k));
                sql.push_str(&format!(" AND i.kind = ?{}", args.len()));
            }
            for t in &tags {
                args.push(s(t));
                sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM mind_tag t WHERE t.item_id = i.id AND t.removed = 0 AND t.tag = ?{})", args.len()));
            }
            if q.inbox {
                sql.push_str(" AND i.inbox = 1");
            }
            if !q.archived {
                sql.push_str(" AND i.archived = 0 AND i.superseded_by IS NULL");
            }
            if q.safe {
                sql.push_str(" AND i.sensitive = 0");
            }
            if let Some(b) = q.before.as_deref().filter(|b| !b.is_empty()) {
                args.push(s(b));
                sql.push_str(&format!(" AND i.saved_at < ?{}", args.len()));
            }
            sql.push_str(if fts.is_empty() { " ORDER BY i.pinned DESC, i.saved_at DESC" } else { " ORDER BY bm25(mind_fts, 0, 8, 4, 3, 2, 6, 1, 2)" });
            sql.push_str(&format!(" LIMIT {limit}"));
            let mut out = rows(c, &sql, &args)?;
            for it in &mut out {
                let t = it["tags"].as_str().unwrap_or("").split(',').filter(|t| !t.is_empty()).map(String::from).collect::<Vec<_>>();
                it["tags"] = json!(t);
            }
            Ok(out)
        })
    }

    /// Counts for the Mind header: all, inbox, still being read, failed.
    pub fn mind_stats(&self) -> Result<Value> {
        self.read(|c| {
            let r = c.query_row(
                "SELECT count(*), coalesce(sum(inbox), 0), coalesce(sum(status IN ('saved', 'fetching', 'reading')), 0), coalesce(sum(status = 'failed'), 0)
                 FROM mind_item WHERE deleted_at IS NULL AND archived = 0 AND superseded_by IS NULL",
                [],
                |r| Ok(json!({ "items": r.get::<_, i64>(0)?, "inbox": r.get::<_, i64>(1)?, "reading": r.get::<_, i64>(2)?, "failed": r.get::<_, i64>(3)? })),
            )?;
            Ok(r)
        })
    }

    /// Every tag in use with its count, most used first (Claude gets these to reuse).
    pub fn mind_tags(&self, limit: usize) -> Result<Vec<Value>> {
        self.read(|c| {
            rows(
                c,
                &format!("SELECT tag, count(*) AS n FROM mind_tag t JOIN mind_item i ON i.id = t.item_id
                 WHERE t.removed = 0 AND i.deleted_at IS NULL GROUP BY tag ORDER BY n DESC, tag LIMIT {}", limit.clamp(1, 1000)),
                &[],
            )
        })
    }

    /// Change the item's own fields (title, kind, flags). `raw` is his words and can't be
    /// rewritten: append a note or supersede the item instead.
    pub fn mind_update(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        if patch.contains_key("raw") {
            return Err(invalid("his words are kept verbatim: use mind_append to add, or mind_supersede to replace"));
        }
        let mut p = patch.clone();
        let tags_add = p.remove("tags_add");
        let tags_remove = p.remove("tags_remove");
        if let Some(k) = p.get("kind").and_then(Value::as_str) {
            crate::mission::one_of("kind", k, KINDS)?;
        }
        let id = self.write(by, |tx| {
            let id = crate::store::resolve_prefix(tx, "mind_item", id)?;
            patch_row(tx, "mind_item", &id, UPDATE_COLS, &p)?;
            for t in tags_add.iter().flat_map(|v| v.as_array().cloned().unwrap_or_default()) {
                if let Some(t) = t.as_str().and_then(clean_tag) {
                    tx.execute(
                        "INSERT INTO mind_tag (item_id, tag, by, removed) VALUES (?1, ?2, 'owner', 0)
                         ON CONFLICT(item_id, tag) DO UPDATE SET removed = 0, by = 'owner'",
                        params![id, t],
                    )?;
                }
            }
            for t in tags_remove.iter().flat_map(|v| v.as_array().cloned().unwrap_or_default()) {
                if let Some(t) = t.as_str().and_then(clean_tag) {
                    tx.execute(
                        "INSERT INTO mind_tag (item_id, tag, by, removed) VALUES (?1, ?2, 'owner', 1)
                         ON CONFLICT(item_id, tag) DO UPDATE SET removed = 1, by = 'owner'",
                        params![id, t],
                    )?;
                }
            }
            if p.get("title").and_then(Value::as_str).is_some() {
                links::resolve_dangling(tx, &id)?;
            }
            fts_sync(tx, &id)?;
            tx.record("mind_item", &id, ChangeOp::Upsert)?;
            Ok(id)
        })?;
        self.mind_get(&id)
    }

    /// Append a dated note to an item (Nora's read, a later thought). Never touches `raw`.
    pub fn mind_append(&self, by: &str, id: &str, text: &str) -> Result<Value> {
        let text = text.trim();
        if text.is_empty() {
            return Err(invalid("nothing to append"));
        }
        let id = self.write(by, |tx| {
            let id = crate::store::resolve_prefix(tx, "mind_item", id)?;
            let who = if by.to_ascii_lowercase().contains("nora") { "nora" } else { "owner" };
            tx.execute(
                "INSERT INTO mind_note (id, item_id, text, by, at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![ids::new_id(), id, text, who, tx.now()],
            )?;
            links::link_wiki(tx, &id, text, who)?;
            tx.execute("UPDATE mind_item SET updated_at = ?2 WHERE id = ?1", params![id, tx.now()])?;
            fts_sync(tx, &id)?;
            tx.record("mind_item", &id, ChangeOp::Upsert)?;
            Ok(id)
        })?;
        self.mind_get(&id)
    }

    /// Replace an item with new words: a new item is saved, the old one stays (closed, pointing
    /// at the new one) and the new one links back with `supersedes`.
    pub fn mind_supersede(&self, by: &str, id: &str, raw: &str) -> Result<Value> {
        let new_id = self.write(by, |tx| {
            let old_id = crate::store::resolve_prefix(tx, "mind_item", id)?;
            let old = row(tx, "SELECT kind, title, source_url, sensitive, happened_on FROM mind_item WHERE id = ?1", &[s(&old_id)], "mind_item", &old_id)?;
            let new_id = insert_item(tx, &NewMindItem {
                kind: old["kind"].as_str().map(String::from),
                raw: raw.to_string(),
                title: None,
                url: old["source_url"].as_str().map(String::from),
                sensitive: old["sensitive"].as_i64() == Some(1),
                happened_on: old["happened_on"].as_str().map(String::from),
                origin: Some(if by.to_ascii_lowercase().contains("nora") { "nora".into() } else { "app".into() }),
                ..Default::default()
            })?;
            tx.execute("UPDATE mind_item SET superseded_by = ?2, updated_at = ?3 WHERE id = ?1", params![old_id, new_id, tx.now()])?;
            tx.execute(
                "INSERT INTO mind_edge (id, from_id, to_kind, to_id, rel, by, created_at) VALUES (?1, ?2, 'mind_item', ?3, 'supersedes', 'owner', ?4)",
                params![ids::new_id(), new_id, old_id, tx.now()],
            )?;
            tx.record("mind_item", &old_id, ChangeOp::Upsert)?;
            Ok(new_id)
        })?;
        self.mind_get(&new_id)
    }

    /// Soft delete (the row stays; links to it show as removed).
    pub fn mind_delete(&self, by: &str, id: &str) -> Result<()> {
        self.write(by, |tx| {
            let id = crate::store::resolve_prefix(tx, "mind_item", id)?;
            tx.execute("UPDATE mind_item SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1", params![id, tx.now()])?;
            tx.execute("DELETE FROM mind_fts WHERE item_id = ?1", [&id])?;
            tx.record("mind_item", &id, ChangeOp::Delete)?;
            Ok(())
        })
    }

    /// Place an item on a board as a card that references it (no copy).
    pub fn mind_add_to_board(&self, by: &str, id: &str, board_id: &str) -> Result<Value> {
        let (item_id, kind, w, h) = self.read(|c| {
            let id = crate::store::resolve_prefix(c, "mind_item", id)?;
            let it = row(c, "SELECT kind, asset_id, thumb_asset_id, width, height FROM mind_item WHERE id = ?1", &[s(&id)], "mind_item", &id)?;
            let has_img = it["thumb_asset_id"].is_string() || it["asset_id"].is_string();
            let kind = match it["kind"].as_str() {
                Some("image") => "image",
                Some("link" | "video") if has_img => "image",
                Some("link" | "video") => "link",
                _ => "text",
            };
            let (w, h) = match (it["width"].as_f64(), it["height"].as_f64()) {
                (Some(w), Some(h)) if kind == "image" && w > 0.0 => (260.0, (260.0 * h / w).clamp(160.0, 460.0)),
                _ if kind == "image" => (260.0, 320.0),
                _ => (240.0, 160.0),
            };
            Ok((id, kind, w, h))
        })?;
        let card = self.mc_add_item(by, crate::mission::boards::NewItem {
            board_id: board_id.to_string(),
            kind: kind.to_string(),
            mind_item_id: Some(item_id.clone()),
            w: Some(w),
            h: Some(h),
            ..Default::default()
        })?;
        self.write(by, |tx| {
            let board = card["board_id"].as_str().unwrap_or(board_id).to_string();
            let exists: Option<String> = tx
                .query_row("SELECT id FROM mind_edge WHERE from_id = ?1 AND to_kind = 'board' AND to_id = ?2", params![item_id, board], |r| r.get(0))
                .optional()?;
            if exists.is_none() {
                tx.execute(
                    "INSERT INTO mind_edge (id, from_id, to_kind, to_id, rel, by, created_at) VALUES (?1, ?2, 'board', ?3, 'part_of', 'owner', ?4)",
                    params![ids::new_id(), item_id, board, tx.now()],
                )?;
            }
            Ok(())
        })?;
        Ok(card)
    }
}

fn tags_of(c: &Connection, id: &str) -> Result<Vec<Value>> {
    rows(c, "SELECT tag, by FROM mind_tag WHERE item_id = ?1 AND removed = 0 ORDER BY by DESC, tag", &[s(id)])
}

pub(crate) fn insert_item(tx: &Tx<'_>, n: &NewMindItem) -> Result<String> {
    let raw = n.raw.clone();
    let url = n.url.clone().filter(|u| !u.trim().is_empty()).map(|u| u.trim().to_string()).or_else(|| lone_url(&raw));
    if raw.trim().is_empty() && url.is_none() && n.asset_id.is_none() {
        return Err(invalid("nothing to save: give text, a link or an image"));
    }
    if let Some(u) = &url
        && !(u.starts_with("https://") || u.starts_with("http://"))
    {
        return Err(invalid("links must start with http:// or https://"));
    }
    let kind = match n.kind.as_deref().filter(|k| !k.is_empty()) {
        Some(k) => {
            crate::mission::one_of("kind", k, KINDS)?;
            k.to_string()
        }
        None => guess_kind(&raw, url.as_deref(), n.asset_id.is_some()).to_string(),
    };
    let origin = n.origin.clone().unwrap_or_else(|| "app".into());
    crate::mission::one_of("origin", &origin, ORIGINS)?;
    let asset_id = match &n.asset_id {
        Some(a) => Some(
            tx.query_row("SELECT id FROM asset WHERE id = ?1", [a], |r| r.get::<_, String>(0))
                .optional()?
                .ok_or_else(|| not_found("asset", a))?,
        ),
        None => None,
    };
    let title = n
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| (!matches!(kind.as_str(), "link" | "video" | "image" | "quote")).then(|| first_line_title(&raw)).flatten());
    let id = n.id.clone().unwrap_or_else(ids::new_id);
    let domain = url.as_deref().and_then(domain_of);
    tx.execute(
        "INSERT INTO mind_item (id, kind, raw, title, source_url, source_domain, asset_id, width, height, happened_on, origin,
                                status, sensitive, saved_by, saved_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'saved', ?12, ?13, ?14, ?14)",
        params![id, kind, raw, title, url, domain, asset_id, n.width, n.height, n.happened_on, origin, n.sensitive as i64, tx.actor(), tx.now()],
    )?;
    for t in n.tags.iter().filter_map(|t| clean_tag(t)) {
        tx.execute("INSERT OR IGNORE INTO mind_tag (item_id, tag, by) VALUES (?1, ?2, 'owner')", params![id, t])?;
    }
    let who = if tx.actor().to_ascii_lowercase().contains("nora") { "nora" } else { "owner" };
    links::link_wiki(tx, &id, &raw, who)?;
    links::resolve_dangling(tx, &id)?;
    // First stage of enrichment.
    let first = if url.is_some() && kind != "image" {
        Some("fetch")
    } else if asset_id.is_some() || (kind == "image" && url.is_some()) {
        Some(if asset_id.is_some() { "thumb" } else { "fetch" })
    } else if !n.sensitive {
        Some("tag")
    } else {
        None
    };
    match first {
        Some(stage) => jobs::enqueue(tx, &id, stage)?,
        None => {
            tx.execute("UPDATE mind_item SET status = 'ready' WHERE id = ?1", [&id])?;
        }
    }
    fts_sync(tx, &id)?;
    tx.record("mind_item", &id, ChangeOp::Upsert)?;
    Ok(id)
}

/// Rewrite the item's search row from its current fields, tags and notes.
pub(crate) fn fts_sync(tx: &Tx<'_>, id: &str) -> Result<()> {
    tx.execute("DELETE FROM mind_fts WHERE item_id = ?1", [id])?;
    tx.execute(
        "INSERT INTO mind_fts (item_id, title, raw, description, summary, tags, body, source)
         SELECT i.id, coalesce(i.title, ''), i.raw, coalesce(i.description, ''), coalesce(i.summary, ''),
                coalesce((SELECT group_concat(t.tag, ' ') FROM mind_tag t WHERE t.item_id = i.id AND t.removed = 0), ''),
                coalesce(i.body_text, '') || ' ' || coalesce((SELECT group_concat(n.text, ' ') FROM mind_note n WHERE n.item_id = i.id), ''),
                coalesce(i.source_domain, '') || ' ' || coalesce(i.site_name, '') || ' ' || i.kind
         FROM mind_item i WHERE i.id = ?1 AND i.deleted_at IS NULL",
        [id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
