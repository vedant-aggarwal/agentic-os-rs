//! Nora's memory in three layers (ported from the old app's `nora-memory.ts`):
//!
//! - **Long-term** facts (`L12`): kind + importance 1-5. Saving a near-twin updates it instead of
//!   adding a copy.
//! - **Short-term** loops (`S4`): what's going on now and things to follow up, each with an expiry.
//! - **Episodes**: one short summary per day, written by the nightly consolidation.
//!
//! Every turn sees the important long-term facts, the ones matching the conversation, all live
//! short-term notes and the last three days.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::store::{NoraStore, now_ms};

pub const LONG_KINDS: &[&str] = &["fact", "preference", "person", "project", "goal", "habit", "decision", "health", "lesson", "other"];
pub const SHORT_KINDS: &[&str] = &["now", "followup", "context"];
const DAY_MS: i64 = 86_400_000;

const STOP: &str = "the and for that this with you your are was were have has had not but can will just about what when where how who \
from they them then than there their into out our its it's i'm im me my mine a an to of in on at is be do does did so if or as by up \
hai hain ka ki ke ko se mein me main bhi aur kya nahi toh tha thi";

fn words(t: &str) -> Vec<String> {
    let stop: HashSet<&str> = STOP.split_whitespace().collect();
    let mut seen = HashSet::new();
    t.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.chars().count() > 2 && !stop.contains(w))
        .map(|w| {
            for suf in ["ing", "ed", "es", "s"] {
                if let Some(s) = w.strip_suffix(suf) {
                    return s.to_string();
                }
            }
            w.to_string()
        })
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Memory {
    pub id: String,
    pub text: String,
    pub kind: String,
    pub importance: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShortNote {
    pub id: String,
    pub text: String,
    pub kind: String,
    pub expires_at: i64,
}

/// What `remember` takes.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct RememberArgs {
    /// The fact or note, in one sentence.
    pub text: String,
    /// `long` (default: a lasting fact) or `short` (what's going on now / a loop to follow up).
    #[serde(default)]
    pub term: Option<String>,
    /// Long: fact, preference, person, project, goal, habit, decision, health, lesson, other. Short: now, followup, context.
    #[serde(default)]
    pub kind: Option<String>,
    /// Long only: 1 (trivia) to 5 (core). Default 3.
    #[serde(default)]
    pub importance: Option<i64>,
    /// Short only: days until it expires (1-30; default 3, followup 7).
    #[serde(default)]
    pub days: Option<i64>,
}

fn parse_id(id: &str) -> Option<(bool, i64)> {
    let id = id.trim();
    let (short, num) = match id.chars().next()? {
        'S' | 's' => (true, &id[1..]),
        'L' | 'l' => (false, &id[1..]),
        _ => (false, id),
    };
    num.parse().ok().map(|n| (short, n))
}

