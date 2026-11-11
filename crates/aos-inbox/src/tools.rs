//! Inbox tools for the foundation registry: `inbox_list`, `inbox_mark`, `inbox_reply`,
//! `inbox_status` (`inbox.list` etc. resolve too).
//!
//! `inbox_reply` has risk `ask`. It never sends on a first call: it stores a draft and says so.
//! Only a second call with `confirm: true` queues the draft for sending, and only WhatsApp
//! replies can be sent for now.

use aos_core::Error;
use aos_tools::{Risk, Tool, ToolCtx, ToolError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Inbox;
use crate::store::{self, Filter, Source};

/// The tools that only need the database. The `aos` CLI registers these.
pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "inbox_list",
            "Messages from WhatsApp and email, newest first. Defaults to open (unhandled) items triage says Nora should read. Triage score is the probability (0..1) that it deserves reading.",
            Risk::Free,
            list,
        )
        .read_only(),
        Tool::new(
            "inbox_mark",
            "Mark an inbox item handled (default) or open again, with an optional note.",
            Risk::Free,
            mark,
        )
        .positional(&["id"]),
        Tool::new(
            "inbox_reply",
            "Reply to an inbox item. First call stores a DRAFT and sends nothing; show it to the owner. Only call again with confirm=true after he approves the exact text. WhatsApp only.",
            Risk::Ask,
            reply,
        )
        .positional(&["id", "text"]),
    ]
}

/// [`tools`] plus `inbox_status`, which needs the running workers.
pub fn tools_with(inbox: &Inbox) -> Vec<Tool> {
    let mut v = tools();
    let i = inbox.clone();
    v.push(
        Tool::new(
            "inbox_status",
            "Inbox health: open counts, WhatsApp link (paired or needs pairing), Gmail accounts, triage service.",
            Risk::Free,
            move |ctx: &ToolCtx<'_>, _: Empty| status(ctx, &i),
        )
        .read_only(),
    );
    v
}

