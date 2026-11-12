//! Mind OS tools: the library of saved things (links, images, notes, quotes, reels, people,
//! books, decisions, learnings). `mind.save` / `mind_save` both work.
//!
//! Rule for agents (Nora): save and link freely, append dated notes with `mind_append`. His
//! words (`raw`) are never rewritten; a change in meaning is `mind_supersede`, only when he asks.

use aos_core::mind::links::NewLink;
use aos_core::mind::{MindQuery, NewMindItem};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mission::{IdArg, NoArgs, Patch};
use crate::registry::{Risk, Tool, ToolCtx, ToolError};

type R = Result<Value, ToolError>;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new("mind_save", "Save something to his Mind library: text (a note, quote, thought; [[Title]] links to another item), a link (article, reel, video; the server fetches title, image and text) or an uploaded image (asset_id). Kind is guessed when left out: link, image, note, quote, video, file, person, book, decision, learning, daily. Claude adds a description and tags later. Nora: save what he asks you to remember from a chat or call.", Risk::Free, save).positional(&["text"]),
        Tool::new("mind_search", "Search the Mind library by words (title, his words, description, tags, page text, site) and #tags; empty query lists the newest. Returns cards with a snippet of why each matched.", Risk::Free, search).read_only().positional(&["query"]),
        Tool::new("mind_read", "One Mind item in full: his words verbatim, the summary and tags, Nora's notes, links out, backlinks and the boards it is on.", Risk::Free, read).read_only().positional(&["id"]),
        Tool::new("mind_link", "Link a Mind item to another item (to_id, or to_title), or to a board, mc_goal, dream, project, task or journal entry (to_kind + to_id). rel: mentions (default), about, part_of, learned_from, supersedes, contradicts, supports.", Risk::Free, link).positional(&["from_id", "to_id"]),
        Tool::new("mind_unlink", "Remove one link (edge id from mind_read).", Risk::Free, unlink).positional(&["edge_id"]),
        Tool::new("mind_append", "Add a dated note to a Mind item (what he said later, Nora's read). His own words stay untouched.", Risk::Free, append).positional(&["id", "text"]),
        Tool::new("mind_update", "Change a Mind item's title, kind, pinned, archived, sensitive, inbox (false = filed) or happened_on, or tags (tags_add, tags_remove). His words can't be changed here.", Risk::Free, update).positional(&["id"]),
        Tool::new("mind_supersede", "Replace an item with new words (a changed decision or fact): the old one is kept, closed, and points at the new one. Only when he asks.", Risk::Free, supersede).positional(&["id", "text"]),
        Tool::new("mind_board_add", "Put a Mind item on a board as a card that references it.", Risk::Free, board_add).positional(&["id", "board_id"]),
        Tool::new("mind_stats", "Mind library counts (items, inbox, being read, failed) and the tags in use.", Risk::Free, stats).read_only(),
        Tool::new("mind_retag", "Queue Claude to read items again: those never read, or all with all=true.", Risk::Free, retag),
        Tool::new("mind_delete", "Remove a Mind item (soft delete; links to it show as removed).", Risk::Ask, delete).positional(&["id"]),
    ]
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveArgs {
    /// Client-made id (optional).
    pub id: Option<String>,
    /// His words or the shared text, kept verbatim. A lone link becomes a link card.
    #[serde(default)]
    pub text: String,
    pub url: Option<String>,
    pub title: Option<String>,
    /// link, image, note, quote, video, file, person, book, decision, learning, daily.
    pub kind: Option<String>,
    /// From POST /api/media (images).
    pub asset_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sensitive: bool,
    /// capture, voice, paste, share, api, nora, board (default app, or nora for Nora).
    pub origin: Option<String>,
    /// The day it is about (YYYY-MM-DD).
    pub happened_on: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

fn save(ctx: &ToolCtx<'_>, a: SaveArgs) -> R {
    let origin = a.origin.or_else(|| ctx.actor.to_ascii_lowercase().contains("nora").then(|| "nora".to_string()));
    Ok(ctx.store.mind_save(
        ctx.actor,
        NewMindItem {
            id: a.id,
            kind: a.kind,
            raw: a.text,
            title: a.title,
            url: a.url,
            asset_id: a.asset_id,
            origin,
            sensitive: a.sensitive,
            happened_on: a.happened_on,
            tags: a.tags,
            width: a.width,
            height: a.height,
        },
    )?)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// Words and #tags. Empty lists the newest.
    #[serde(default)]
    pub query: String,
    pub kind: Option<String>,
    pub tag: Option<String>,
    /// Only items not yet filed.
    #[serde(default)]
    pub inbox: bool,
    /// Include archived and superseded items.
    #[serde(default)]
    pub archived: bool,
    /// Leave sensitive items out (Safe mode).
    #[serde(default)]
    pub safe: bool,
    pub limit: Option<usize>,
    /// Items saved before this time (next page of the grid).
    pub before: Option<String>,
    /// text (default) or visual. Visual search (local CLIP) is not built yet and falls back to text.
    pub mode: Option<String>,
}

fn search(ctx: &ToolCtx<'_>, a: SearchArgs) -> R {
    let visual = a.mode.as_deref() == Some("visual");
    let items = ctx.store.mind_search(&MindQuery {
        text: a.query,
        kind: a.kind,
        tag: a.tag,
        inbox: a.inbox,
        archived: a.archived,
        safe: a.safe,
        limit: a.limit,
        before: a.before,
    })?;
    let mut out = json!({ "items": items, "mode": "text" });
    if visual {
        out["note"] = json!("visual search is not built yet; these are text results");
    }
    Ok(out)
}

fn read(ctx: &ToolCtx<'_>, a: IdArg) -> R {
    Ok(ctx.store.mind_get(&a.id)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkArgs {
    pub from_id: String,
    pub to_id: Option<String>,
    /// Link to a Mind item by title (resolves later if it doesn't exist yet).
    pub to_title: Option<String>,
    /// mind_item (default), board, mc_goal, dream, project, task, journal.
    pub to_kind: Option<String>,
    pub rel: Option<String>,
}

fn link(ctx: &ToolCtx<'_>, a: LinkArgs) -> R {
    Ok(ctx.store.mind_link(ctx.actor, NewLink { from_id: a.from_id, to_kind: a.to_kind, to_id: a.to_id, to_title: a.to_title, rel: a.rel })?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgeArg {
    pub edge_id: String,
}

fn unlink(ctx: &ToolCtx<'_>, a: EdgeArg) -> R {
    ctx.store.mind_unlink(ctx.actor, &a.edge_id)?;
    Ok(json!({ "ok": true }))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextArgs {
    pub id: String,
    pub text: String,
}

fn append(ctx: &ToolCtx<'_>, a: TextArgs) -> R {
    Ok(ctx.store.mind_append(ctx.actor, &a.id, &a.text)?)
}

fn update(ctx: &ToolCtx<'_>, a: Patch) -> R {
    Ok(ctx.store.mind_update(ctx.actor, &a.id, &a.fields)?)
}

fn supersede(ctx: &ToolCtx<'_>, a: TextArgs) -> R {
    Ok(ctx.store.mind_supersede(ctx.actor, &a.id, &a.text)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoardArgs {
    pub id: String,
    pub board_id: String,
}

fn board_add(ctx: &ToolCtx<'_>, a: BoardArgs) -> R {
    Ok(ctx.store.mind_add_to_board(ctx.actor, &a.id, &a.board_id)?)
}

fn stats(ctx: &ToolCtx<'_>, _: NoArgs) -> R {
    let mut s = ctx.store.mind_stats()?;
    s["tags"] = json!(ctx.store.mind_tags(40)?);
    Ok(s)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetagArgs {
    #[serde(default)]
    pub all: bool,
}

fn retag(ctx: &ToolCtx<'_>, a: RetagArgs) -> R {
    Ok(json!({ "queued": ctx.store.mind_retag(ctx.actor, a.all)? }))
}

fn delete(ctx: &ToolCtx<'_>, a: IdArg) -> R {
    ctx.store.mind_delete(ctx.actor, &a.id)?;
    Ok(json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use aos_core::Store;
    use serde_json::json;

    use crate::registry::ToolCtx;

    #[test]
    fn nora_saves_searches_reads_and_links() {
        let st = Store::open_temporary().unwrap();
        let reg = crate::registry();
        let ctx = ToolCtx::new(&st, "nora");
        let a = reg.call(&ctx, "mind.save", json!({ "text": "Book: Atomic Habits, small changes add up" , "kind": "book" })).unwrap();
        assert_eq!(a["origin"], "nora");
        let b = reg.call(&ctx, "mind_save", json!({ "text": "Habit idea from [[Book: Atomic Habits, small changes add up]]" })).unwrap();
        assert_eq!(b["links"][0]["to_id"], a["id"]);
        let hits = reg.call(&ctx, "mind.search", json!({ "query": "atomic" })).unwrap();
        assert_eq!(hits["items"].as_array().unwrap().len(), 2);
        let v = reg.call(&ctx, "mind_search", json!({ "query": "habits", "mode": "visual" })).unwrap();
        assert!(v["note"].is_string());
        reg.call(&ctx, "mind_append", json!({ "id": a["id"], "text": "He finished chapter 3." })).unwrap();
        let r = reg.call(&ctx, "mind.read", json!({ "id": a["id"] })).unwrap();
        assert_eq!(r["backlinks"][0]["from_id"], b["id"]);
        assert_eq!(r["notes"][0]["text"], "He finished chapter 3.");
        let e = reg.call(&ctx, "mind.link", json!({ "from_id": b["id"], "to_title": "Later page" })).unwrap();
        assert!(e["to_id"].is_null());
        assert!(reg.call(&ctx, "mind_update", json!({ "id": a["id"], "raw": "x" })).is_err());
    }
}
