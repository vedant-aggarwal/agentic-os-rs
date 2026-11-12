//! Wave: urge help. A ten-minute timer, what he noticed, what he did, how it ended.
//!
//! - One row per urge in `urge`. The timer lives on the row (`ends_at`), so it survives restarts.
//! - Outcomes: `beaten` (earns Willpower XP), `strong` (timer ended, still strong: no XP, no
//!   penalty) and `slipped` (logged as data; nothing resets and no XP is taken away from earlier
//!   days). A slip never zeroes a count. Stats show beaten next to slips.
//! - The night guard keeps a setting and one row per night in `wave_guard`.
//!
//! Reads return JSON rows. Detail is for Private mode only; the app hides it in Safe mode.

use chrono::{DateTime, Duration, Local, NaiveDate, Timelike};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::error::{Result, invalid};
use crate::mission::{patch_row, row, rows, s};
use crate::store::{ChangeOp, Store};
use crate::{ids, time, xp};

pub const SCHEMA: &str = include_str!("../migrations/wave.sql");

pub fn ensure_schema(c: &Connection) -> Result<()> {
    c.execute_batch(SCHEMA)?;
    Ok(())
}

pub const MINUTES: i64 = 10;
pub const WAVE_XP: i64 = 15;
pub const PLACES: &[&str] = &["bed", "bathroom", "desk", "elsewhere"];
pub const OUTCOMES: &[&str] = &["beaten", "strong", "slipped"];
const COLS: &str = "id, date, hour, started_at, ends_at, finished_at, intensity, place, alone, feelings, trigger_kind, action, lie, truth, outcome, note, steps, xp, created_at, updated_at";
/// An unfinished urge older than this is stale, not active.
const ACTIVE_HOURS: i64 = 12;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewWave {
    /// Made on the device so an offline start can be updated before it syncs.
    pub id: Option<String>,
    /// RFC 3339; defaults to now.
    pub started_at: Option<String>,
    /// Timer length, default 10.
    pub minutes: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WaveSettings {
    pub night_guard: bool,
    /// `HH:MM`, local.
    pub night_guard_at: String,
}

fn fix(mut v: Value) -> Value {
    if let Some(m) = v.as_object_mut() {
        for k in ["feelings", "steps"] {
            if let Some(Value::String(t)) = m.get(k) {
                let parsed = serde_json::from_str::<Value>(t).unwrap_or_else(|_| json!([]));
                m.insert(k.into(), parsed);
            }
        }
        if let Some(a) = m.get("alone").and_then(Value::as_i64) {
            m.insert("alone".into(), json!(a != 0));
        }
    }
    v
}

fn one(c: &Connection, id: &str) -> Result<Value> {
    Ok(fix(row(c, &format!("SELECT {COLS} FROM urge WHERE id = ?1"), &[s(id)], "urge", id)?))
}

fn local_hour(ts: &str) -> Result<(NaiveDate, u32)> {
    let d = DateTime::parse_from_rfc3339(ts).map_err(|_| invalid(format!("not a time: {ts}")))?;
    let l = d.with_timezone(&Local);
    Ok((l.date_naive(), l.hour()))
}

fn active(c: &Connection) -> Result<Option<Value>> {
    let since = time::ts(time::now() - Duration::hours(ACTIVE_HOURS));
    let r = rows(c, &format!("SELECT {COLS} FROM urge WHERE finished_at IS NULL AND started_at >= ?1 ORDER BY started_at DESC LIMIT 1"), &[s(since)])?;
    Ok(r.into_iter().next().map(fix))
}

impl Store {
    /// Start a wave. If this id exists (a replayed offline write) that one comes back; with no id,
    /// a wave that is already running comes back, so a second caller never makes two.
    pub fn wave_start(&self, by: &str, n: NewWave) -> Result<Value> {
        let minutes = n.minutes.unwrap_or(MINUTES).clamp(1, 60);
        let started = match &n.started_at {
            Some(t) => DateTime::parse_from_rfc3339(t).map_err(|_| invalid("started_at must be RFC 3339"))?.to_utc(),
            None => time::now(),
        };
        self.write(by, |tx| {
            if let Some(id) = &n.id
                && let Ok(v) = one(tx, id)
            {
                return Ok(v);
            }
            // Without an id, a running wave comes back. With a new id the device means it.
            if n.id.is_none()
                && let Some(v) = active(tx)?
            {
                return Ok(v);
            }
            let id = n.id.clone().unwrap_or_else(ids::new_id);
            let (date, hour) = local_hour(&time::ts(started))?;
            tx.execute(
                "INSERT INTO urge (id, date, hour, started_at, ends_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![id, date, hour, time::ts(started), time::ts(started + Duration::minutes(minutes)), tx.now()],
            )?;
            tx.record("urge", &id, ChangeOp::Upsert)?;
            one(tx, &id)
        })
    }

    /// Add what he noticed or did. Allowed keys: intensity (0..10), place, alone, feelings,
    /// trigger_kind, action, lie, truth, note, steps, ends_at. Moving `ends_at` later on a
    /// finished wave reopens it ("another ten minutes").
    pub fn wave_update(&self, by: &str, id: &str, patch: &Map<String, Value>) -> Result<Value> {
        let mut p = Map::new();
        for (k, v) in patch {
            match k.as_str() {
                "intensity" => {
                    if !v.is_null() && !v.as_i64().is_some_and(|i| (0..=10).contains(&i)) {
                        return Err(invalid("intensity must be 0..10"));
                    }
                    p.insert(k.clone(), v.clone());
                }
                "place" => {
                    if let Some(x) = v.as_str() {
                        crate::mission::one_of("place", x, PLACES)?;
                    }
                    p.insert(k.clone(), v.clone());
                }
                "alone" => {
                    p.insert(k.clone(), v.as_bool().map(|b| json!(b as i64)).unwrap_or(Value::Null));
                }
                "feelings" | "steps" => {
                    let arr = v.as_array().ok_or_else(|| invalid(format!("{k} must be a list")))?;
                    p.insert(k.clone(), json!(Value::Array(arr.clone()).to_string()));
                }
                "trigger_kind" | "action" | "lie" | "truth" | "note" | "ends_at" => {
                    if k == "ends_at" {
                        DateTime::parse_from_rfc3339(v.as_str().unwrap_or("")).map_err(|_| invalid("ends_at must be RFC 3339"))?;
                    }
                    p.insert(k.clone(), v.clone());
                }
                other => return Err(invalid(format!("urge: '{other}' can't be changed here"))),
            }
        }
        self.write(by, |tx| {
            let id = crate::store::resolve_prefix(tx, "urge", id)?;
            if let Some(new_end) = patch.get("ends_at").and_then(Value::as_str) {
                let old = one(tx, &id)?;
                let later = old["ends_at"].as_str().unwrap_or("") < new_end;
                if later && old["finished_at"].is_string() {
                    let date: NaiveDate = old["date"].as_str().and_then(|d| d.parse().ok()).unwrap_or_else(time::local_today);
                    xp::set_award(tx, "wave", &id, None, 0, date)?;
                    tx.execute("UPDATE urge SET finished_at = NULL, outcome = NULL, xp = 0 WHERE id = ?1", [&id])?;
                }
            }
            patch_row(tx, "urge", &id, &["intensity", "place", "alone", "feelings", "trigger_kind", "action", "lie", "truth", "note", "steps", "ends_at"], &p)?;
            one(tx, &id)
        })
    }

    /// Close a wave: beaten (earns XP), strong (timer ended, still strong) or slipped.
    /// Calling again changes the outcome and the XP follows.
    pub fn wave_finish(&self, by: &str, id: &str, outcome: &str, note: Option<&str>) -> Result<Value> {
        crate::mission::one_of("outcome", outcome, OUTCOMES)?;
        self.write(by, |tx| {
            let id = crate::store::resolve_prefix(tx, "urge", id)?;
            let old = one(tx, &id)?;
            let date: NaiveDate = old["date"].as_str().and_then(|d| d.parse().ok()).unwrap_or_else(time::local_today);
            let area: Option<String> = tx.query_row("SELECT id FROM area WHERE id = 'mindset'", [], |r| r.get(0)).optional()?;
            let target = if outcome == "beaten" { WAVE_XP } else { 0 };
            let delta = xp::set_award(tx, "wave", &id, area.as_deref(), target, date)?;
            let finished = old["finished_at"].as_str().map(String::from).unwrap_or_else(|| tx.now().to_string());
            tx.execute(
                "UPDATE urge SET outcome = ?2, finished_at = ?3, xp = ?4, note = COALESCE(?5, note), updated_at = ?6 WHERE id = ?1",
                params![id, outcome, finished, target, note, tx.now()],
            )?;
            tx.record("urge", &id, ChangeOp::Upsert)?;
            let mut v = one(tx, &id)?;
            v["xp_delta"] = json!(delta);
            Ok(v)
        })
    }

    pub fn wave_get(&self, id: &str) -> Result<Value> {
        self.read(|c| {
            let id = crate::store::resolve_prefix(c, "urge", id)?;
            one(c, &id)
        })
    }

    /// The wave that is running or waiting for its outcome, if any.
    pub fn wave_active(&self) -> Result<Option<Value>> {
        self.read(active)
    }

    /// Newest first.
    pub fn wave_list(&self, limit: usize) -> Result<Vec<Value>> {
        self.read(|c| Ok(rows(c, &format!("SELECT {COLS} FROM urge ORDER BY started_at DESC LIMIT {}", limit.clamp(1, 500)), &[])?.into_iter().map(fix).collect()))
    }

    /// Beaten next to slips, by hour, trigger, place and action, over the last `days` days.
    pub fn wave_stats(&self, today: NaiveDate, days: i64) -> Result<Value> {
        let days = days.clamp(1, 365);
        let from = today - Duration::days(days - 1);
        let all: Vec<Value> = self.read(|c| {
            Ok(rows(c, &format!("SELECT {COLS} FROM urge WHERE date >= ?1 AND date <= ?2 ORDER BY started_at"), &[s(from.to_string()), s(today.to_string())])?
                .into_iter()
                .map(fix)
                .collect())
        })?;
        let n_of = |day: Option<&str>, o: &str| all.iter().filter(|u| day.is_none_or(|d| u["date"] == d) && u["outcome"] == o).count();
        let t = today.to_string();
        let group = |key: &dyn Fn(&Value) -> Option<String>| -> Vec<Value> {
            let mut m: std::collections::BTreeMap<String, (usize, usize, usize)> = Default::default();
            for u in &all {
                if let Some(k) = key(u) {
                    let e = m.entry(k).or_default();
                    e.0 += 1;
                    e.1 += (u["outcome"] == "beaten") as usize;
                    e.2 += (u["outcome"] == "slipped") as usize;
                }
            }
            let mut v: Vec<_> = m.into_iter().collect();
            v.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
            v.into_iter().map(|(k, (n, b, sl))| json!({ "name": k, "urges": n, "beaten": b, "slipped": sl })).collect()
        };
        let by_hour: Vec<Value> = (0..24)
            .map(|h| {
                let of: Vec<&Value> = all.iter().filter(|u| u["hour"] == h).collect();
                json!({ "hour": h, "urges": of.len(), "beaten": of.iter().filter(|u| u["outcome"] == "beaten").count(), "slipped": of.iter().filter(|u| u["outcome"] == "slipped").count() })
            })
            .collect();
        let peak = by_hour.iter().filter(|h| h["urges"].as_u64().unwrap_or(0) > 0).max_by_key(|h| h["urges"].as_u64().unwrap_or(0)).map(|h| h["hour"].clone());
        let ints: Vec<i64> = all.iter().filter_map(|u| u["intensity"].as_i64()).collect();
        let avg = if ints.is_empty() { Value::Null } else { json!((ints.iter().sum::<i64>() as f64 / ints.len() as f64 * 10.0).round() / 10.0) };
        let last_slip = self.read(|c| Ok(c.query_row("SELECT MAX(date) FROM urge WHERE outcome = 'slipped'", [], |r| r.get::<_, Option<String>>(0))?))?;
        let since_slip = last_slip.and_then(|d| d.parse::<NaiveDate>().ok()).map(|d| (today - d).num_days());
        let guard = self.read(|c| {
            let asked: i64 = c.query_row("SELECT COUNT(*) FROM wave_guard WHERE date >= ?1", [from.to_string()], |r| r.get(0))?;
            let docked: i64 = c.query_row("SELECT COUNT(*) FROM wave_guard WHERE date >= ?1 AND docked = 1", [from.to_string()], |r| r.get(0))?;
            Ok(json!({ "asked": asked, "docked": docked }))
        })?;
        Ok(json!({
            "days": days,
            "urges": all.len(),
            "beaten": n_of(None, "beaten"), "strong": n_of(None, "strong"), "slipped": n_of(None, "slipped"),
            "open": all.iter().filter(|u| u["outcome"].is_null()).count(),
            "today": { "urges": all.iter().filter(|u| u["date"] == t.as_str()).count(), "beaten": n_of(Some(&t), "beaten"), "slipped": n_of(Some(&t), "slipped") },
            "avg_intensity": avg,
            "peak_hour": peak,
            "days_since_slip": since_slip,
            "by_hour": by_hour,
            "by_trigger": group(&|u| u["trigger_kind"].as_str().filter(|x| !x.is_empty()).map(String::from)),
            "by_place": group(&|u| u["place"].as_str().map(String::from)),
            "by_action": group(&|u| u["action"].as_str().filter(|x| !x.is_empty()).map(String::from)),
            "guard": guard,
        }))
    }

    /// Short text for Nora (call context, the night journal): the week in numbers and the last urges.
    pub fn wave_context(&self, today: NaiveDate, max: usize) -> Result<String> {
        let st = self.wave_stats(today, 7)?;
        if st["urges"].as_u64().unwrap_or(0) == 0 {
            return Ok("Wave (last 7 days): no urges logged.".into());
        }
        let mut out = vec![format!(
            "Wave (last 7 days): {} urges: {} beaten, {} still strong, {} slipped. Today: {} urges, {} beaten, {} slipped.{}{}",
            st["urges"], st["beaten"], st["strong"], st["slipped"], st["today"]["urges"], st["today"]["beaten"], st["today"]["slipped"],
            st["peak_hour"].as_i64().map(|h| format!(" Busiest hour: {h:02}:00.")).unwrap_or_default(),
            st["days_since_slip"].as_i64().map(|d| format!(" Days since last slip: {d}.")).unwrap_or_default(),
        )];
        for u in self.wave_list(6)? {
            let at = DateTime::parse_from_rfc3339(u["started_at"].as_str().unwrap_or("")).map(|d| d.with_timezone(&Local).format("%a %H:%M").to_string()).unwrap_or_default();
            let feel = u["feelings"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("/")).unwrap_or_default();
            out.push(format!(
                "- {at}: {}; intensity {}; {}; {}; trigger {}; felt {}; did {}{}{}",
                u["outcome"].as_str().unwrap_or("open"),
                u["intensity"].as_i64().map(|i| i.to_string()).unwrap_or_else(|| "?".into()),
                u["place"].as_str().unwrap_or("place ?"),
                match u["alone"].as_bool() {
                    Some(true) => "alone",
                    Some(false) => "not alone",
                    None => "alone ?",
                },
                u["trigger_kind"].as_str().unwrap_or("?"),
                if feel.is_empty() { "?" } else { &feel },
                u["action"].as_str().unwrap_or("nothing logged"),
                u["lie"].as_str().filter(|x| !x.is_empty()).map(|l| format!("; his brain said \"{l}\"")).unwrap_or_default(),
                u["truth"].as_str().filter(|x| !x.is_empty()).map(|l| format!(", his truth \"{l}\"")).unwrap_or_default(),
            ));
        }
        let text = out.join("\n");
        Ok(text.chars().take(max).collect())
    }

    // -- settings and the night guard ------------------------------------------------------

    pub fn wave_settings(&self) -> Result<WaveSettings> {
        self.read(|c| {
            let get = |k: &str| -> Result<Option<String>> { Ok(c.query_row("SELECT value FROM wave_setting WHERE key = ?1", [k], |r| r.get(0)).optional()?) };
            Ok(WaveSettings {
                night_guard: get("night_guard")?.as_deref() == Some("on"),
                night_guard_at: get("night_guard_at")?.unwrap_or_else(|| "22:30".into()),
            })
        })
    }

    pub fn wave_set_settings(&self, by: &str, night_guard: Option<bool>, at: Option<&str>) -> Result<WaveSettings> {
        if let Some(a) = at
            && chrono::NaiveTime::parse_from_str(a, "%H:%M").is_err()
        {
            return Err(invalid("night_guard_at must be HH:MM"));
        }
        self.write(by, |tx| {
            let put = |k: &str, v: &str| -> Result<()> {
                tx.execute("INSERT INTO wave_setting (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [k, v])?;
                Ok(())
            };
            if let Some(g) = night_guard {
                put("night_guard", if g { "on" } else { "off" })?;
            }
            if let Some(a) = at {
                put("night_guard_at", a)?;
            }
            Ok(())
        })?;
        self.wave_settings()
    }

    /// Record that Nora asked tonight. False if she already did.
    pub fn wave_guard_asked(&self, by: &str, day: NaiveDate) -> Result<bool> {
        self.write(by, |tx| {
            let n = tx.execute("INSERT OR IGNORE INTO wave_guard (date, asked_at) VALUES (?1, ?2)", params![day.to_string(), tx.now()])?;
            Ok(n > 0)
        })
    }

    /// His answer to "phone docked outside the bed?".
    pub fn wave_guard_answer(&self, by: &str, day: NaiveDate, docked: bool, note: Option<&str>) -> Result<Value> {
        self.write(by, |tx| {
            tx.execute(
                "INSERT INTO wave_guard (date, asked_at, docked, note) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(date) DO UPDATE SET docked = excluded.docked, note = COALESCE(excluded.note, note)",
                params![day.to_string(), tx.now(), docked as i64, note],
            )?;
            Ok(json!({ "date": day.to_string(), "docked": docked, "note": note }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn start_is_idempotent_and_updates_validate() {
        let st = Store::open_temporary().unwrap();
        let a = st.wave_start("t", NewWave { id: Some("w1".into()), ..Default::default() }).unwrap();
        assert_eq!(a["id"], "w1");
        let b = st.wave_start("t", NewWave::default()).unwrap();
        assert_eq!(b["id"], "w1", "a running wave comes back instead of a second one");
        let again = st.wave_start("t", NewWave { id: Some("w1".into()), ..Default::default() }).unwrap();
        assert_eq!(again["id"], "w1", "a replayed start is a no-op");
        assert_eq!(st.wave_active().unwrap().unwrap()["id"], "w1");
        assert!(st.wave_update("t", "w1", &obj(json!({ "intensity": 11 }))).is_err());
        assert!(st.wave_update("t", "w1", &obj(json!({ "place": "kitchen" }))).is_err());
        assert!(st.wave_update("t", "w1", &obj(json!({ "colour": "red" }))).is_err());
        let u = st.wave_update("t", "w1", &obj(json!({ "intensity": 7, "place": "desk", "alone": true, "feelings": ["bored", "tired"], "trigger_kind": "late night" }))).unwrap();
        assert_eq!(u["alone"], true);
        assert_eq!(u["feelings"], json!(["bored", "tired"]));
    }

    #[test]
    fn beaten_earns_xp_once_and_slip_takes_it_back() {
        let st = Store::open_temporary().unwrap();
        st.wave_start("t", NewWave { id: Some("w1".into()), ..Default::default() }).unwrap();
        let f = st.wave_finish("t", "w1", "beaten", None).unwrap();
        assert_eq!(f["xp_delta"], WAVE_XP);
        assert_eq!(st.wave_finish("t", "w1", "beaten", None).unwrap()["xp_delta"], 0);
        assert_eq!(st.xp_earned("wave", "w1").unwrap(), WAVE_XP);
        assert!(st.wave_active().unwrap().is_none());
        let s2 = st.wave_finish("t", "w1", "slipped", Some("late")).unwrap();
        assert_eq!(s2["xp_delta"], -WAVE_XP);
        assert_eq!(st.xp_earned("wave", "w1").unwrap(), 0);
        // Another ten minutes reopens a finished wave.
        let later = time::ts(time::now() + Duration::minutes(10));
        let r = st.wave_update("t", "w1", &obj(json!({ "ends_at": later }))).unwrap();
        assert!(r["outcome"].is_null() && r["finished_at"].is_null());
        assert_eq!(st.wave_active().unwrap().unwrap()["id"], "w1");
    }

    #[test]
    fn stats_show_beaten_next_to_slips() {
        let st = Store::open_temporary().unwrap();
        let day = time::local_today();
        for (i, (out, trig)) in [("beaten", "bored"), ("beaten", "bored"), ("slipped", "stress"), ("strong", "bored")].into_iter().enumerate() {
            let id = format!("w{i}");
            st.wave_start("t", NewWave { id: Some(id.clone()), ..Default::default() }).unwrap();
            st.wave_update("t", &id, &obj(json!({ "trigger_kind": trig, "intensity": 6, "place": "bed", "action": "walk" }))).unwrap();
            st.wave_finish("t", &id, out, None).unwrap();
        }
        let s = st.wave_stats(day, 30).unwrap();
        assert_eq!((s["beaten"].as_i64(), s["slipped"].as_i64(), s["strong"].as_i64()), (Some(2), Some(1), Some(1)));
        assert_eq!(s["by_trigger"][0]["name"], "bored");
        assert_eq!(s["by_trigger"][0]["urges"], 3);
        assert_eq!(s["avg_intensity"], 6.0);
        assert_eq!(s["days_since_slip"], 0);
        assert_eq!(s["by_hour"].as_array().unwrap().len(), 24);
        let ctx = st.wave_context(day, 4000).unwrap();
        assert!(ctx.contains("2 beaten") && ctx.contains("1 slipped"));
    }

    #[test]
    fn settings_and_guard() {
        let st = Store::open_temporary().unwrap();
        assert!(!st.wave_settings().unwrap().night_guard);
        assert!(st.wave_set_settings("t", Some(true), Some("25:99")).is_err());
        let s = st.wave_set_settings("t", Some(true), Some("22:00")).unwrap();
        assert!(s.night_guard && s.night_guard_at == "22:00");
        let day = time::local_today();
        assert!(st.wave_guard_asked("t", day).unwrap());
        assert!(!st.wave_guard_asked("t", day).unwrap());
        st.wave_guard_answer("t", day, true, None).unwrap();
        assert_eq!(st.wave_stats(day, 7).unwrap()["guard"]["docked"], 1);
    }
}
