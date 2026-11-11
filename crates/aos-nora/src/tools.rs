//! Nora's tools for the shared registry (app, `aos` CLI via the server, MCP for Grok Bot and for
//! Nora's own Claude turns). `alarm_set` is foundation's tool; the scheduler here fires it.

use aos_tools::{Risk, Tool, ToolCtx, ToolError};
use chrono::Local;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::events::NoraEvent;
use crate::memory::RememberArgs;
use crate::nora::{EngineChoice, Nora};
use crate::prompt;
use crate::store::{Msg, NewMsg};

fn err(e: anyhow::Error) -> ToolError {
    ToolError::InvalidArgs(format!("{e:#}"))
}

fn view(m: &Msg, owner: &str) -> Value {
    let who = match m.engine.as_str() {
        "user" => owner.to_string(),
        "claude" => "Nora (claude)".into(),
        "grok" => "Nora (grok)".into(),
        _ => "system".into(),
    };
    json!({ "id": m.id, "at": prompt::local_time(m.at), "who": who, "via": m.source, "kind": m.kind, "text": m.text })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendArgs {
    /// The message.
    pub text: String,
    /// auto (default: Claude, which may hand over to Grok), claude or grok.
    #[serde(default)]
    pub engine: EngineChoice,
    /// Who is writing: user (default, starts a turn), grok (Grok Nora's own words, just stored) or system.
    #[serde(default, rename = "as")]
    pub as_: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThreadArgs {
    /// How many recent messages (default 30, max 200).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Only messages older than this message id (to page back).
    #[serde(default)]
    pub before: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DetailArgs {
    /// Words to search for in older messages.
    #[serde(default)]
    pub query: Option<String>,
    /// From this time: a day (2026-11-12, yesterday), a time today (08:00) or an ISO time.
    #[serde(default)]
    pub from: Option<String>,
    /// Up to this time (same forms as `from`).
    #[serde(default)]
    pub to: Option<String>,
    /// Show messages around this message id (10 before, 10 after), full text.
    #[serde(default)]
    pub around: Option<i64>,
    /// Max messages (default 40).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GrokAskArgs {
    /// What to ask or tell Grok Nora (she answers in the thread when the Grok side replies).
    pub text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallArgs {
    /// What to look for.
    pub query: String,
    /// Max long-term memories (default 15).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForgetArgs {
    /// Memory id: L12 (long-term) or S4 (short-term).
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CallArgs {
    /// Why she wants to call, one line (shown on the ring and in the thread if missed).
    pub reason: String,
}

fn parse_time(s: &str) -> Result<i64, ToolError> {
    let now = Local::now();
    if !s.contains(':') {
        let day = aos_core::time::parse_day(s, now.date_naive())?;
        if let Some(t) = day.and_hms_opt(0, 0, 0).and_then(|t| t.and_local_timezone(Local).earliest()) {
            return Ok(t.timestamp_millis());
        }
    }
    Ok(aos_core::time::parse_when(s, &now)?.timestamp_millis())
}

pub fn tools(nora: &Nora) -> Vec<Tool> {
    let n = || nora.clone();
    let (n1, n2, n3, n4, n5, n6, n7, n8) = (n(), n(), n(), n(), n(), n(), n(), n());
    vec![
        Tool::new(
            "nora_send",
            "Send a message into Nora's thread. As the user (default) it starts a reply: engine auto/claude runs a Claude turn, grok queues it for Grok Bot. `as: grok` stores Grok Nora's own words without a turn.",
            Risk::Tell,
            move |ctx: &ToolCtx<'_>, a: SendArgs| {
                let msg = match a.as_.as_deref().unwrap_or("user") {
                    "user" => n1.send(&a.text, a.engine, ctx.actor).map_err(err)?,
                    who @ ("grok" | "system") => n1
                        .post(NewMsg::new(who, ctx.actor, "message", a.text.trim()))
                        .map_err(err)?
                        .ok_or_else(|| ToolError::InvalidArgs("duplicate".into()))?,
                    other => return Err(ToolError::InvalidArgs(format!("as must be user, grok or system, not {other}"))),
                };
                Ok(json!({ "message": msg }))
            },
        )
        .positional(&["text"]),
        Tool::new(
            "nora_thread",
            "Nora's merged thread (his messages, Claude Nora, Grok Nora, alarms), newest last, with local timestamps.",
            Risk::Free,
            move |_ctx: &ToolCtx<'_>, a: ThreadArgs| {
                let msgs = n2.thread(a.limit.unwrap_or(30).min(200), a.before).map_err(err)?;
                let owner = &n2.config().owner;
                Ok(json!({ "messages": msgs.iter().map(|m| view(m, owner)).collect::<Vec<_>>() }))
            },
        )
        .read_only()
        .positional(&["limit"]),
        Tool::new(
            "thread_detail",
            "Read older parts of Nora's thread in full: search words, a time range, or the messages around one message id.",
            Risk::Free,
            move |_ctx: &ToolCtx<'_>, a: DetailArgs| {
                let limit = a.limit.unwrap_or(40).min(200);
                let db = n3.db();
                let msgs = if let Some(id) = a.around {
                    let mut before = db.recent(10, Some(id + 1)).map_err(err)?;
                    let center = before.last().map(|m| m.at).unwrap_or(0);
                    let after = db.search(None, Some(center), None, 200).map_err(err)?;
                    before.extend(after.into_iter().filter(|m| m.id > id).take(10));
                    before
                } else {
                    let from = a.from.as_deref().map(parse_time).transpose()?;
                    let to = a.to.as_deref().map(parse_time).transpose()?;
                    db.search(a.query.as_deref(), from, to, limit).map_err(err)?
                };
                let owner = &n3.config().owner;
                Ok(json!({ "messages": msgs.iter().map(|m| view(m, owner)).collect::<Vec<_>>() }))
            },
        )
        .read_only()
        .positional(&["query"]),
        Tool::new(
            "grok_ask",
            "Hand a message to Nora on Grok Bot (when the answer depends on Grok's own memory, chats or bots). Queued until the Grok write side is connected.",
            Risk::Tell,
            move |_ctx: &ToolCtx<'_>, a: GrokAskArgs| {
                let item = n4.db().queue_grok("ask", a.text.trim()).map_err(err)?;
                n4.bus().send(NoraEvent::GrokQueued { id: item.id, kind: item.kind.clone() });
                Ok(json!({ "status": "queued", "id": item.id }))
            },
        )
        .positional(&["text"]),
        Tool::new(
            "remember",
            "Save to Nora's memory: a lasting fact (term long, with kind and importance 1-5) or a short-term note / open loop (term short, expires in `days`). Near-duplicates update the old fact.",
            Risk::Free,
            move |ctx: &ToolCtx<'_>, a: RememberArgs| n5.db().remember(&a, ctx.actor).map_err(err),
        )
        .positional(&["text"]),
        Tool::new(
            "recall",
            "Search Nora's memory: long-term facts ranked by match and importance, plus live short-term notes and recent days.",
            Risk::Free,
            move |_ctx: &ToolCtx<'_>, a: RecallArgs| {
                let db = n6.db();
                Ok(json!({
                    "long": db.recall(&a.query, a.limit.unwrap_or(15).min(60)).map_err(err)?,
                    "short": db.short_term().map_err(err)?,
                    "days": db.episodes(3).map_err(err)?,
                }))
            },
        )
        .read_only()
        .positional(&["query"]),
        Tool::new(
            "forget",
            "Mark a memory as no longer true (L12 or S4).",
            Risk::Tell,
            move |_ctx: &ToolCtx<'_>, a: ForgetArgs| n7.db().forget(&a.id).map_err(err),
        )
        .positional(&["id"]),
        Tool::new(
            "call_request",
            "Ask to call him now (rings once; if he misses it, the reason becomes a message). The call itself comes in a later phase.",
            Risk::Tell,
            move |ctx: &ToolCtx<'_>, a: CallArgs| {
                let call = n8.db().add_call(a.reason.trim(), ctx.actor).map_err(err)?;
                n8.bus().send(NoraEvent::CallRequested { call: call.clone() });
                Ok(json!({ "status": "ringing", "call": call }))
            },
        )
        .positional(&["reason"]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nora::testing::nora_with;

    #[test]
    fn tools_register_and_run() {
        let (nora, _e, _p) = nora_with(&[], None);
        let mut r = aos_tools::registry();
        r.extend(tools(&nora));
        let ctx = ToolCtx::new(nora.core(), "test");
        r.call(&ctx, "nora.send", json!({ "text": "hello", "as": "grok" })).unwrap();
        r.call(&ctx, "remember", json!({ "text": "Likes chai", "kind": "preference" })).unwrap();
        let rec = r.call(&ctx, "recall", json!({ "query": "chai" })).unwrap();
        assert_eq!(rec["long"][0]["text"], "Likes chai");
        let t = r.call(&ctx, "nora_thread", json!({})).unwrap();
        assert_eq!(t["messages"][0]["who"], "Nora (grok)");
        let d = r.call(&ctx, "thread_detail", json!({ "query": "hel" })).unwrap();
        assert_eq!(d["messages"].as_array().unwrap().len(), 1);
        let d = r.call(&ctx, "thread_detail", json!({ "around": 1 })).unwrap();
        assert_eq!(d["messages"].as_array().unwrap().len(), 1);
        assert_eq!(r.call(&ctx, "grok_ask", json!({ "text": "what did the scout find?" })).unwrap()["status"], "queued");
        assert_eq!(r.call(&ctx, "call_request", json!({ "reason": "test" })).unwrap()["status"], "ringing");
        // alarm.set is foundation's tool and resolves through the same registry.
        r.call(&ctx, "alarm.set", json!({ "at": "+1m", "label": "Stand up" })).unwrap();
        assert!(r.call(&ctx, "nora_send", json!({ "text": "x", "as": "boss" })).is_err());
    }
}
