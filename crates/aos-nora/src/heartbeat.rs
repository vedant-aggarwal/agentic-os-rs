//! Hourly heartbeat, rules first (ported from the old app's `nora-heartbeat.ts`).
//!
//! 1. Rules decide without a model when they can: heartbeat off, asleep (21:00-05:00), in a
//!    calendar event, a call already ringing, or nothing changed since the last model beat.
//!    Those are stored as `skip` with the reason.
//! 2. Otherwise one cheap Claude run gets the time, today's plan and the recent thread with
//!    timestamps, and answers `none / note / message / alarm / call` with a reason.
//! 3. We act on it ourselves (no tools in that run). A call rings once; a missed ring becomes a
//!    message (see the scheduler).

use anyhow::Result;
use chrono::{DateTime, Local, Timelike};
use serde_json::{Value, json};
use tracing::info;

use crate::engine::{TurnRequest, ask};
use crate::events::NoraEvent;
use crate::nora::Nora;
use crate::prompt;
use crate::store::{Heartbeat, NewMsg};

pub const SLEEP_FROM: u32 = 21;
pub const SLEEP_TO: u32 = 5;

/// Why the model is not needed this hour, or `None` to ask it.
pub fn rules(now: DateTime<Local>, busy: Option<&str>, ringing: bool, sig: &str, last_sig: Option<&str>) -> Option<String> {
    let h = now.hour();
    if !(SLEEP_TO..SLEEP_FROM).contains(&h) {
        return Some(format!("asleep ({SLEEP_FROM}:00-0{SLEEP_TO}:00)"));
    }
    if let Some(b) = busy {
        return Some(format!("busy: {b}"));
    }
    if ringing {
        return Some("a call is already ringing".into());
    }
    if last_sig == Some(sig) {
        return Some("nothing changed since the last beat".into());
    }
    None
}

/// What changed: the newest change in the main store and the newest real message.
fn signature(nora: &Nora) -> Result<String> {
    let version = nora.core().latest_version()?;
    let msg: i64 = nora.db().conn().query_row(
        "SELECT COALESCE(MAX(id), 0) FROM nora_msg WHERE engine != 'system' AND source NOT IN ('heartbeat', 'scheduler')",
        [],
        |r| r.get(0),
    )?;
    Ok(format!("{version}:{msg}"))
}

fn plan_json(nora: &Nora) -> String {
    let registry = aos_tools::registry();
    let ctx = aos_tools::ToolCtx::new(nora.core(), "nora");
    match registry.call(&ctx, "today", Value::Null) {
        Ok(v) => prompt::clip(&v.to_string(), 6000),
        Err(e) => format!("(plan unavailable: {e})"),
    }
}

fn beat_prompt(nora: &Nora, now: DateTime<Local>) -> Result<String> {
    let owner = &nora.config().owner;
    let recent = nora.db().recent(20, None)?;
    let last = nora
        .db()
        .heartbeats(3)?
        .iter()
        .map(|b| format!("- {} {}: {}", prompt::local_time(b.at), b.decision, b.reason))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "Hourly check on {owner}. Now: {now}.\n\nToday's plan (JSON):\n{plan}\n\nRecent thread:\n{thread}\n\nYour last checks:\n{last}\n\n\
Compare what he planned with what is happening. Decide one action:\n\
- none: on track or nothing useful to add\n- note: a quiet note for his day view\n- message: a short message to him now\n\
- alarm: set an alarm/reminder (give `at`: 07:30, +15m or ISO)\n- call: only if something important is slipping and a message won't do\n\
Reply with JSON only: {{\"decision\": \"none|note|message|alarm|call\", \"reason\": \"why, one line\", \"text\": \"what to say / the note / the alarm label\", \"at\": \"for alarm only\"}}",
        now = now.format("%Y-%m-%d %H:%M %a"),
        plan = plan_json(nora),
        thread = prompt::format_thread(&recent, owner),
    ))
}

const SYSTEM: &str = "You are Nora's hourly heartbeat. You decide quietly whether to act. Prefer none unless acting clearly helps. Reply with one JSON object only.";

/// Parse the model's decision. Unknown decisions become `none`.
pub fn parse_decision(reply: &str) -> (String, String, String, Option<String>) {
    let start = reply.find('{').unwrap_or(0);
    let end = reply.rfind('}').map(|i| i + 1).unwrap_or(reply.len());
    let v: Value = serde_json::from_str(reply.get(start..end).unwrap_or("")).unwrap_or(json!({}));
    let mut decision = v["decision"].as_str().unwrap_or("none").to_lowercase();
    if !["none", "note", "message", "alarm", "call"].contains(&decision.as_str()) {
        decision = "none".into();
    }
    let reason = v["reason"].as_str().unwrap_or("no reason given").to_string();
    let text = v["text"].as_str().unwrap_or("").trim().to_string();
    (decision, reason, text, v["at"].as_str().map(String::from))
}

