//! The morning sheet: what goes on the paper (data), how it looks (HTML) and how it becomes a
//! PDF (headless Edge or Chrome). The data is also stored as a snapshot so the photo intake can
//! map a pen mark ("3 done", "H2") back to the right task or habit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use aos_core::model::{AlarmKind, AlarmStatus, HabitLogStatus, TaskStatus};
use aos_core::streak::StreakState;
use aos_tools::ToolCtx;
use chrono::{DateTime, Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::nora::Nora;
use crate::prompt;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    /// `YYYY-MM-DD`; also the sheet's id on paper.
    pub id: String,
    pub date: String,
    pub weekday: String,
    pub schedule: Vec<SchedItem>,
    pub groups: Vec<Group>,
    pub habits: Vec<SheetHabit>,
    pub messages: Vec<SheetMsg>,
    pub alerts: Vec<String>,
    pub analysis: Option<Analysis>,
    pub news: Option<String>,
    /// Area ids and names, for the intake prompt.
    pub areas: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SchedItem {
    /// `HH:MM`, local.
    pub time: String,
    pub label: String,
    /// `task` or `alarm`.
    pub kind: String,
    /// The number on the paper for tasks (`3`, `3.1`).
    pub n: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub pillar: String,
    pub area: String,
    pub area_id: Option<String>,
    pub tasks: Vec<SheetTask>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetTask {
    /// The number printed next to the box.
    pub n: String,
    pub id: String,
    pub title: String,
    /// Why it is on the list when it is not simply due today ("late: due Tue 10 Nov").
    pub flag: Option<String>,
    pub time: Option<String>,
    pub done_sub: u32,
    pub subs: Vec<SheetTask>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetHabit {
    /// `H1`, `H2`...
    pub n: String,
    pub id: String,
    pub name: String,
    /// "streak 12" or "bent: do it today".
    pub note: String,
    pub floor: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetMsg {
    /// `M1`, `M2`...
    pub n: String,
    pub id: String,
    pub from: String,
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub done: String,
    pub missed: String,
    pub mood: String,
    pub pattern: String,
    pub today: String,
}

/// Zero-width and other invisible characters that newsletters pad their previews with.
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{3164}' | '\u{FEFF}'
    )
}

/// One line of at most `n` characters: whitespace collapsed, invisible characters dropped.
pub fn clip1(s: &str, n: usize) -> String {
    let clean: String = s.chars().filter(|c| !invisible(*c)).collect();
    prompt::clip(&clean.split_whitespace().collect::<Vec<_>>().join(" "), n)
}

fn local_of(at: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(at).ok().map(|t| t.with_timezone(&Local))
}

fn short_day(d: NaiveDate) -> String {
    format!("{} {} {}", d.format("%a"), d.day(), d.format("%b"))
}

/// Collect everything for the paper except Nora's analysis and the news.
pub fn gather(nora: &Nora, day: NaiveDate) -> Result<Sheet> {
    let store = nora.core();
    let now = aos_core::time::now();
    let today = store.today_overview(day, now)?;
    let pillars = store.list_pillars()?;
    let areas = store.list_areas(false)?;
    let private: Vec<String> = aos_core::seed::local_profile(&aos_core::paths::local_seed_path()).map(|p| p.private_habits).unwrap_or_default();

    let mut sheet = Sheet {
        id: day.format("%Y-%m-%d").to_string(),
        date: day.format("%-d %B %Y").to_string(),
        weekday: day.format("%A").to_string(),
        areas: areas.iter().map(|a| (a.id.clone(), a.name.clone())).collect(),
        ..Default::default()
    };

    // Tasks by pillar and area, numbered across the sheet.
    let mut n = 0u32;
    let mut overdue = 0u32;
    for g in &today.areas {
        let mut tasks = Vec::new();
        for node in &g.tasks {
            if !node.task.status.is_active() {
                continue;
            }
            n += 1;
            let flag = match node.task.due {
                Some(d) if d < day => {
                    overdue += 1;
                    Some(format!("late, due {}", short_day(d)))
                }
                _ => None,
            };
            let time = node.task.scheduled.as_deref().filter(|s| s.len() >= 16 && s.starts_with(&sheet.id)).map(|s| s[11..16].to_string());
            let mut st = SheetTask { n: n.to_string(), id: node.task.id.clone(), title: node.task.title.clone(), flag, time, ..Default::default() };
            let mut k = 0;
            for s in &node.subtasks {
                if s.status == TaskStatus::Done {
                    st.done_sub += 1;
                }
                if !s.status.is_active() {
                    continue;
                }
                k += 1;
                let time = s.scheduled.as_deref().filter(|x| x.len() >= 16 && x.starts_with(&sheet.id)).map(|x| x[11..16].to_string());
                st.subs.push(SheetTask { n: format!("{n}.{k}"), id: s.id.clone(), title: s.title.clone(), time, ..Default::default() });
            }
            tasks.push(st);
        }
        if tasks.is_empty() {
            continue;
        }
        let pillar = g.pillar_id.as_ref().and_then(|p| pillars.iter().find(|x| &x.id == p)).map(|p| p.name.clone()).unwrap_or_default();
        sheet.groups.push(Group { pillar, area: g.area.clone().unwrap_or_else(|| "No area".into()), area_id: g.area_id.clone(), tasks });
    }

    // Schedule: timed tasks and alarms for the day.
    for g in &sheet.groups {
        for t in &g.tasks {
            if let Some(time) = &t.time {
                sheet.schedule.push(SchedItem { time: time.clone(), label: t.title.clone(), kind: "task".into(), n: Some(t.n.clone()) });
            }
            for s in &t.subs {
                if let Some(time) = &s.time {
                    sheet.schedule.push(SchedItem { time: time.clone(), label: s.title.clone(), kind: "task".into(), n: Some(s.n.clone()) });
                }
            }
        }
    }
    for a in store.list_alarms(now, false)? {
        if a.status != AlarmStatus::Scheduled {
            continue;
        }
        if let Some(t) = local_of(&a.at).filter(|t| t.date_naive() == day) {
            let label = if a.kind == AlarmKind::Alarm { format!("Alarm: {}", a.label) } else { a.label.clone() };
            sheet.schedule.push(SchedItem { time: t.format("%H:%M").to_string(), label, kind: "alarm".into(), n: None });
        }
    }
    sheet.schedule.sort_by(|a, b| a.time.cmp(&b.time));

    // Habits due today.
    let mut bent = Vec::new();
    for (i, h) in today.habits.iter().filter(|h| h.due_today).enumerate() {
        let hidden = private.iter().any(|p| p == &h.habit.id || p.eq_ignore_ascii_case(&h.habit.name));
        let note = match h.streak.state {
            StreakState::Bent => {
                bent.push(if hidden { "a private habit".to_string() } else { h.habit.name.clone() });
                "bent: do it today".to_string()
            }
            StreakState::Broken => "start again today".to_string(),
            StreakState::Intact if h.streak.current > 0 => format!("streak {}", h.streak.current),
            StreakState::Intact => String::new(),
        };
        sheet.habits.push(SheetHabit {
            n: format!("H{}", i + 1),
            id: h.habit.id.clone(),
            // A sheet on a desk can be read by anyone: private habits print without their name.
            name: if hidden { "Private habit".into() } else { h.habit.name.clone() },
            note,
            floor: if hidden { None } else { h.habit.floor.clone() },
        });
    }

    // Messages that matter (the inbox tool decides which; a missing inbox is just empty).
    let mut open_to_read = 0;
    let mut reg = aos_tools::registry();
    reg.extend(aos_inbox::tools::tools());
    if let Ok(v) = reg.call(&ToolCtx::new(store, "morning"), "inbox_list", json!({ "limit": 5 })) {
        open_to_read = v["open_to_read"].as_i64().unwrap_or(0);
        for (i, it) in v["items"].as_array().cloned().unwrap_or_default().iter().enumerate() {
            let from = ["sender_name", "chat_name", "sender"].iter().filter_map(|k| it[*k].as_str()).find(|s| !s.trim().is_empty()).unwrap_or("?").to_string();
            let body = [it["subject"].as_str().unwrap_or(""), it["text"].as_str().unwrap_or("")].iter().filter(|s| !s.trim().is_empty()).cloned().collect::<Vec<_>>().join(": ");
            sheet.messages.push(SheetMsg {
                n: format!("M{}", i + 1),
                id: it["id"].as_str().unwrap_or("").to_string(),
                from: clip1(&from, 30),
                text: clip1(&body, 110),
                source: it["source"].as_str().unwrap_or("").to_string(),
            });
        }
    }

    // Alerts, from rules.
    if overdue > 0 {
        sheet.alerts.push(format!("{overdue} task{} late. Do, move or drop each one today.", if overdue == 1 { " is" } else { "s are" }));
    }
    if !bent.is_empty() {
        sheet.alerts.push(format!("Never miss twice: {} bent yesterday.", bent.join(", ")));
    }
    if !store.journal_written_on(day - chrono::Duration::days(1))? {
        sheet.alerts.push("No journal entry yesterday. The anchor streak is the first thing to repair.".into());
    }
    if open_to_read > sheet.messages.len() as i64 {
        sheet.alerts.push(format!("{open_to_read} messages are waiting to be read (the page shows the first {}).", sheet.messages.len()));
    }
    if sheet.groups.is_empty() {
        sheet.alerts.push("No tasks on the list for today. Write some below.".into());
    }
    Ok(sheet)
}

/// The facts of one day as plain lines, for Nora's analysis. No model call.
pub fn day_facts(nora: &Nora, day: NaiveDate) -> Result<String> {
    let store = nora.core();
    let t = store.today_overview(day, aos_core::time::now())?;
    let mut out = format!("Date: {} ({})\n", day, day.format("%A"));
    out.push_str(&format!("Tasks: {} done, {} skipped, {} still open ({} counted)\n", t.score.done, t.score.skipped, t.score.open, t.score.counted));
    for g in &t.areas {
        let area = g.area.clone().unwrap_or_else(|| "no area".into());
        for node in &g.tasks {
            let st = &node.task;
            let line = match st.status {
                TaskStatus::Done => format!("- DONE [{area}] {}", st.title),
                TaskStatus::Skipped => format!(
                    "- SKIPPED [{area}] {} (reason: {}{})",
                    st.title,
                    st.skip_reason.as_deref().unwrap_or("none given"),
                    match st.reason_genuine {
                        Some(false) => ", he said it was not the real reason",
                        _ => "",
                    }
                ),
                s if s.is_active() => format!("- NOT DONE [{area}] {} (no reason given yet)", st.title),
                _ => continue,
            };
            out.push_str(&line);
            out.push('\n');
        }
    }
    for h in t.habits.iter().filter(|h| h.due_today) {
        let s = match h.today {
            Some(HabitLogStatus::Done) => "done",
            Some(HabitLogStatus::Floor) => "floor version",
            Some(HabitLogStatus::Missed) => "missed",
            Some(HabitLogStatus::Excused) => "excused",
            None => "not logged",
        };
        out.push_str(&format!("- habit {}: {s}, streak {}\n", h.habit.name, h.streak.current));
    }
    for m in &t.moods {
        out.push_str(&format!("- mood {:+.1}, energy {}{}{}\n", m.valence, m.energy.map(|e| e.to_string()).unwrap_or_else(|| "?".into()), if m.labels.is_empty() { String::new() } else { format!(", {}", m.labels.join("/")) }, m.note.as_deref().map(|n| format!(", note: {n}")).unwrap_or_default()));
    }
    out.push_str(&format!("Journal written: {}\nXP earned: {}\n", if t.journal_written { "yes" } else { "no" }, t.xp.today));
    // What he said to Nora that day.
    let start = day.and_hms_opt(0, 0, 0).and_then(|d| d.and_local_timezone(Local).earliest()).map(|d| d.timestamp_millis()).unwrap_or(0);
    let msgs = nora.db().msgs_between(start, start + 86_400_000 - 1)?;
    let said: Vec<String> = msgs.iter().filter(|m| m.engine == "user").map(|m| format!("{}: {}", prompt::local_time(m.at), clip1(&m.text, 200))).collect();
    if !said.is_empty() {
        out.push_str("What he told Nora that day:\n");
        out.push_str(&prompt::clip(&said.join("\n"), 3000));
        out.push('\n');
    }
    Ok(out)
}

/// Parse Nora's labelled reply; any text that does not fit the labels lands in `done`.
pub fn parse_analysis(text: &str) -> Analysis {
    let mut a = Analysis::default();
    let mut cur: Option<&str> = None;
    let mut loose = String::new();
    for line in text.lines() {
        let l = line.trim().trim_start_matches(['*', '-', ' ']).trim();
        let up = l.to_ascii_uppercase();
        let hit = [("DONE", "done"), ("MISSED", "missed"), ("MOOD", "mood"), ("PATTERN", "pattern"), ("TODAY", "today"), ("NEXT", "today")]
            .iter()
            .find(|(k, _)| up.starts_with(k) && l[k.len()..].trim_start_matches(['*']).starts_with([':', '-']));
        if let Some((k, field)) = hit {
            cur = Some(field);
            let rest = l[k.len()..].trim_start_matches('*').trim_start_matches([':', '-', ' ']).trim_start_matches('*').trim().to_string();
            set_field(&mut a, field, &rest);
        } else if !l.is_empty() {
            match cur {
                Some(f) => set_field(&mut a, f, l),
                None => {
                    loose.push_str(l);
                    loose.push(' ');
                }
            }
        }
    }
    if a == Analysis::default() {
        a.done = loose.trim().to_string();
    }
    a
}

fn set_field(a: &mut Analysis, field: &str, text: &str) {
    let slot = match field {
        "done" => &mut a.done,
        "missed" => &mut a.missed,
        "mood" => &mut a.mood,
        "pattern" => &mut a.pattern,
        _ => &mut a.today,
    };
    if !slot.is_empty() {
        slot.push(' ');
    }
    slot.push_str(text);
}

/// Facts-only analysis for when the model is unavailable, so the page is never empty.
pub fn fallback_analysis(nora: &Nora, day: NaiveDate) -> Analysis {
    let facts = day_facts(nora, day).unwrap_or_default();
    let count = |p: &str| facts.lines().filter(|l| l.starts_with(p)).count();
    Analysis {
        done: format!("{} task(s) done.", count("- DONE")),
        missed: format!("{} skipped, {} not done.", count("- SKIPPED"), count("- NOT DONE")),
        mood: "Not analysed (Nora's model was unavailable this morning).".into(),
        pattern: String::new(),
        today: String::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// HTML

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const CSS: &str = r#"
@page { size: A4; margin: 9mm 12mm 10mm; }
* { box-sizing: border-box; }
html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }
body { margin: 0; font-family: "Segoe UI", Arial, sans-serif; color: #000; font-size: 10.5pt; line-height: 1.25; }
.page { page-break-after: always; }
.page:last-child { page-break-after: auto; }
header.top { display: flex; justify-content: space-between; align-items: flex-end; border-bottom: 2px solid #000; padding-bottom: 2mm; margin-bottom: 3mm; }
header.top h1 { font-size: 24pt; margin: 0; font-weight: 700; letter-spacing: -0.5px; }
header.top h1 small { font-size: 12pt; font-weight: 400; margin-left: 3mm; }
header.top .meta { font-size: 8pt; text-align: right; line-height: 1.3; }
h2 { font-size: 10.5pt; text-transform: uppercase; letter-spacing: 1.2px; margin: 4mm 0 1.5mm; border-bottom: 1px solid #000; padding-bottom: 0.6mm; }
h3 { font-size: 10.5pt; margin: 3mm 0 1mm; }
h3 span { font-weight: 400; font-size: 8.5pt; text-transform: uppercase; letter-spacing: 1px; }
.pillar { font-size: 9pt; text-transform: uppercase; letter-spacing: 1.6px; font-weight: 700; background: #000; color: #fff; padding: 0.8mm 2mm; margin: 4mm 0 0; }
.box { display: inline-block; width: 4.6mm; height: 4.6mm; border: 1.4px solid #000; border-radius: 0.8mm; flex: none; margin-top: 0.4mm; }
.box.sm { width: 3.8mm; height: 3.8mm; border-width: 1.2px; }
.row { display: flex; gap: 2.2mm; align-items: flex-start; min-height: 8mm; padding: 1.2mm 0 0; border-bottom: 1px dotted #777; }
.row.sub { margin-left: 8mm; min-height: 6.5mm; padding-top: 0.8mm; font-size: 10pt; }
.n { width: 6.5mm; font-size: 8pt; font-weight: 700; padding-top: 0.9mm; flex: none; text-align: right; }
.t { flex: 1; }
.flag { font-size: 8pt; border: 1px solid #000; padding: 0 1mm; margin-left: 1.5mm; white-space: nowrap; }
.tm { font-size: 8.5pt; font-weight: 700; margin-right: 1.5mm; }
.new { display: flex; gap: 2mm; align-items: flex-end; height: 8mm; border-bottom: 1px dotted #777; font-size: 8pt; color: #333; }
.new b { font-size: 8pt; }
.alerts { border: 2px solid #000; padding: 2mm 3mm; margin: 0 0 2mm; }
.alerts div { margin: 0.6mm 0; }
.alerts div::before { content: "! "; font-weight: 700; }
.grid { border-top: 1px solid #000; }
.hour { display: flex; height: 9.4mm; border-bottom: 1px solid #aaa; }
.hour .h { width: 11mm; font-size: 9pt; font-weight: 700; padding-top: 0.8mm; border-right: 1px solid #000; flex: none; }
.hour .c { flex: 1; padding: 0.8mm 0 0 2mm; font-size: 9.5pt; }
.hour .c i { font-style: normal; font-size: 8pt; font-weight: 700; margin-right: 1mm; }
.cols { display: flex; gap: 6mm; }
.cols > div { flex: 1; }
.hab { display: flex; gap: 2.2mm; align-items: flex-start; min-height: 8mm; padding-top: 1.2mm; border-bottom: 1px dotted #777; }
.hab small { color: #333; font-size: 8pt; margin-left: 1.5mm; }
.msg { display: flex; gap: 2.2mm; min-height: 11mm; padding: 1.4mm 0; border-bottom: 1px dotted #777; font-size: 9.5pt; }
.msg b { font-size: 9pt; }
.msg .src { font-size: 7.5pt; text-transform: uppercase; letter-spacing: 1px; color: #333; margin-left: 1mm; }
.ana p { margin: 1mm 0; font-size: 10pt; }
.ana p b { display: inline-block; width: 18mm; font-size: 8.5pt; text-transform: uppercase; letter-spacing: 1px; vertical-align: top; }
.ana p span { display: inline-block; width: calc(100% - 20mm); }
.news li { margin: 1mm 0; font-size: 9.5pt; }
.news ul { margin: 0; padding-left: 5mm; }
.lines div { height: 8.5mm; border-bottom: 1px solid #999; }
.legend { font-size: 8pt; border-top: 1px solid #000; margin-top: 3mm; padding-top: 1mm; color: #222; }
.dense .row { min-height: 6.4mm; padding-top: 0.6mm; }
.dense .row.sub { min-height: 5.6mm; }
.dense .new { height: 6mm; }
.dense h3 { margin: 2mm 0 0.5mm; }
.empty { font-size: 9pt; color: #444; font-style: italic; margin: 1mm 0; }
"#;

fn task_row(t: &SheetTask, sub: bool) -> String {
    let flag = t.flag.as_ref().map(|f| format!("<span class=\"flag\">{}</span>", esc(f))).unwrap_or_default();
    let time = t.time.as_ref().map(|x| format!("<span class=\"tm\">{}</span>", esc(x))).unwrap_or_default();
    let done = if t.done_sub > 0 { format!("<span class=\"flag\">{} sub done</span>", t.done_sub) } else { String::new() };
    format!(
        "<div class=\"row{}\"><span class=\"n\">{}</span><span class=\"box{}\"></span><span class=\"t\">{time}{}{flag}{done}</span></div>",
        if sub { " sub" } else { "" },
        esc(&t.n),
        if sub { " sm" } else { "" },
        esc(&t.title)
    )
}

/// The printable page set (2 to 3 A4 pages) as one HTML document.
pub fn render_html(s: &Sheet) -> String {
    let mut h = String::new();
    // A long list shrinks its rows so it still fits on two pages of tasks.
    let rows: usize = s.groups.iter().map(|g| g.tasks.iter().map(|t| 1 + t.subs.len()).sum::<usize>() + 1).sum();
    let dense = rows > 20;
    h.push_str(&format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>Morning sheet {}</title><style>{CSS}</style></head><body{}>", esc(&s.id), if dense { " class=\"dense\"" } else { "" }));
    let top = |page: u32| {
        format!(
            "<header class=\"top\"><h1>{}<small>{}</small></h1><div class=\"meta\">Morning sheet {} &middot; page {page}/3<br>Photograph it when marked and send it to Nora</div></header>",
            esc(&s.weekday),
            esc(&s.date),
            esc(&s.id)
        )
    };

    // Page 1: alerts, schedule, habits.
    h.push_str("<section class=\"page\">");
    h.push_str(&top(1));
    if !s.alerts.is_empty() {
        h.push_str("<div class=\"alerts\">");
        for a in &s.alerts {
            h.push_str(&format!("<div>{}</div>", esc(a)));
        }
        h.push_str("</div>");
    }
    h.push_str("<h2>Schedule</h2><div class=\"grid\">");
    for hour in 5..=22u32 {
        let items: Vec<String> = s
            .schedule
            .iter()
            .filter(|i| i.time.get(..2).and_then(|x| x.parse::<u32>().ok()).map(|x| x == hour || (hour == 5 && x < 5) || (hour == 22 && x > 22)).unwrap_or(false))
            .map(|i| format!("<i>{}</i>{}{}", esc(&i.time), esc(&i.label), i.n.as_ref().map(|n| format!(" ({})", esc(n))).unwrap_or_default()))
            .collect();
        h.push_str(&format!("<div class=\"hour\"><div class=\"h\">{hour:02}</div><div class=\"c\">{}</div></div>", items.join(" &nbsp; ")));
    }
    h.push_str("</div><h2>Habits</h2>");
    if s.habits.is_empty() {
        h.push_str("<p class=\"empty\">No habits are due today.</p>");
    }
    h.push_str("<div class=\"cols\"><div>");
    let half = s.habits.len().div_ceil(2);
    for (i, hb) in s.habits.iter().enumerate() {
        if i == half && half > 0 {
            h.push_str("</div><div>");
        }
        let floor = hb.floor.as_ref().map(|f| format!(" (floor: {})", esc(f))).unwrap_or_default();
        h.push_str(&format!(
            "<div class=\"hab\"><span class=\"n\">{}</span><span class=\"box\"></span><span>{}<small>{}{}</small></span></div>",
            esc(&hb.n),
            esc(&hb.name),
            esc(&hb.note),
            floor
        ));
    }
    h.push_str("</div></div><div class=\"legend\">How to mark: tick the box when done. X and a reason when skipped. Arrow and a day to move. Write new tasks on the dotted lines with their area. The numbers (3, 3.1, H2, M1) are how Nora finds each item.</div></section>");

    // Page 2: tasks by pillar and area, with room to add.
    h.push_str("<section class=\"page\">");
    h.push_str(&top(2));
    h.push_str("<h2>Tasks</h2>");
    if s.groups.is_empty() {
        h.push_str("<p class=\"empty\">Nothing is planned. Write today's tasks on the lines.</p>");
    }
    let mut last_pillar = String::new();
    for g in &s.groups {
        if g.pillar != last_pillar && !g.pillar.is_empty() {
            h.push_str(&format!("<div class=\"pillar\">{}</div>", esc(&g.pillar)));
            last_pillar = g.pillar.clone();
        }
        h.push_str(&format!("<h3>{}</h3>", esc(&g.area)));
        for t in &g.tasks {
            h.push_str(&task_row(t, false));
            for sub in &t.subs {
                h.push_str(&task_row(sub, true));
            }
        }
        h.push_str("<div class=\"new\"><b>+</b></div>");
    }
    h.push_str("<h2>New tasks (write the area first, then the task)</h2>");
    for _ in 0..4 {
        h.push_str("<div class=\"new\"><span class=\"box\"></span></div>");
    }
    h.push_str("</section>");

    // Page 3: messages, analysis, news, notes.
    h.push_str("<section class=\"page\">");
    h.push_str(&top(3));
    h.push_str("<h2>Messages that matter</h2>");
    if s.messages.is_empty() {
        h.push_str("<p class=\"empty\">Nothing needs you in the inbox.</p>");
    }
    for m in &s.messages {
        h.push_str(&format!(
            "<div class=\"msg\"><span class=\"n\">{}</span><span class=\"box\"></span><span><b>{}</b><span class=\"src\">{}</span><br>{}</span></div>",
            esc(&m.n),
            esc(&m.from),
            esc(&m.source),
            esc(&m.text)
        ));
    }
    h.push_str("<h2>Nora on yesterday</h2><div class=\"ana\">");
    match &s.analysis {
        Some(a) => {
            for (label, text) in [("Done", &a.done), ("Missed", &a.missed), ("Mood", &a.mood), ("Pattern", &a.pattern), ("Today", &a.today)] {
                if !text.trim().is_empty() {
                    h.push_str(&format!("<p><b>{label}</b><span>{}</span></p>", esc(text)));
                }
            }
        }
        None => h.push_str("<p class=\"empty\">No analysis this morning.</p>"),
    }
    h.push_str("</div>");
    if let Some(news) = s.news.as_ref().filter(|n| !n.trim().is_empty()) {
        h.push_str("<h2>News</h2><div class=\"news\"><ul>");
        for line in news.lines().map(|l| l.trim().trim_start_matches(['-', '*', '•', ' '])).filter(|l| !l.is_empty()) {
            h.push_str(&format!("<li>{}</li>", esc(line)));
        }
        h.push_str("</ul></div>");
    }
    h.push_str("<h2>Notes</h2><div class=\"lines\">");
    for _ in 0..7 {
        h.push_str("<div></div>");
    }
    h.push_str("</div></section>");
    h.push_str("</body></html>");
    h
}

// ---------------------------------------------------------------------------------------------
// PDF

/// Edge or Chrome, from `AOS_PDF_BROWSER` or the usual install folders.
pub fn find_browser() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AOS_PDF_BROWSER").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let pf = |v: &str| std::env::var_os(v).map(PathBuf::from);
    let mut c: Vec<PathBuf> = Vec::new();
    for root in [pf("ProgramFiles(x86)"), pf("ProgramFiles"), pf("LOCALAPPDATA")].into_iter().flatten() {
        c.push(root.join("Microsoft/Edge/Application/msedge.exe"));
        c.push(root.join("Google/Chrome/Application/chrome.exe"));
    }
    for p in ["/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"] {
        c.push(PathBuf::from(p));
    }
    c.into_iter().find(|p| p.is_file())
}

fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

/// Print `html` to an A4 PDF with the headless browser. Returns the page count when it can tell.
pub async fn html_to_pdf(html_path: &Path, pdf_path: &Path) -> Result<Option<usize>> {
    let bin = find_browser().context("no Edge or Chrome found (set AOS_PDF_BROWSER)")?;
    let profile = std::env::temp_dir().join(format!("aos-pdf-{}", aos_core::ids::new_id()));
    let _ = std::fs::remove_file(pdf_path);
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.args(["--headless=new", "--disable-gpu", "--no-pdf-header-footer", "--no-first-run", "--disable-extensions"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--print-to-pdf={}", pdf_path.display()))
        .arg(file_url(html_path))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().with_context(|| format!("starting {}", bin.display()))?;
    // The browser may hand off and exit before the file is written, so wait for the file to settle.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let mut last = 0u64;
    let mut stable = 0;
    let result = loop {
        if tokio::time::Instant::now() > deadline {
            break Err(anyhow::anyhow!("the browser did not write the PDF within 60 s"));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let len = std::fs::metadata(pdf_path).map(|m| m.len()).unwrap_or(0);
        if len > 0 && len == last {
            stable += 1;
            if stable >= 3 {
                break Ok(());
            }
        } else {
            stable = 0;
        }
        last = len;
    };
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    let _ = std::fs::remove_dir_all(&profile);
    result?;
    let bytes = std::fs::read(pdf_path)?;
    if !bytes.starts_with(b"%PDF") {
        bail!("the browser wrote something that is not a PDF");
    }
    Ok(count_pages(&bytes))
}

/// Count `/Type /Page` objects (not `/Pages`). Good enough for the page count of our own output.
pub fn count_pages(pdf: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(pdf);
    let n = text.match_indices("/Type /Page").filter(|(i, _)| !text[*i + 11..].starts_with('s')).count();
    (n > 0).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nora::testing::nora_with;
    use aos_core::tasks::NewTask;

    #[test]
    fn analysis_parses_labels_and_falls_back() {
        let a = parse_analysis("DONE: Gym and reading.\nMISSED: Work block (tired).\nMOOD: Calm, energy 3.\nPATTERN: Evenings slip after phone use.\nTODAY: Start with the Work block.");
        assert_eq!(a.done, "Gym and reading.");
        assert!(a.missed.contains("tired"));
        assert_eq!(a.today, "Start with the Work block.");
        let b = parse_analysis("A quiet day with little logged.");
        assert_eq!(b.done, "A quiet day with little logged.");
        let c = parse_analysis("**Done:** Gym\n**Pattern:** Late starts");
        assert_eq!((c.done.as_str(), c.pattern.as_str()), ("Gym", "Late starts"));
    }

    #[test]
    fn clip1_drops_invisible_padding() {
        assert_eq!(clip1("Hello \u{34F}\u{200B} \u{FEFF}world   again", 40), "Hello world again");
        assert!(clip1("abcdefghij", 5).starts_with("abcde"));
    }

    #[test]
    fn gathers_numbers_and_renders() {
        let (nora, _e, _p) = nora_with(&[], None);
        let store = nora.core();
        let day = aos_core::time::local_today();
        let add = |title: &str, area: Option<&str>, parent: Option<String>, sched: Option<String>, due: Option<NaiveDate>| {
            store.add_task("t", NewTask { title: title.into(), area_id: area.map(String::from), parent_id: parent, due, scheduled: sched, ..Default::default() }).unwrap()
        };
        let gym = add("Gym: legs", Some("fitness"), None, Some(format!("{}T06:30", day.format("%Y-%m-%d"))), Some(day));
        add("Warm up", Some("fitness"), Some(gym.id.clone()), None, Some(day));
        add("Read 20 pages", Some("reading"), None, None, Some(day));
        add("Old <thing>", None, None, None, Some(day - chrono::Duration::days(2)));
        let s = gather(&nora, day).unwrap();
        let ns: Vec<String> = s.groups.iter().flat_map(|g| g.tasks.iter().map(|t| t.n.clone())).collect();
        assert_eq!(ns, vec!["1", "2", "3"]);
        assert_eq!(s.groups[0].tasks[0].subs[0].n, "1.1");
        assert_eq!(s.schedule[0].time, "06:30");
        assert!(s.alerts.iter().any(|a| a.contains("late")));
        let html = render_html(&s);
        assert!(html.contains("Gym: legs") && html.contains("Old &lt;thing&gt;") && html.contains("page 3/3"));
        assert!(html.matches("class=\"box").count() >= 8);
    }

    #[tokio::test]
    async fn renders_a_real_pdf_when_a_browser_exists() {
        if find_browser().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("aos-sheet-{}", aos_core::ids::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = Sheet { id: "2026-01-02".into(), date: "2 January 2026".into(), weekday: "Friday".into(), ..Default::default() };
        let html = dir.join("s.html");
        std::fs::write(&html, render_html(&s)).unwrap();
        let pages = html_to_pdf(&html, &dir.join("s.pdf")).await.unwrap();
        assert_eq!(pages, Some(3));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
