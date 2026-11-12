//! Wave tools (urge help): `wave_start`, `wave_update`, `wave_finish`, `wave_stats`, plus reads
//! for Nora's night journal and the night guard. `wave.start` works as an alias.
//!
//! The log is private. Nora reads it for the night journal and the close-the-day talk; she
//! never reads it out in a way that shames, and a slip is data.

use aos_core::wave::NewWave;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::registry::{Risk, Tool, ToolCtx, ToolError};

type R = Result<Value, ToolError>;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new("wave_start", "Start a Wave (urge help): a 10-minute timer. Returns the running one if there is already one. `id` may be made on the device.", Risk::Free, start),
        Tool::new("wave_update", "Add to the running Wave: intensity (0..10), place (bed, bathroom, desk, elsewhere), alone, feelings, trigger_kind, action, lie, truth, note, steps, ends_at (later = another ten minutes).", Risk::Free, update).positional(&["id"]),
        Tool::new("wave_finish", "End a Wave: outcome beaten (earns Willpower XP), strong (still strong, no XP) or slipped (no shame; it is data and nothing resets).", Risk::Tell, finish).positional(&["id", "outcome"]),
        Tool::new("wave_active", "The Wave that is running or waiting for its outcome, if any.", Risk::Free, active).read_only(),
        Tool::new("wave_list", "Recent Waves, newest first, with every logged field.", Risk::Free, list).read_only().positional(&["limit"]),
        Tool::new("wave_stats", "Urges beaten next to slips over the last N days (default 30), by hour, trigger, place and action, with the night guard count. Read for the night journal.", Risk::Free, stats).read_only().positional(&["days"]),
        Tool::new("wave_context", "The week in a few lines of text: counts and the last urges with what he noticed and did. Read before the close-the-day talk. Never shame.", Risk::Free, context).read_only(),
        Tool::new("wave_settings", "Night guard setting: on or off, and the time (HH:MM) Nora asks \"phone docked outside the bed?\". Pass nothing to read it.", Risk::Tell, settings),
        Tool::new("wave_guard_answer", "Record his answer to the night guard question for tonight (or `date`): docked true or false.", Risk::Free, guard_answer).positional(&["docked"]),
    ]
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartArgs {
    pub id: Option<String>,
    /// RFC 3339 start time (default now).
    pub started_at: Option<String>,
    /// Timer length in minutes (default 10).
    pub minutes: Option<i64>,
}

fn start(ctx: &ToolCtx<'_>, a: StartArgs) -> R {
    Ok(ctx.store.wave_start(ctx.actor, NewWave { id: a.id, started_at: a.started_at, minutes: a.minutes })?)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateArgs {
    pub id: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

fn update(ctx: &ToolCtx<'_>, a: UpdateArgs) -> R {
    Ok(ctx.store.wave_update(ctx.actor, &a.id, &a.fields)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinishArgs {
    pub id: String,
    /// beaten, strong or slipped.
    pub outcome: String,
    pub note: Option<String>,
}

fn finish(ctx: &ToolCtx<'_>, a: FinishArgs) -> R {
    Ok(ctx.store.wave_finish(ctx.actor, &a.id, &a.outcome, a.note.as_deref())?)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

fn active(ctx: &ToolCtx<'_>, _: NoArgs) -> R {
    Ok(json!({ "wave": ctx.store.wave_active()? }))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListArgs {
    pub limit: Option<usize>,
}

fn list(ctx: &ToolCtx<'_>, a: ListArgs) -> R {
    Ok(json!({ "waves": ctx.store.wave_list(a.limit.unwrap_or(20))? }))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatsArgs {
    pub days: Option<i64>,
}

fn stats(ctx: &ToolCtx<'_>, a: StatsArgs) -> R {
    Ok(ctx.store.wave_stats(ctx.today(), a.days.unwrap_or(30))?)
}

fn context(ctx: &ToolCtx<'_>, _: NoArgs) -> R {
    Ok(json!({ "text": ctx.store.wave_context(ctx.today(), 4000)? }))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsArgs {
    pub night_guard: Option<bool>,
    /// HH:MM, local (default 22:30).
    pub night_guard_at: Option<String>,
}

fn settings(ctx: &ToolCtx<'_>, a: SettingsArgs) -> R {
    let s = if a.night_guard.is_some() || a.night_guard_at.is_some() {
        ctx.store.wave_set_settings(ctx.actor, a.night_guard, a.night_guard_at.as_deref())?
    } else {
        ctx.store.wave_settings()?
    };
    Ok(json!(s))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardArgs {
    pub docked: bool,
    pub note: Option<String>,
    /// YYYY-MM-DD, default today.
    pub date: Option<String>,
}

fn guard_answer(ctx: &ToolCtx<'_>, a: GuardArgs) -> R {
    let day = match &a.date {
        Some(d) => aos_core::time::parse_day(d, ctx.today())?,
        None => ctx.today(),
    };
    Ok(ctx.store.wave_guard_answer(ctx.actor, day, a.docked, a.note.as_deref())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_run_through_the_registry() {
        let reg = crate::registry();
        let st = aos_core::Store::open_temporary().unwrap();
        let c = |name: &str, args: Value| reg.call(&ToolCtx::new(&st, "t"), name, args).unwrap();
        let w = c("wave.start", json!({ "id": "w1" }));
        assert_eq!(w["id"], "w1");
        c("wave_update", json!({ "id": "w1", "intensity": 5, "place": "bed" }));
        let f = c("wave_finish", json!({ "id": "w1", "outcome": "beaten" }));
        assert_eq!(f["xp_delta"], aos_core::wave::WAVE_XP);
        assert_eq!(c("wave_stats", json!({}))["beaten"], 1);
        assert!(c("wave_context", json!({}))["text"].as_str().unwrap().contains("1 beaten"));
        assert_eq!(c("wave_settings", json!({ "night_guard": true, "night_guard_at": "23:00" }))["night_guard_at"], "23:00");
        assert!(c("wave_active", json!({}))["wave"].is_null());
    }
}