impl NoraStore {
    pub fn remember(&self, a: &RememberArgs, source: &str) -> Result<Value> {
        let text: String = a.text.trim().chars().take(600).collect();
        if text.is_empty() {
            bail!("nothing to remember");
        }
        let c = self.conn();
        let now = now_ms();
        if a.term.as_deref() == Some("short") {
            let kind = a.kind.as_deref().filter(|k| SHORT_KINDS.contains(k)).unwrap_or("now");
            let days = a.days.unwrap_or(if kind == "followup" { 7 } else { 3 }).clamp(1, 30);
            let exp = now + days * DAY_MS;
            c.execute(
                "INSERT INTO nora_short (text, kind, source, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![text, kind, source, now, exp],
            )?;
            return Ok(json!({ "saved": format!("S{}", c.last_insert_rowid()), "expires_at": exp }));
        }
        let kind = a.kind.as_deref().filter(|k| LONG_KINDS.contains(k)).unwrap_or("fact");
        let importance = a.importance.unwrap_or(3).clamp(1, 5);
        let w: HashSet<String> = words(&text).into_iter().collect();
        let mut stmt = c.prepare("SELECT id, text FROM nora_memory WHERE status = 'active'")?;
        let rows: Vec<(i64, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        let twin = rows.iter().find(|(_, t)| {
            let mw = words(t);
            let shared = mw.iter().filter(|x| w.contains(*x)).count();
            shared >= 3 && shared as f64 / (w.len().max(mw.len()) as f64) >= 0.7
        });
        if let Some((id, _)) = twin {
            c.execute(
                "UPDATE nora_memory SET text = ?2, importance = max(importance, ?3), updated_at = ?4 WHERE id = ?1",
                params![id, text, importance, now],
            )?;
            return Ok(json!({ "updated": format!("L{id}") }));
        }
        c.execute(
            "INSERT INTO nora_memory (text, kind, importance, source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![text, kind, importance, source, now],
        )?;
        Ok(json!({ "saved": format!("L{}", c.last_insert_rowid()) }))
    }

    /// Mark a memory as no longer true (`L12` or `S4`).
    pub fn forget(&self, id: &str) -> Result<Value> {
        let Some((short, n)) = parse_id(id) else { bail!("memory ids look like L12 or S4") };
        let table = if short { "nora_short" } else { "nora_memory" };
        let changed = self
            .conn()
            .execute(&format!("UPDATE {table} SET status = 'forgotten' WHERE id = ?1 AND status = 'active'"), [n])?;
        if changed == 0 {
            bail!("no active memory {id}");
        }
        Ok(json!({ "forgotten": id }))
    }

    /// Live short-term notes; expired ones are closed on the way.
    pub fn short_term(&self) -> Result<Vec<ShortNote>> {
        let c = self.conn();
        c.execute("UPDATE nora_short SET status = 'expired' WHERE status = 'active' AND expires_at < ?1", [now_ms()])?;
        let mut stmt = c.prepare("SELECT id, text, kind, expires_at FROM nora_short WHERE status = 'active' ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(ShortNote { id: format!("S{}", r.get::<_, i64>(0)?), text: r.get(1)?, kind: r.get(2)?, expires_at: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Long-term memories ranked for a query: word overlap, then importance, then recency.
    pub fn recall(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let c = self.conn();
        let mut stmt = c.prepare("SELECT id, text, kind, importance, updated_at FROM nora_memory WHERE status = 'active'")?;
        let rows: Vec<Memory> = stmt
            .query_map([], |r| {
                Ok(Memory { id: format!("L{}", r.get::<_, i64>(0)?), text: r.get(1)?, kind: r.get(2)?, importance: r.get(3)?, updated_at: r.get(4)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        let q: HashSet<String> = words(query).into_iter().collect();
        let now = now_ms();
        let mut scored: Vec<(f64, usize, Memory)> = rows
            .into_iter()
            .map(|m| {
                let hits = words(&m.text).iter().filter(|w| q.contains(*w)).count();
                let fresh = if now - m.updated_at < 14 * DAY_MS { 1.0 } else { 0.0 };
                (hits as f64 * 4.0 + m.importance as f64 * 1.5 + fresh, hits, m)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut keep: Vec<Memory> = Vec::new();
        let mut ids = HashSet::new();
        // Always the important ones, then what matches, then the rest by score.
        let important = scored.iter().filter(|s| s.2.importance >= 4).take(25);
        let matching = scored.iter().filter(|s| s.1 > 0).take(20);
        for s in important.chain(matching).chain(scored.iter()) {
            if keep.len() >= limit {
                break;
            }
            if ids.insert(s.2.id.clone()) {
                keep.push(s.2.clone());
            }
        }
        if !keep.is_empty() {
            let nums: Vec<String> = keep.iter().map(|m| m.id[1..].to_string()).collect();
            c.execute(&format!("UPDATE nora_memory SET last_used = ?1 WHERE id IN ({})", nums.join(",")), [now])?;
        }
        Ok(keep)
    }

    pub fn episodes(&self, n: usize) -> Result<Vec<(String, String)>> {
        let c = self.conn();
        let mut stmt = c.prepare("SELECT day, summary FROM nora_episode ORDER BY day DESC LIMIT ?1")?;
        let mut rows: Vec<(String, String)> = stmt.query_map([n as i64], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub fn set_episode(&self, day: &str, summary: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO nora_episode (day, summary, at) VALUES (?1, ?2, ?3) ON CONFLICT(day) DO UPDATE SET summary = excluded.summary, at = excluded.at",
            params![day, summary, now_ms()],
        )?;
        Ok(())
    }

    /// The memory part of every prompt.
    pub fn memory_context(&self, query: &str, owner: &str) -> Result<String> {
        let long = self.recall(query, 40)?;
        let mut by_kind: Vec<(String, Vec<&Memory>)> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for m in &long {
            let i = *index.entry(m.kind.clone()).or_insert_with(|| {
                by_kind.push((m.kind.clone(), Vec::new()));
                by_kind.len() - 1
            });
            by_kind[i].1.push(m);
        }
        let mut out = format!("Long-term memory about {owner} (use `recall` to search more):\n");
        if by_kind.is_empty() {
            out.push_str("- nothing yet\n");
        }
        for (kind, ms) in by_kind {
            out.push_str(&format!("{kind}:\n"));
            for m in ms {
                let imp = if m.importance >= 4 { ", important" } else { "" };
                out.push_str(&format!("- ({}{imp}) {}\n", m.id, m.text));
            }
        }
        out.push_str("Short-term (what's going on now, open loops):\n");
        let st = self.short_term()?;
        if st.is_empty() {
            out.push_str("- nothing right now\n");
        }
        for s in st {
            out.push_str(&format!("- ({}, {}, until {}) {}\n", s.id, s.kind, crate::prompt::local_day(s.expires_at), s.text));
        }
        let eps = self.episodes(3)?;
        if !eps.is_empty() {
            out.push_str("Recent days:\n");
            for (d, s) in eps {
                out.push_str(&format!("- {d}: {s}\n"));
            }
        }
        Ok(out)
    }
}

// --- nightly consolidation --------------------------------------------------------------------

/// The consolidation prompt for one day's messages.
pub fn consolidation_prompt(day: &str, transcript: &str, short: &[ShortNote]) -> String {
    let notes = short.iter().map(|s| format!("- {} ({}) {}", s.id, s.kind, s.text)).collect::<Vec<_>>().join("\n");
    format!(
        "Consolidate Nora's memory for {day}.\n\nThe day's thread:\n{transcript}\n\nLive short-term notes:\n{notes}\n\n\
Reply with JSON only, no prose:\n\
{{\"episode\": \"2-4 sentences: what happened today and how he was\", \
\"remember\": [{{\"text\": \"a lasting fact\", \"kind\": \"fact|preference|person|project|goal|habit|decision|health|lesson|other\", \"importance\": 1-5}}], \
\"expire\": [\"S ids that are done or stale\"]}}\n\
Only facts that will still matter next month go in remember. Empty lists are fine."
    )
}

/// Apply the model's consolidation JSON. Returns what changed.
pub fn apply_consolidation(store: &NoraStore, day: &str, reply: &str) -> Result<Value> {
    let start = reply.find('{').unwrap_or(0);
    let end = reply.rfind('}').map(|i| i + 1).unwrap_or(reply.len());
    let v: Value = serde_json::from_str(&reply[start..end])?;
    let mut saved = 0;
    if let Some(ep) = v["episode"].as_str().filter(|s| !s.trim().is_empty()) {
        store.set_episode(day, ep.trim())?;
    }
    for r in v["remember"].as_array().into_iter().flatten() {
        if let Some(text) = r["text"].as_str() {
            let a = RememberArgs {
                text: text.into(),
                kind: r["kind"].as_str().map(String::from),
                importance: r["importance"].as_i64(),
                ..Default::default()
            };
            if store.remember(&a, "consolidation").is_ok() {
                saved += 1;
            }
        }
    }
    let mut expired = 0;
    for id in v["expire"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if id.starts_with(['S', 's']) && store.forget(id).is_ok() {
            expired += 1;
        }
    }
    Ok(json!({ "day": day, "episode": v["episode"], "remembered": saved, "expired": expired }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::temp_store;

    #[test]
    fn remember_merges_twins_and_recall_ranks() {
        let (s, _p) = temp_store();
        let a = RememberArgs { text: "He goes to the gym every morning at six".into(), importance: Some(4), ..Default::default() };
        assert!(s.remember(&a, "t").unwrap()["saved"].is_string());
        let b = RememberArgs { text: "He goes to the gym every morning at seven".into(), ..Default::default() };
        assert_eq!(s.remember(&b, "t").unwrap()["updated"], "L1");
        s.remember(&RememberArgs { text: "Dad runs the shop".into(), kind: Some("person".into()), ..Default::default() }, "t").unwrap();
        let r = s.recall("shop", 1).unwrap();
        assert_eq!(r.len(), 1);
        // importance 4 is always kept first
        assert!(r[0].text.contains("gym"));
        let r = s.recall("shop", 5).unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn short_term_forget_and_context() {
        let (s, _p) = temp_store();
        let out = s.remember(&RememberArgs { text: "Follow up on the invoice".into(), term: Some("short".into()), kind: Some("followup".into()), ..Default::default() }, "t").unwrap();
        assert_eq!(out["saved"], "S1");
        let ctx = s.memory_context("invoice", "Him").unwrap();
        assert!(ctx.contains("(S1, followup"));
        s.forget("S1").unwrap();
        assert!(s.short_term().unwrap().is_empty());
        assert!(s.forget("S1").is_err());
    }

    #[test]
    fn consolidation_applies_json() {
        let (s, _p) = temp_store();
        s.remember(&RememberArgs { text: "Waiting on the courier".into(), term: Some("short".into()), ..Default::default() }, "t").unwrap();
        let reply = r#"Here: {"episode":"Quiet day, shipped the server.","remember":[{"text":"Prefers short replies","kind":"preference","importance":4}],"expire":["S1"]}"#;
        let out = apply_consolidation(&s, "2026-11-12", reply).unwrap();
        assert_eq!(out["remembered"], 1);
        assert_eq!(out["expired"], 1);
        assert_eq!(s.episodes(3).unwrap()[0].1, "Quiet day, shipped the server.");
    }
}
