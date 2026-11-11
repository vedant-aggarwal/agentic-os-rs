//! Photo intake: a photo of the marked-up morning sheet goes to Claude with the printed sheet as
//! context. Claude returns a list of changes as JSON; we apply them through the tool registry and
//! post a short summary in the thread. Nothing is guessed: unreadable marks come back as
//! `unclear` and are listed in the summary instead of applied.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use aos_tools::ToolCtx;
use serde::Deserialize;
use serde_json::{Value, json};

use super::sheet::{Sheet, clip1};
use crate::engine::{ImageInput, TurnRequest, ask};
use crate::nora::Nora;
use crate::store::NewMsg;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Changes {
    /// The sheet id printed in the page header, when the photo shows it.
    pub sheet: Option<String>,
    pub ticks: Vec<Tick>,
    pub habits: Vec<HabitMark>,
    pub new_tasks: Vec<NewTask>,
    pub moves: Vec<Move>,
    pub notes: Vec<Note>,
    pub messages: Vec<MsgMark>,
    pub journal: Option<String>,
    pub unclear: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Tick {
    pub n: String,
    /// `done` or `skipped`.
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct HabitMark {
    pub n: String,
    /// `done`, `floor` or `missed`.
    pub status: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct NewTask {
    pub title: String,
    pub area: Option<String>,
    pub due: Option<String>,
    pub scheduled: Option<String>,
    /// Number of the task this is a subtask of.
    pub parent_n: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Move {
    pub n: String,
    pub due: Option<String>,
    pub scheduled: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Note {
    /// The item the note is written next to; empty for a free note.
    pub n: Option<String>,
    pub text: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MsgMark {
    pub n: String,
    /// `handled` when ticked.
    pub action: String,
    pub note: Option<String>,
}

pub const SYSTEM: &str = "You read a photo of a printed morning planning sheet that its owner marked up with a pen, \
and you turn the marks into a list of changes. The photo may be tilted, shadowed or in poor light. Reply with one JSON \
object only, no prose, no code fences. Never guess: if a mark or a word is not clearly readable, or it is not clear which \
item it belongs to, put a short description in `unclear` and leave it out of the other lists. Copy handwriting faithfully; \
do not improve or rewrite his words. `unclear` is only for marks that are on the photo but illegible or that you cannot place; \nnever list items that simply have no mark, and never comment on parts of the sheet that are not in the photo.";

/// The text the model reads next to the photo: what was printed, with the numbers, and the schema.
pub fn context_prompt(sheet: &Sheet) -> String {
    let mut s = format!("The printed sheet is `{}` ({} {}). What was printed, with the number beside each box:\n\nTASKS\n", sheet.id, sheet.weekday, sheet.date);
    for g in &sheet.groups {
        s.push_str(&format!("[{}{}{}]\n", g.pillar, if g.pillar.is_empty() { "" } else { " / " }, g.area));
        for t in &g.tasks {
            s.push_str(&format!("  {}  {}\n", t.n, t.title));
            for sub in &t.subs {
                s.push_str(&format!("    {}  {}\n", sub.n, sub.title));
            }
        }
    }
    s.push_str("\nHABITS\n");
    for h in &sheet.habits {
        s.push_str(&format!("  {}  {}\n", h.n, h.name));
    }
    s.push_str("\nMESSAGES\n");
    for m in &sheet.messages {
        s.push_str(&format!("  {}  {}: {}\n", m.n, m.from, clip1(&m.text, 80)));
    }
    s.push_str("\nAREAS you may use for new tasks (use the id): ");
    s.push_str(&sheet.areas.iter().map(|(id, name)| format!("{id} ({name})")).collect::<Vec<_>>().join(", "));
    s.push_str(
        "\n\nPaper conventions: a tick or filled box = done; an X or a cross-out with a reason = skipped; an arrow with a day = move \
to that day; lines written under the dotted '+' lines or in 'New tasks' = new tasks (the word before the task is its area); \
anything else written next to an item = a note. Today is ",
    );
    s.push_str(&format!("{}.\n\nReturn this JSON (omit nothing; use empty lists):\n", sheet.id));
    s.push_str(
        r#"{
 "sheet":    "the sheet id printed in the page header of the photo (like 2026-11-13), or null if you cannot read it",
 "ticks":    [{"n":"3","status":"done|skipped","reason":"words he wrote, only for skipped"}],
 "habits":   [{"n":"H2","status":"done|floor|missed"}],
 "new_tasks":[{"title":"...","area":"area id or null","due":"today|tomorrow|YYYY-MM-DD or null","scheduled":"e.g. today 15:00 or null","parent_n":"3 or null","notes":null}],
 "moves":    [{"n":"5","due":"tomorrow|YYYY-MM-DD","scheduled":null}],
 "notes":    [{"n":"5 or null","text":"..."}],
 "messages": [{"n":"M2","action":"handled","note":null}],
 "journal":  "free writing meant as a journal entry, or null",
 "unclear":  ["short description of anything you could not read or place"]
}"#,
    );
    s
}

/// Pull the JSON object out of a reply that may have text or fences around it.
pub fn parse_changes(text: &str) -> Result<Changes> {
    let (a, b) = (text.find('{'), text.rfind('}'));
    let (Some(a), Some(b)) = (a, b) else { bail!("the reply had no JSON") };
    serde_json::from_str(&text[a..=b]).context("the reply was not the expected JSON")
}

#[derive(Debug, Default)]
pub struct Applied {
    pub done: u32,
    pub skipped: Vec<String>,
    pub habits: u32,
    pub new_tasks: Vec<String>,
    pub moved: u32,
    pub notes: u32,
    pub messages: u32,
    pub journal: bool,
    pub unclear: Vec<String>,
    pub errors: Vec<String>,
}

impl Applied {
    /// Two or three plain lines for the thread.
    pub fn summary(&self, sheet_id: &str) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.done > 0 {
            parts.push(format!("{} done", self.done));
        }
        if !self.skipped.is_empty() {
            parts.push(format!("{} skipped ({})", self.skipped.len(), self.skipped.join(", ")));
        }
        if self.habits > 0 {
            parts.push(format!("{} habit{} logged", self.habits, if self.habits == 1 { "" } else { "s" }));
        }
        if !self.new_tasks.is_empty() {
            parts.push(format!("{} new: {}", self.new_tasks.len(), self.new_tasks.join("; ")));
        }
        if self.moved > 0 {
            parts.push(format!("{} moved", self.moved));
        }
        if self.notes > 0 {
            parts.push(format!("{} note{}", self.notes, if self.notes == 1 { "" } else { "s" }));
        }
        if self.messages > 0 {
            parts.push(format!("{} message{} marked handled", self.messages, if self.messages == 1 { "" } else { "s" }));
        }
        if self.journal {
            parts.push("journal entry saved".into());
        }
        let mut s = if parts.is_empty() { format!("Read your sheet {sheet_id}: I found no marks to apply.") } else { format!("Read your sheet {sheet_id}: {}.", parts.join(", ")) };
        if !self.unclear.is_empty() {
            s.push_str(&format!("\nCouldn't read: {}. Tell me and I'll add them.", self.unclear.join("; ")));
        }
        if !self.errors.is_empty() {
            s.push_str(&format!("\nNot applied: {}.", self.errors.join("; ")));
        }
        s
    }
}

/// Apply the changes through the same tools the app and agents use.
pub fn apply(nora: &Nora, sheet: &Sheet, ch: &Changes) -> Applied {
    let store = nora.core();
    let mut reg = aos_tools::registry();
    reg.extend(aos_inbox::tools::tools());
    let ctx = ToolCtx::new(store, "morning-photo");
    let mut out = Applied::default();

    let mut tasks: HashMap<String, (String, String)> = HashMap::new(); // n -> (id, title)
    for g in &sheet.groups {
        for t in &g.tasks {
            tasks.insert(t.n.clone(), (t.id.clone(), t.title.clone()));
            for s in &t.subs {
                tasks.insert(s.n.clone(), (s.id.clone(), s.title.clone()));
            }
        }
    }
    let habits: HashMap<&str, &str> = sheet.habits.iter().map(|h| (h.n.as_str(), h.id.as_str())).collect();
    let msgs: HashMap<&str, &str> = sheet.messages.iter().map(|m| (m.n.as_str(), m.id.as_str())).collect();
    let key = |n: &str| n.trim().trim_matches(['(', ')', '#']).to_ascii_uppercase();

    for t in &ch.ticks {
        let n = key(&t.n);
        let Some((id, title)) = tasks.get(&n) else {
            out.unclear.push(format!("a mark on '{}' (no such item)", t.n));
            continue;
        };
        match t.status.as_str() {
            "done" => match reg.call(&ctx, "task_done", json!({ "id": id, "evidence": format!("ticked on paper sheet {}", sheet.id) })) {
                Ok(_) => out.done += 1,
                Err(e) => out.errors.push(format!("{title}: {e}")),
            },
            "skipped" => {
                let reason = t.reason.clone().filter(|r| !r.trim().is_empty());
                let args = json!({ "id": id, "reason": reason.clone().unwrap_or_else(|| "crossed out, no reason written".into()), "genuine": reason.is_some() });
                match reg.call(&ctx, "task_skip", args) {
                    Ok(_) => out.skipped.push(match reason {
                        Some(r) => format!("{}: {}", clip1(title, 24), clip1(&r, 30)),
                        None => format!("{}: no reason", clip1(title, 24)),
                    }),
                    Err(e) => out.errors.push(format!("{title}: {e}")),
                }
            }
            other => out.unclear.push(format!("{title}: unknown status '{other}'")),
        }
    }
    for h in &ch.habits {
        let Some(id) = habits.get(key(&h.n).as_str()) else {
            out.unclear.push(format!("a mark on habit '{}'", h.n));
            continue;
        };
        match reg.call(&ctx, "habit_log", json!({ "habit": id, "status": h.status })) {
            Ok(_) => out.habits += 1,
            Err(e) => out.errors.push(format!("habit {}: {e}", h.n)),
        }
    }
    for t in &ch.new_tasks {
        if t.title.trim().is_empty() {
            continue;
        }
        let mut args = json!({ "title": t.title.trim() });
        let known_area = t.area.as_ref().filter(|a| store.resolve_area(a).is_ok());
        if let Some(a) = known_area {
            args["area"] = json!(a);
        }
        for (k, v) in [("due", &t.due), ("scheduled", &t.scheduled), ("notes", &t.notes)] {
            if let Some(v) = v.as_ref().filter(|v| !v.trim().is_empty() && !v.eq_ignore_ascii_case("null")) {
                args[k] = json!(v);
            }
        }
        if args.get("due").is_none() && args.get("scheduled").is_none() {
            args["due"] = json!("today");
        }
        if let Some((pid, _)) = t.parent_n.as_deref().and_then(|p| tasks.get(&key(p))) {
            args["parent"] = json!(pid);
        }
        match reg.call(&ctx, "task_add", args) {
            Ok(_) => out.new_tasks.push(format!("{}{}", clip1(&t.title, 40), known_area.map(|a| format!(" [{a}]")).unwrap_or_default())),
            Err(e) => out.errors.push(format!("{}: {e}", t.title)),
        }
    }
    for m in &ch.moves {
        let Some((id, title)) = tasks.get(&key(&m.n)) else {
            out.unclear.push(format!("a move on '{}'", m.n));
            continue;
        };
        let mut args = json!({ "id": id });
        if let Some(d) = m.due.as_ref().filter(|d| !d.trim().is_empty()) {
            args["due"] = json!(d);
        }
        if let Some(s) = m.scheduled.as_ref().filter(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case("null")) {
            args["scheduled"] = json!(s);
        } else if args.get("due").is_some() {
            // Moving the day clears an old scheduled time that would keep it on today's list.
            args["scheduled"] = Value::Null;
        }
        match reg.call(&ctx, "task_update", args) {
            Ok(_) => out.moved += 1,
            Err(e) => out.errors.push(format!("{title}: {e}")),
        }
    }
    for n in &ch.notes {
        if n.text.trim().is_empty() {
            continue;
        }
        let target = n.n.as_deref().filter(|x| !x.trim().is_empty()).and_then(|x| tasks.get(&key(x)));
        let res = match target {
            Some((id, _)) => {
                let prev = store.get_task(id).ok().and_then(|t| t.notes).unwrap_or_default();
                let joined = if prev.is_empty() { n.text.trim().to_string() } else { format!("{prev}\n{}", n.text.trim()) };
                reg.call(&ctx, "task_update", json!({ "id": id, "notes": joined }))
            }
            None => reg.call(&ctx, "journal_add", json!({ "text": n.text.trim(), "kind": "note" })),
        };
        match res {
            Ok(_) => out.notes += 1,
            Err(e) => out.errors.push(format!("note: {e}")),
        }
    }
    for m in &ch.messages {
        if m.action != "handled" {
            continue;
        }
        let Some(id) = msgs.get(key(&m.n).as_str()) else { continue };
        match reg.call(&ctx, "inbox_mark", json!({ "id": id, "note": m.note })) {
            Ok(_) => out.messages += 1,
            Err(e) => out.errors.push(format!("message {}: {e}", m.n)),
        }
    }
    if let Some(j) = ch.journal.as_ref().filter(|j| !j.trim().is_empty() && !j.eq_ignore_ascii_case("null")) {
        match reg.call(&ctx, "journal_add", json!({ "text": j.trim() })) {
            Ok(_) => out.journal = true,
            Err(e) => out.errors.push(format!("journal: {e}")),
        }
    }
    out.unclear.extend(ch.unclear.iter().filter(|u| !u.trim().is_empty()).map(|u| clip1(u, 80)));
    out
}

/// Guess the media type from the file's first bytes.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    }
}

/// The whole intake: read the photo with Claude, apply the changes, post the summary.
/// `sheet_id` picks a stored sheet; `None` uses the newest one.
pub async fn process(nora: &Nora, bytes: Vec<u8>, sheet_id: Option<&str>) -> Result<Value> {
    process_inner(nora, bytes, sheet_id, true).await
}

async fn process_inner(nora: &Nora, bytes: Vec<u8>, sheet_id: Option<&str>, may_retry: bool) -> Result<Value> {
    anyhow::ensure!(!bytes.is_empty(), "the photo is empty");
    let media_type = sniff(&bytes).context("that file is not a JPEG, PNG, WebP or GIF image")?;
    let sheet = super::load_sheet(nora, sheet_id)?.context("no morning sheet has been printed yet, so there is nothing to match the photo to")?;

    // Keep the photo next to the sheet.
    let dir = super::print_dir().join("photos");
    let ext = media_type.rsplit('/').next().unwrap_or("jpg");
    let saved = dir.join(format!("{}-{}.{ext}", sheet.id, chrono::Local::now().format("%H%M%S")));
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(&saved, &bytes);
    }

    let req = TurnRequest {
        system: SYSTEM.into(),
        prompt: context_prompt(&sheet),
        model: nora.config().model.clone(),
        images: vec![ImageInput { media_type: media_type.into(), base64: data_encoding::BASE64.encode(&bytes) }],
        timeout: Some(Duration::from_secs(240)),
        ..Default::default()
    };
    let reply = ask(nora.engine().as_ref(), req).await.map_err(anyhow::Error::msg).context("reading the photo")?;
    let changes = parse_changes(&reply)?;
    // The photo shows an older sheet than the one assumed: read it again against the right one.
    if may_retry && sheet_id.is_none() {
        let other = changes.sheet.as_deref().map(str::trim).filter(|o| !o.is_empty() && *o != sheet.id).map(String::from);
        if let Some(other) = other {
            if super::load_sheet(nora, Some(&other))?.is_some() {
                return Box::pin(process_inner(nora, bytes, Some(&other), false)).await;
            }
        }
    }
    let (n2, s2) = (nora.clone(), sheet.clone());
    let applied = tokio::task::spawn_blocking(move || apply(&n2, &s2, &changes)).await?;
    let summary = applied.summary(&sheet.id);
    nora.post(NewMsg::new("claude", "photo", "message", summary.clone()))?;
    Ok(json!({
        "sheet": sheet.id,
        "summary": summary,
        "done": applied.done,
        "skipped": applied.skipped.len(),
        "habits": applied.habits,
        "new_tasks": applied.new_tasks.len(),
        "moved": applied.moved,
        "notes": applied.notes,
        "unclear": applied.unclear,
        "errors": applied.errors,
        "photo": saved.to_string_lossy(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morning::sheet::{Group, SheetHabit, SheetTask};
    use crate::nora::testing::nora_with;
    use aos_core::tasks::NewTask as CoreNew;

    #[test]
    fn json_in_fences_and_prose_parses() {
        let c = parse_changes("Here you go:\n```json\n{\"ticks\":[{\"n\":\"1\",\"status\":\"done\"}],\"unclear\":[\"scribble\"]}\n```").unwrap();
        assert_eq!(c.ticks[0].n, "1");
        assert_eq!(c.unclear, vec!["scribble"]);
        assert!(parse_changes("no json here").is_err());
    }

    #[tokio::test]
    async fn photo_of_an_older_sheet_is_read_against_that_sheet() {
        let reply = r#"{"sheet":"2026-03-03","ticks":[{"n":"1","status":"done"}]}"#;
        let (nora, engine, _p) = nora_with(&[reply], None);
        let t = nora.core().add_task("t", CoreNew { title: "Old task".into(), ..Default::default() }).unwrap();
        let mk = |id: &str, task_id: &str, title: &str| Sheet {
            id: id.into(),
            groups: vec![Group { area: "A".into(), tasks: vec![SheetTask { n: "1".into(), id: task_id.into(), title: title.into(), ..Default::default() }], ..Default::default() }],
            ..Default::default()
        };
        super::super::save_sheet(&nora, &mk("2026-03-03", &t.id, "Old task")).unwrap();
        super::super::save_sheet(&nora, &mk("2026-03-04", "nope", "Newer task")).unwrap();
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &[0; 16]].concat();
        let out = process(&nora, png, None).await.unwrap();
        // The newest sheet was assumed first, the header said otherwise, so it was read again.
        assert_eq!(out["sheet"], "2026-03-03");
        assert_eq!(out["done"], 1);
        let seen = engine.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].prompt.contains("Newer task") && seen[1].prompt.contains("Old task"));
    }

    #[test]
    fn sniffs_images() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff(b"\x89PNG\r\n"), Some("image/png"));
        assert_eq!(sniff(b"hello"), None);
    }

    #[test]
    fn applies_marks_through_the_tools() {
        let (nora, _e, _p) = nora_with(&[], None);
        let store = nora.core();
        let day = aos_core::time::local_today();
        let add = |title: &str| store.add_task("t", CoreNew { title: title.into(), area_id: Some("fitness".into()), due: Some(day), ..Default::default() }).unwrap();
        let (a, b, c) = (add("Gym"), add("Read"), add("Call supplier"));
        let habit = store.add_habit("t", aos_core::habits::NewHabit { id: None, name: "Meditate".into(), area_id: None, cue: None, normal: None, floor: None, cadence: "daily".parse().unwrap(), xp: 5, anchor: false, start_date: day }).unwrap();
        let sheet = Sheet {
            id: day.to_string(),
            groups: vec![Group {
                area: "Fitness".into(),
                tasks: vec![
                    SheetTask { n: "1".into(), id: a.id.clone(), title: "Gym".into(), ..Default::default() },
                    SheetTask { n: "2".into(), id: b.id.clone(), title: "Read".into(), ..Default::default() },
                    SheetTask { n: "3".into(), id: c.id.clone(), title: "Call supplier".into(), ..Default::default() },
                ],
                ..Default::default()
            }],
            habits: vec![SheetHabit { n: "H1".into(), id: habit.id.clone(), name: "Meditate".into(), ..Default::default() }],
            areas: vec![("fitness".into(), "Fitness".into()), ("reading".into(), "Reading".into())],
            ..Default::default()
        };
        let ch = parse_changes(
            r#"{"ticks":[{"n":"1","status":"done"},{"n":"2","status":"skipped","reason":"tired"},{"n":"9","status":"done"}],
                "habits":[{"n":"H1","status":"done"}],
                "new_tasks":[{"title":"Pack parcel","area":"reading","due":"today"},{"title":"Stretch","area":"nonsense","parent_n":"3"}],
                "moves":[{"n":"3","due":"tomorrow"}],
                "notes":[{"n":"3","text":"ask about price"},{"n":null,"text":"slept badly"}],
                "unclear":["word after Read"]}"#,
        )
        .unwrap();
        let out = apply(&nora, &sheet, &ch);
        assert_eq!(out.done, 1);
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.habits, 1);
        assert_eq!(out.new_tasks.len(), 2);
        assert_eq!(out.moved, 1);
        assert_eq!(out.notes, 2);
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(out.unclear.len(), 2);
        assert_eq!(store.get_task(&a.id).unwrap().status, aos_core::model::TaskStatus::Done);
        let skipped = store.get_task(&b.id).unwrap();
        assert_eq!(skipped.skip_reason.as_deref(), Some("tired"));
        let moved = store.get_task(&c.id).unwrap();
        assert_ne!(moved.due, Some(day));
        assert_eq!(moved.notes.as_deref(), Some("ask about price"));
        let s = out.summary(&sheet.id);
        assert!(s.contains("1 done") && s.contains("2 new") && s.contains("Couldn't read"), "{s}");
    }

    #[tokio::test]
    async fn process_reads_with_the_engine_and_posts() {
        let reply = r#"{"ticks":[{"n":"1","status":"done"}],"new_tasks":[{"title":"Water plants","area":"fitness"}]}"#;
        let (nora, engine, _p) = nora_with(&[reply], None);
        let day = aos_core::time::local_today();
        let t = nora.core().add_task("t", CoreNew { title: "Gym".into(), due: Some(day), ..Default::default() }).unwrap();
        let sheet = Sheet {
            id: "2026-03-04".into(),
            groups: vec![Group { area: "Fitness".into(), tasks: vec![SheetTask { n: "1".into(), id: t.id.clone(), title: "Gym".into(), ..Default::default() }], ..Default::default() }],
            areas: vec![("fitness".into(), "Fitness".into())],
            ..Default::default()
        };
        super::super::save_sheet(&nora, &sheet).unwrap();
        let png = {
            let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
            v.extend_from_slice(&[0; 16]);
            v
        };
        let out = process(&nora, png, None).await.unwrap();
        assert_eq!(out["done"], 1);
        assert_eq!(out["new_tasks"], 1);
        let seen = engine.seen.lock().unwrap();
        assert_eq!(seen[0].images.len(), 1);
        assert!(seen[0].prompt.contains("1  Gym"));
        drop(seen);
        let last = nora.thread(1, None).unwrap().pop().unwrap();
        assert!(last.text.contains("1 done"), "{}", last.text);
        assert!(process(&nora, b"nope".to_vec(), None).await.is_err());
    }
}
