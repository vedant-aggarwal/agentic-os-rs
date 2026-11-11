//! One Tokio loop, ticking every second, persisted in SQLite so restarts lose nothing:
//!
//! - **Alarms and reminders** (foundation's `alarm` table, set with the `alarm_set` tool from
//!   Nora, Grok over MCP or the CLI): fired to the second, marked `fired`, posted to the thread and
//!   sent on the bus as `alarm_fired`. Push to the phone comes later.
//! - **Missed calls**: a call still ringing after [`RING_SECS`] becomes a message (ring once).
//! - **Heartbeat** at the top of every hour (rules first; off by default).
//! - **Consolidation** at 03:00 for the day before (off by default).
//! - The Grok outbox is flushed by its own task, [`crate::grok_write::run`].

use std::time::Duration;

use anyhow::Result;
use chrono::{Duration as ChronoDuration, Local, Timelike};
use tracing::warn;

use crate::engine::{TurnRequest, ask};
use crate::events::NoraEvent;
use crate::memory::{apply_consolidation, consolidation_prompt};
use crate::nora::Nora;
use crate::prompt;
use crate::store::{NewMsg, now_ms};

pub const RING_SECS: i64 = 90;
pub const CONSOLIDATE_HOUR: u32 = 3;

pub async fn run(nora: Nora) {
    let shutdown = nora.shutdown_token();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ticks: u64 = 0;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tick.tick() => {}
        }
        ticks += 1;
        let t = ticks;
        let n = nora.clone();
        let fired = tokio::task::spawn_blocking(move || -> Result<()> {
            fire_due_alarms(&n)?;
            missed_calls(&n)?;
            // The 21:00 close-the-day call check runs every 15 s. (The Grok outbox has its own task.)
            if t.is_multiple_of(15) {
                crate::call::close_day_tick(&n)?;
            }
            Ok(())
        })
        .await;
        if let Ok(Err(e)) = fired {
            warn!("nora scheduler: {e:#}");
        }
        if let Err(e) = hourly(&nora).await {
            warn!("nora hourly: {e:#}");
        }
    }
}

/// Fire every scheduled alarm whose time has come. Returns how many fired.
pub fn fire_due_alarms(nora: &Nora) -> Result<usize> {
    let now = aos_core::time::now();
    let due = nora.core().due_alarms(now)?;
    for a in &due {
        let alarm = nora.core().mark_alarm_fired("scheduler", &a.id)?;
        let late_secs = chrono::DateTime::parse_from_rfc3339(&a.at).map(|t| (now - t.to_utc()).num_seconds()).unwrap_or(0);
        let label = match alarm.kind {
            aos_core::model::AlarmKind::Alarm => "Alarm",
            aos_core::model::AlarmKind::Reminder => "Reminder",
        };
        let late = if late_secs > 60 { format!(" (late by {} min: the server was off)", late_secs / 60) } else { String::new() };
        nora.post(NewMsg::new("system", "scheduler", "alarm", format!("{label}: {}{late}", alarm.label)))?;
        nora.bus().send(NoraEvent::AlarmFired { alarm, late_secs });
    }
    Ok(due.len())
}

fn missed_calls(nora: &Nora) -> Result<()> {
    for call in nora.db().take_missed_calls(now_ms() - RING_SECS * 1000)? {
        nora.post(NewMsg::new("claude", "heartbeat", "message", format!("I tried to call you: {}", call.reason)))?;
        nora.bus().send(NoraEvent::CallMissed { call });
    }
    Ok(())
}

/// Heartbeat once per clock hour and consolidation once per night, tracked in `nora_kv`.
async fn hourly(nora: &Nora) -> Result<()> {
    let now = Local::now();
    let hour_key = now.format("%Y-%m-%d %H").to_string();
    if nora.db().get_kv("last_beat_hour")?.as_deref() != Some(&hour_key) {
        nora.db().set_kv("last_beat_hour", &hour_key)?;
        // The first tick after start only records the hour, so a restart doesn't beat twice.
        if nora.db().get_kv("beat_armed")?.is_some() {
            crate::heartbeat::beat(nora, false).await?;
        }
        nora.db().set_kv("beat_armed", "1")?;
    }
    if now.hour() == CONSOLIDATE_HOUR && nora.settings()?.consolidation_enabled {
        let day = (now - ChronoDuration::days(1)).format("%Y-%m-%d").to_string();
        if nora.db().get_kv("last_consolidated")?.as_deref() != Some(&day) {
            nora.db().set_kv("last_consolidated", &day)?;
            consolidate(nora, &day).await?;
        }
    }
    Ok(())
}

/// Consolidate one day (`YYYY-MM-DD`, local) with the cheap model.
pub async fn consolidate(nora: &Nora, day: &str) -> Result<serde_json::Value> {
    let date = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")?;
    let start = date.and_hms_opt(0, 0, 0).and_then(|t| t.and_local_timezone(Local).earliest()).map(|t| t.timestamp_millis()).unwrap_or(0);
    let end = start + 86_400_000;
    let msgs = nora.db().msgs_between(start, end - 1)?;
    if msgs.is_empty() {
        return Ok(serde_json::json!({ "day": day, "skipped": "no messages" }));
    }
    let transcript = prompt::clip(&prompt::format_thread(&msgs, &nora.config().owner), 60_000);
    let short = nora.db().short_term()?;
    let req = TurnRequest {
        system: "You consolidate an assistant's memory. Reply with one JSON object only.".into(),
        prompt: consolidation_prompt(day, &transcript, &short),
        model: Some(nora.config().cheap_model.clone()),
        timeout: Some(Duration::from_secs(300)),
        ..Default::default()
    };
    let reply = ask(nora.engine().as_ref(), req).await.map_err(anyhow::Error::msg)?;
    apply_consolidation(nora.db(), day, &reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nora::testing::nora_with;

    #[test]
    fn fires_due_alarm_once_and_posts_it() {
        let (nora, _e, _p) = nora_with(&[], None);
        let mut rx = nora.bus().subscribe();
        let at = aos_core::time::now() - ChronoDuration::seconds(1);
        nora.core().set_alarm("t", at, "Stretch", aos_core::model::AlarmKind::Alarm, None).unwrap();
        nora.core().set_alarm("t", at + ChronoDuration::hours(1), "Later", aos_core::model::AlarmKind::Alarm, None).unwrap();
        assert_eq!(fire_due_alarms(&nora).unwrap(), 1);
        assert_eq!(fire_due_alarms(&nora).unwrap(), 0);
        let msgs = nora.thread(5, None).unwrap();
        assert_eq!(msgs.last().unwrap().text, "Alarm: Stretch");
        let mut fired = false;
        while let Ok(e) = rx.try_recv() {
            fired |= matches!(e, NoraEvent::AlarmFired { .. });
        }
        assert!(fired);
    }

    #[tokio::test]
    async fn consolidates_a_day() {
        let (nora, _e, _p) = nora_with(&["{\"episode\":\"Built Nora.\",\"remember\":[],\"expire\":[]}"], None);
        nora.post(NewMsg::new("user", "app", "to_grok", "hello")).unwrap();
        let day = Local::now().format("%Y-%m-%d").to_string();
        let out = consolidate(&nora, &day).await.unwrap();
        assert_eq!(out["episode"], "Built Nora.");
        let out = consolidate(&nora, "2001-01-01").await.unwrap();
        assert_eq!(out["skipped"], "no messages");
    }
}