/// Foundation's registry plus every inbox tool.
pub fn registry(inbox: &Inbox) -> aos_tools::Registry {
    let mut r = aos_tools::registry();
    r.extend(tools_with(inbox));
    r
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListArgs {
    /// `whatsapp`, `gmail`, `workmail` or `app`.
    #[serde(default)]
    pub source: Option<Source>,
    /// `open` (default), `handled` or `all`.
    #[serde(default)]
    pub state: Option<String>,
    /// Include items triage scored below the threshold (promotions, OTPs...). Default false.
    #[serde(default)]
    pub all_triage: bool,
    /// One chat or thread id.
    #[serde(default)]
    pub chat: Option<String>,
    /// How many, default 20, at most 200.
    #[serde(default)]
    pub limit: Option<u32>,
}

fn list(ctx: &ToolCtx<'_>, a: ListArgs) -> Result<Value, ToolError> {
    let handled = match a.state.as_deref().unwrap_or("open") {
        "open" => Some(false),
        "handled" => Some(true),
        "all" => None,
        other => return Err(ToolError::InvalidArgs(format!("state '{other}': use open, handled or all"))),
    };
    let items = store::list_items(
        ctx.store,
        &Filter {
            source: a.source.map(|s| s.as_str().to_string()),
            handled,
            only_read: !a.all_triage,
            chat: a.chat,
            limit: a.limit.unwrap_or(20).min(200) as usize,
        },
    )?;
    let (open, to_read) = store::open_counts(ctx.store)?;
    Ok(json!({ "items": items, "open": open, "open_to_read": to_read }))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkArgs {
    /// Item id or a unique prefix (4+ characters).
    pub id: String,
    /// true (default) marks handled, false reopens.
    #[serde(default)]
    pub handled: Option<bool>,
    /// A note to keep with the item.
    #[serde(default)]
    pub note: Option<String>,
}

fn mark(ctx: &ToolCtx<'_>, a: MarkArgs) -> Result<Value, ToolError> {
    let item = store::mark(ctx.store, ctx.actor, &a.id, a.handled.unwrap_or(true), a.note.as_deref())?;
    Ok(json!({ "item": item }))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplyArgs {
    /// Item id or unique prefix.
    pub id: String,
    /// The reply text, exactly as it should be sent.
    pub text: String,
    /// Must be true to send. Leave it out to only store a draft.
    #[serde(default)]
    pub confirm: bool,
}

fn reply(ctx: &ToolCtx<'_>, a: ReplyArgs) -> Result<Value, ToolError> {
    let (item, draft) = store::save_draft(ctx.store, ctx.actor, &a.id, &a.text)?;
    if !a.confirm {
        return Ok(json!({
            "status": "draft",
            "sent": false,
            "reply_id": draft.id,
            "to": if item.chat_name.is_empty() { &item.chat } else { &item.chat_name },
            "text": draft.text,
            "next": "Nothing was sent. Show the owner the text; call inbox_reply again with confirm=true only after he approves it."
        }));
    }
    if item.source != Source::Whatsapp.as_str() {
        return Err(ToolError::InvalidArgs(format!(
            "replying to {} is not supported yet (draft kept)",
            item.source
        )));
    }
    let queued = store::queue_reply(ctx.store, ctx.actor, &draft.id)?;
    Ok(json!({
        "status": "queued",
        "sent": false,
        "reply_id": queued.id,
        "next": "Queued. The WhatsApp worker sends it within seconds if WhatsApp is paired; check inbox_status."
    }))
}

fn status(ctx: &ToolCtx<'_>, inbox: &Inbox) -> Result<Value, ToolError> {
    let (open, to_read) = store::open_counts(ctx.store).map_err(ToolError::from)?;
    let queued = store::queued_replies(ctx.store, Source::Whatsapp)
        .map_err(|e: Error| ToolError::from(e))?
        .len();
    let wa = inbox.whatsapp.service.status();
    let laya = inbox.laya.status();
    Ok(json!({
        "open": open,
        "open_to_read": to_read,
        "whatsapp": {
            "enabled": inbox.config.whatsapp.enabled,
            "link": inbox.whatsapp.link(),
            "sync": wa,
            "queued_replies": queued,
            "pair_command": "aos inbox whatsapp pair",
        },
        "gmail": inbox.config.gmail.iter().map(|g| json!({ "account": g.name, "query": g.query })).collect::<Vec<_>>(),
        "triage": { "enabled": inbox.triage.enabled(), "addr": inbox.triage.addr(), "laya_process": laya },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::sample;
    use aos_core::Store;

    fn run(r: &aos_tools::Registry, s: &Store, tool: &str, args: Value) -> Result<Value, ToolError> {
        r.call(&ToolCtx::new(s, "test"), tool, args)
    }

    fn registry() -> aos_tools::Registry {
        let mut r = aos_tools::registry();
        r.extend(tools());
        r
    }

    #[test]
    fn list_hides_triaged_noise_and_mark_closes() {
        let s = Store::open_temporary().unwrap();
        let r = registry();
        let a = store::insert_item(&s, "t", &sample("1", "c", "pay me", "2026-01-01T10:00:00.000Z")).unwrap().unwrap();
        let b = store::insert_item(&s, "t", &sample("2", "c", "SALE!!", "2026-01-01T10:01:00.000Z")).unwrap().unwrap();
        store::set_triage(&s, &a.id, Some(0.9), true, "laya").unwrap();
        store::set_triage(&s, &b.id, Some(0.02), false, "laya").unwrap();

        let out = run(&r, &s, "inbox.list", json!({})).unwrap();
        assert_eq!(out["items"].as_array().unwrap().len(), 1);
        assert_eq!(out["open"], 2);
        let all = run(&r, &s, "inbox_list", json!({ "all_triage": true })).unwrap();
        assert_eq!(all["items"].as_array().unwrap().len(), 2);

        run(&r, &s, "inbox_mark", json!({ "id": a.id, "note": "paid" })).unwrap();
        let open = run(&r, &s, "inbox_list", json!({})).unwrap();
        assert!(open["items"].as_array().unwrap().is_empty());
    }

    #[test]
    fn reply_needs_explicit_confirmation() {
        let s = Store::open_temporary().unwrap();
        let r = registry();
        assert_eq!(r.get("inbox_reply").unwrap().risk, Risk::Ask);
        let a = store::insert_item(&s, "t", &sample("1", "c", "hi", "2026-01-01T10:00:00.000Z")).unwrap().unwrap();

        let d = run(&r, &s, "inbox_reply", json!({ "id": a.id, "text": "ok" })).unwrap();
        assert_eq!(d["status"], "draft");
        assert!(store::queued_replies(&s, Source::Whatsapp).unwrap().is_empty());

        let q = run(&r, &s, "inbox_reply", json!({ "id": a.id, "text": "ok", "confirm": true })).unwrap();
        assert_eq!(q["status"], "queued");
        assert_eq!(store::queued_replies(&s, Source::Whatsapp).unwrap().len(), 1);
    }

    #[test]
    fn email_replies_are_refused() {
        let s = Store::open_temporary().unwrap();
        let r = registry();
        let mut m = sample("1", "t1", "hello", "2026-01-01T10:00:00.000Z");
        m.source = Source::Gmail;
        let a = store::insert_item(&s, "t", &m).unwrap().unwrap();
        let e = run(&r, &s, "inbox_reply", json!({ "id": a.id, "text": "ok", "confirm": true }));
        assert!(matches!(e, Err(ToolError::InvalidArgs(_))));
    }
}