/// One heartbeat. `force` skips the rules (manual runs from the API).
pub async fn beat(nora: &Nora, force: bool) -> Result<Heartbeat> {
    let now = Local::now();
    let sig = signature(nora)?;
    if !force {
        let enabled = nora.settings()?.heartbeat_enabled;
        let busy = nora.config().calendar.as_ref().and_then(|f| f(now));
        let ringing = nora.db().ringing_call()?.is_some();
        let last_sig = nora.db().last_model_sig()?;
        let skip = if enabled { rules(now, busy.as_deref(), ringing, &sig, last_sig.as_deref()) } else { Some("heartbeat off".into()) };
        if let Some(reason) = skip {
            let b = nora.db().add_heartbeat("skip", &reason, "", "rules", &sig)?;
            nora.bus().send(NoraEvent::Heartbeat { beat: b.clone() });
            return Ok(b);
        }
    }
    let req = TurnRequest {
        system: SYSTEM.into(),
        prompt: beat_prompt(nora, now)?,
        model: Some(nora.config().cheap_model.clone()),
        timeout: Some(std::time::Duration::from_secs(180)),
        ..Default::default()
    };
    let model = nora.config().cheap_model.clone();
    let (decision, reason, text, at) = match ask(nora.engine().as_ref(), req).await {
        Ok(reply) => parse_decision(&reply),
        Err(e) => ("none".into(), format!("model failed: {e}"), String::new(), None),
    };
    let decision = act(nora, &decision, &reason, &text, at.as_deref())?;
    let b = nora.db().add_heartbeat(&decision, &reason, &text, &model, &sig)?;
    info!(decision = %b.decision, reason = %b.reason, "heartbeat");
    nora.bus().send(NoraEvent::Heartbeat { beat: b.clone() });
    Ok(b)
}

/// Carry out a decision. Returns the decision actually taken.
fn act(nora: &Nora, decision: &str, reason: &str, text: &str, at: Option<&str>) -> Result<String> {
    let say = if text.is_empty() { reason } else { text };
    match decision {
        "note" | "message" => {
            let kind = if decision == "note" { "note" } else { "message" };
            nora.post(NewMsg::new("claude", "heartbeat", kind, say))?;
        }
        "alarm" => {
            let when = at.and_then(|a| aos_core::time::parse_when(a, &Local::now()).ok());
            let Some(when) = when else {
                nora.post(NewMsg::new("claude", "heartbeat", "note", say))?;
                return Ok("note".into());
            };
            nora.core().set_alarm("nora", when, say, aos_core::model::AlarmKind::Reminder, None)?;
        }
        "call" => {
            let call = nora.db().add_call(say, "heartbeat")?;
            nora.bus().send(NoraEvent::CallRequested { call });
        }
        _ => {}
    }
    Ok(decision.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nora::Settings;
    use crate::nora::testing::nora_with;
    use chrono::TimeZone;

    fn at(h: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 12, h, 0, 0).unwrap()
    }

    #[test]
    fn rules_skip_without_model() {
        assert!(rules(at(23), None, false, "a", None).unwrap().starts_with("asleep"));
        assert!(rules(at(4), None, false, "a", None).unwrap().starts_with("asleep"));
        assert!(rules(at(11), Some("Physics"), false, "a", None).unwrap().contains("Physics"));
        assert!(rules(at(11), None, true, "a", None).is_some());
        assert!(rules(at(11), None, false, "a", Some("a")).unwrap().contains("nothing changed"));
        assert!(rules(at(11), None, false, "b", Some("a")).is_none());
    }

    #[test]
    fn parses_decisions() {
        let (d, r, t, a) = parse_decision("ok {\"decision\":\"alarm\",\"reason\":\"gym\",\"text\":\"Gym\",\"at\":\"+15m\"}");
        assert_eq!((d.as_str(), r.as_str(), t.as_str(), a.as_deref()), ("alarm", "gym", "Gym", Some("+15m")));
        assert_eq!(parse_decision("garbage").0, "none");
        assert_eq!(parse_decision("{\"decision\":\"dance\"}").0, "none");
    }

    #[tokio::test]
    async fn off_by_default_then_model_message() {
        let (nora, engine, _p) = nora_with(&["{\"decision\":\"message\",\"reason\":\"drifting\",\"text\":\"Back to the essay?\"}"], None);
        let b = beat(&nora, false).await.unwrap();
        assert_eq!((b.decision.as_str(), b.reason.as_str()), ("skip", "heartbeat off"));
        assert!(engine.seen.lock().unwrap().is_empty());
        nora.set_settings(&Settings { heartbeat_enabled: true, ..Default::default() }).unwrap();
        let b = beat(&nora, true).await.unwrap();
        assert_eq!(b.decision, "message");
        let msgs = nora.thread(5, None).unwrap();
        assert_eq!(msgs.last().unwrap().text, "Back to the essay?");
        assert_eq!(msgs.last().unwrap().source, "heartbeat");
    }

    #[tokio::test]
    async fn call_decision_rings_once() {
        let (nora, _e, _p) = nora_with(&["{\"decision\":\"call\",\"reason\":\"deadline in 1h\"}"], None);
        beat(&nora, true).await.unwrap();
        assert!(nora.db().ringing_call().unwrap().is_some());
        // While ringing, the rules hold the next beat back.
        assert!(rules(at(11), None, true, "x", None).is_some());
    }
}
