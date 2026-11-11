//! The built-in tools. Each one is an input struct (its doc comments become the JSON Schema
//! descriptions agents read) plus a handler that calls the store.

use aos_core::goals::{GoalPatch, NewGoal, format_inr, parse_inr};
use aos_core::habits::NewHabit;
use aos_core::journal::NewJournal;
use aos_core::model::{AlarmKind, HabitLogStatus, TaskStatus};
use aos_core::mood::NewMood;
use aos_core::streak::Cadence;
use aos_core::tasks::{
    DueFilter, NewTask, StatusFilter, TaskFilter, TaskPatch, normalize_scheduled,
};
use aos_core::time::{parse_day, parse_when};
use chrono::{Duration, NaiveDate};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::registry::{Risk, Tool, ToolCtx, ToolError};

type R = Result<Value, ToolError>;

pub fn all() -> Vec<Tool> {
    vec![
        Tool::new("today", "Everything about one day: tasks by area with subtasks, the day's score, habits with streaks, mood, journal, alarms in the next 24h, XP.", Risk::Free, today).read_only().positional(&["date"]),
        Tool::new("task_add", "Add a task. Give `parent` to make it a subtask of a bigger task.", Risk::Free, task_add).positional(&["title"]),
        Tool::new("task_list", "List tasks with their subtasks and progress. Defaults to unfinished top-level tasks.", Risk::Free, task_list).read_only(),
        Tool::new("task_update", "Change a task's fields. Pass null to clear a field. Status here can be open, doing or dropped; use task_done / task_skip to close.", Risk::Free, task_update).positional(&["id"]),
        Tool::new("task_done", "Mark a task done, with evidence (a commit, link, file or sentence). Earns XP.", Risk::Free, task_done).positional(&["id", "evidence"]),
        Tool::new("task_skip", "Skip a task with a reason (tired, forgot, too big, avoided, phone, sick, other) and whether that reason is the real one. No blame, no XP loss.", Risk::Free, task_skip).positional(&["id", "reason"]),
        Tool::new("habit_add", "Add a habit with its cue (\"after I ___\"), normal version and floor version.", Risk::Free, habit_add).positional(&["name"]),
        Tool::new("habit_list", "Habits with today's state and streaks (never miss twice: one miss bends, two in a row end it).", Risk::Free, habit_list).read_only().positional(&["date"]),
        Tool::new("habit_log", "Log a habit for a day: done (default), floor (minimum version, still counts), missed or excused.", Risk::Free, habit_log).positional(&["habit", "status"]),
        Tool::new("mood_log", "Log mood (valence -1..1) and energy (1..5) with labels and reasons.", Risk::Free, mood_log).positional(&["valence"]),
        Tool::new("journal_add", "Add a journal entry. The text is stored exactly as given; never rewrite his words.", Risk::Free, journal_add).positional(&["text"]),
        Tool::new("journal_read", "Read journal entries for a day or the last few days, newest first.", Risk::Free, journal_read).read_only(),
        Tool::new("alarm_set", "Set an alarm or reminder. `at` takes 07:30, +15m, tomorrow 06:00 or an ISO time.", Risk::Tell, alarm_set).positional(&["at", "label"]),
        Tool::new("alarm_list", "Upcoming alarms and reminders.", Risk::Free, alarm_list).read_only(),
        Tool::new("alarm_cancel", "Cancel an alarm or reminder.", Risk::Tell, alarm_cancel).positional(&["id"]),
        Tool::new("xp_summary", "Overall level, XP per pillar and area, XP today and this week.", Risk::Free, xp_summary).read_only(),
        Tool::new("goal_list", "Goals per pillar: specs, why, images, price, earned so far, progress.", Risk::Free, goal_list).read_only(),
        Tool::new("goal_add", "Add a goal (a thing to earn or reach) under a pillar. Money in paise (Rs 1 = 100 paise) or rupee text.", Risk::Free, goal_add).positional(&["name"]),
        Tool::new("goal_update", "Change a goal's details, price, manual progress or status. Pass null to clear a field.", Risk::Free, goal_update).positional(&["goal"]),
        Tool::new("goal_earn", "Add money earned toward a goal (negative corrects a mistake). Give `rupees` (\"2500\", \"5,41,800\") or `amount_paise`.", Risk::Free, goal_earn).positional(&["goal", "rupees"]),
        Tool::new("profile", "Who the app is for (name, avatar) and the goal shown for each pillar in the hero, with price and earned so far.", Risk::Free, profile).read_only(),
        Tool::new("area_list", "The pillars and their areas (ids to use in `area` arguments).", Risk::Free, area_list).read_only(),
    ]
}

/// Lets a field tell "not given" (`None`) apart from "set to null" (`Some(None)`).
fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

fn day_or_today(ctx: &ToolCtx<'_>, s: &Option<String>) -> Result<NaiveDate, ToolError> {
    match s {
        Some(s) => Ok(parse_day(s, ctx.today())?),
        None => Ok(ctx.today()),
    }
}

fn area_id(ctx: &ToolCtx<'_>, key: &str) -> Result<String, ToolError> {
    Ok(ctx.store.resolve_area(key)?.id)
}

fn project_id(ctx: &ToolCtx<'_>, key: &str) -> Result<String, ToolError> {
    Ok(ctx.store.resolve_project(key)?.id)
}

fn task_id(ctx: &ToolCtx<'_>, key: &str) -> Result<String, ToolError> {
    Ok(ctx.store.resolve_task(key)?.id)
}

// ---------------------------------------------------------------------------------------------
// Today and XP

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TodayArgs {
    /// The day to show (default today): today, yesterday, -1d, 2026-11-11.
    pub date: Option<String>,
}

fn today(ctx: &ToolCtx<'_>, a: TodayArgs) -> R {
    let day = day_or_today(ctx, &a.date)?;
    Ok(json!(ctx.store.today_overview(day, ctx.now_utc())?))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

fn xp_summary(ctx: &ToolCtx<'_>, _: NoArgs) -> R {
    Ok(json!(ctx.store.xp_summary(ctx.today())?))
}

/// Profile from `[profile]` in the personal seed file, joined with the live goal rows.
/// Prices and earned amounts are rupees. Without a profile every field is empty and the
/// app uses its own fallback.
fn profile(ctx: &ToolCtx<'_>, _: NoArgs) -> R {
    let p = aos_core::seed::local_profile(&aos_core::paths::local_seed_path()).unwrap_or_default();
    let goals = ctx.store.list_goals(false)?;
    let hero: Vec<Value> = p
        .hero
        .iter()
        .map(|h| {
            let g = h.goal.as_deref().and_then(|id| goals.iter().find(|g| g.id == id));
            json!({
                "slot": h.slot,
                "goal_id": g.map(|g| g.id.clone()),
                "name": g.map(|g| g.name.clone()),
                "variant": h.variant,
                "price": g.and_then(|g| g.target_paise).map(|p| p / 100),
                "earned": g.map(|g| g.earned_paise / 100),
                "progress": g.map(|g| g.progress),
                "price_note": h.price_note,
                "specs": g.map(|g| g.specs.clone()).unwrap_or_default(),
                "why": g.and_then(|g| g.why.clone()),
                "about": h.about,
            })
        })
        .collect();
    Ok(json!({ "name": p.name, "avatar": p.avatar, "companion": p.companion, "private_habits": p.private_habits, "hero": hero }))
}

fn area_list(ctx: &ToolCtx<'_>, _: NoArgs) -> R {
    Ok(json!(ctx.store.area_tree()?))
}

// ---------------------------------------------------------------------------------------------
// Tasks

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskAddArgs {
    /// What to do.
    pub title: String,
    /// Area id or name, e.g. "fitness" (see area_list). Subtasks inherit the parent's area.
    pub area: Option<String>,
    /// Project id or name.
    pub project: Option<String>,
    /// Parent task id (or a unique prefix of it) to make this a subtask.
    pub parent: Option<String>,
    /// Due day: today, tomorrow, +3d, fri, 2026-11-20.
    pub due: Option<String>,
    /// When to do it: a day, optionally with a time ("today 15:00", "fri", "2026-11-20T09:00").
    pub scheduled: Option<String>,
    /// Difficulty or avoidance weight 0..10 (default 1). XP when done = 10 x weight.
    pub xp_weight: Option<i64>,
    pub notes: Option<String>,
}

fn task_add(ctx: &ToolCtx<'_>, a: TaskAddArgs) -> R {
    let new = NewTask {
        title: a.title,
        area_id: a.area.as_deref().map(|k| area_id(ctx, k)).transpose()?,
        project_id: a
            .project
            .as_deref()
            .map(|k| project_id(ctx, k))
            .transpose()?,
        parent_id: a.parent.as_deref().map(|k| task_id(ctx, k)).transpose()?,
        due: a
            .due
            .as_deref()
            .map(|d| parse_day(d, ctx.today()))
            .transpose()?,
        scheduled: a
            .scheduled
            .as_deref()
            .map(|s| normalize_scheduled(s, ctx.today()))
            .transpose()?,
        xp_weight: a.xp_weight,
        notes: a.notes,
        source: Some(ctx.actor.to_string()),
        external_id: None,
    };
    Ok(json!(ctx.store.add_task(ctx.actor, new)?))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskListArgs {
    /// active (default: open or doing), done, skipped, dropped, closed, all.
    pub status: Option<String>,
    /// today (due or scheduled today, plus overdue), overdue, undated, or a day for exactly that day.
    pub due: Option<String>,
    /// Area id or name.
    pub area: Option<String>,
    /// Project id or name.
    pub project: Option<String>,
    /// List the subtasks of this task instead.
    pub parent: Option<String>,
    /// Words to find in the title.
    pub search: Option<String>,
    /// At most this many top-level tasks (default 50, 0 = all).
    pub limit: Option<usize>,
}

fn task_list(ctx: &ToolCtx<'_>, a: TaskListArgs) -> R {
    let status = match a.status.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("active") | Some("open") => StatusFilter::Active,
        Some("done") => StatusFilter::Done,
        Some("skipped") => StatusFilter::Skipped,
        Some("dropped") => StatusFilter::Dropped,
        Some("closed") => StatusFilter::Closed,
        Some("all") => StatusFilter::All,
        Some(other) => return Err(ToolError::InvalidArgs(format!("unknown status '{other}'"))),
    };
    let today = ctx.today();
    let due = match a.due.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("any") => DueFilter::Any,
        Some("today") => DueFilter::OnOrBefore(today),
        Some("overdue") => DueFilter::Overdue(today),
        Some("undated") | Some("none") => DueFilter::Undated,
        Some(day) => DueFilter::On(parse_day(day, today)?),
    };
    let filter = TaskFilter {
        status,
        due,
        area_id: a.area.as_deref().map(|k| area_id(ctx, k)).transpose()?,
        project_id: a
            .project
            .as_deref()
            .map(|k| project_id(ctx, k))
            .transpose()?,
        parent_id: a.parent.as_deref().map(|k| task_id(ctx, k)).transpose()?,
        search: a.search,
        limit: a.limit.unwrap_or(50),
    };
    let tasks = ctx.store.list_tasks(&filter)?;
    Ok(json!({ "count": tasks.len(), "tasks": tasks }))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskUpdateArgs {
    /// Task id or a unique prefix.
    pub id: String,
    pub title: Option<String>,
    /// open, doing or dropped.
    pub status: Option<String>,
    /// Area id or name; null clears it.
    #[serde(default, deserialize_with = "nullable")]
    pub area: Option<Option<String>>,
    /// Project id or name; null clears it.
    #[serde(default, deserialize_with = "nullable")]
    pub project: Option<Option<String>>,
    /// Parent task id; null makes it top-level.
    #[serde(default, deserialize_with = "nullable")]
    pub parent: Option<Option<String>>,
    /// Due day; null clears it.
    #[serde(default, deserialize_with = "nullable")]
    pub due: Option<Option<String>>,
    /// Scheduled day/time; null clears it.
    #[serde(default, deserialize_with = "nullable")]
    pub scheduled: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    pub notes: Option<Option<String>>,
    /// 0..10
    pub xp_weight: Option<i64>,
}

fn task_update(ctx: &ToolCtx<'_>, a: TaskUpdateArgs) -> R {
    let today = ctx.today();
    let id = task_id(ctx, &a.id)?;
    let status = match a.status.as_deref() {
        Some(s) => Some(s.parse::<TaskStatus>()?),
        None => None,
    };
    // Resolve each `Option<Option<key>>` without losing the "clear" case.
    let patch = TaskPatch {
        title: a.title,
        notes: a.notes,
        status,
        xp_weight: a.xp_weight,
        area_id: a
            .area
            .map(|v| v.map(|k| area_id(ctx, &k)).transpose())
            .transpose()?,
        project_id: a
            .project
            .map(|v| v.map(|k| project_id(ctx, &k)).transpose())
            .transpose()?,
        parent_id: a
            .parent
            .map(|v| v.map(|k| task_id(ctx, &k)).transpose())
            .transpose()?,
        due: a
            .due
            .map(|v| v.map(|d| parse_day(&d, today)).transpose())
            .transpose()?,
        scheduled: a
            .scheduled
            .map(|v| v.map(|s| normalize_scheduled(&s, today)).transpose())
            .transpose()?,
    };
    Ok(json!(ctx.store.update_task(ctx.actor, &id, patch, today)?))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskDoneArgs {
    /// Task id or a unique prefix.
    pub id: String,
    /// Proof: a commit, link, file path or one sentence.
    pub evidence: Option<String>,
}

fn task_done(ctx: &ToolCtx<'_>, a: TaskDoneArgs) -> R {
    let id = task_id(ctx, &a.id)?;
    Ok(json!(ctx.store.complete_task(
        ctx.actor,
        &id,
        a.evidence,
        ctx.today()
    )?))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskSkipArgs {
    /// Task id or a unique prefix.
    pub id: String,
    /// Why: tired, forgot, too big, avoided, phone, sick, other (free text is fine).
    pub reason: String,
    /// Is this the real reason?
    pub genuine: bool,
}

fn task_skip(ctx: &ToolCtx<'_>, a: TaskSkipArgs) -> R {
    let id = task_id(ctx, &a.id)?;
    Ok(json!(ctx.store.skip_task(
        ctx.actor,
        &id,
        &a.reason,
        a.genuine,
        ctx.today()
    )?))
}

// ---------------------------------------------------------------------------------------------
// Habits

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HabitAddArgs {
    pub name: String,
    /// Area id or name.
    pub area: Option<String>,
    /// The anchor cue: "after I ___".
    pub cue: Option<String>,
    /// The normal version.
    pub normal: Option<String>,
    /// The smallest version that still counts.
    pub floor: Option<String>,
    /// daily (default), weekdays, or days like mon,wed,fri.
    pub cadence: Option<String>,
    /// XP when done (default 5; the floor version earns half).
    pub xp: Option<i64>,
    /// The one streak that matters most (journaling).
    pub anchor: Option<bool>,
}

fn habit_add(ctx: &ToolCtx<'_>, a: HabitAddArgs) -> R {
    let cadence: Cadence = a.cadence.as_deref().unwrap_or("daily").parse()?;
    let habit = ctx.store.add_habit(
        ctx.actor,
        NewHabit {
            id: None,
            name: a.name,
            area_id: a.area.as_deref().map(|k| area_id(ctx, k)).transpose()?,
            cue: a.cue,
            normal: a.normal,
            floor: a.floor,
            cadence,
            xp: a.xp.unwrap_or(5).clamp(0, 100),
            anchor: a.anchor.unwrap_or(false),
            start_date: ctx.today(),
        },
    )?;
    Ok(json!(habit))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HabitListArgs {
    /// Day to show state for (default today).
    pub date: Option<String>,
}

fn habit_list(ctx: &ToolCtx<'_>, a: HabitListArgs) -> R {
    let day = day_or_today(ctx, &a.date)?;
    let habits = ctx.store.list_habits(day)?;
    Ok(json!({ "date": day, "habits": habits }))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HabitLogArgs {
    /// Habit id, id prefix or name.
    pub habit: String,
    /// done (default), floor, missed, excused.
    pub status: Option<String>,
    /// Day (default today). Past days only.
    pub date: Option<String>,
    pub note: Option<String>,
}

fn habit_log(ctx: &ToolCtx<'_>, a: HabitLogArgs) -> R {
    let habit = ctx.store.resolve_habit(&a.habit)?;
    let status: HabitLogStatus = a.status.as_deref().unwrap_or("done").parse()?;
    let day = day_or_today(ctx, &a.date)?;
    Ok(json!(ctx.store.log_habit(
        ctx.actor,
        &habit.id,
        day,
        status,
        a.note,
        ctx.today()
    )?))
}

// ---------------------------------------------------------------------------------------------
// Mood and journal

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoodLogArgs {
    /// How pleasant: -1.0 (very unpleasant) .. 1.0 (very pleasant).
    pub valence: f64,
    /// 1 (drained) .. 5 (full of energy).
    pub energy: Option<i64>,
    /// Feelings, e.g. calm, anxious, proud.
    #[serde(default)]
    pub labels: Vec<String>,
    /// What's behind it, e.g. sleep, work, family.
    #[serde(default)]
    pub reasons: Vec<String>,
    pub note: Option<String>,
}

fn mood_log(ctx: &ToolCtx<'_>, a: MoodLogArgs) -> R {
    let mood = ctx.store.log_mood(
        ctx.actor,
        NewMood {
            valence: a.valence,
            energy: a.energy,
            labels: a.labels,
            reasons: a.reasons,
            note: a.note,
            healthkit_id: None,
            at: Some(ctx.now_utc()),
        },
    )?;
    Ok(json!(mood))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalAddArgs {
    /// His words, exactly. Stored verbatim.
    pub text: String,
    /// journal (default), win, challenge, improve...
    pub kind: Option<String>,
    /// Day it belongs to (default today).
    pub date: Option<String>,
}

fn journal_add(ctx: &ToolCtx<'_>, a: JournalAddArgs) -> R {
    let date = a
        .date
        .as_deref()
        .map(|d| parse_day(d, ctx.today()))
        .transpose()?;
    let added = ctx.store.add_journal(
        ctx.actor,
        NewJournal {
            text: a.text,
            kind: a.kind,
            date,
            source: Some(ctx.actor.to_string()),
        },
    )?;
    Ok(json!(added))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalReadArgs {
    /// Last day to read (default today).
    pub date: Option<String>,
    /// How many days back to include, counting `date` (default 1).
    pub days: Option<i64>,
    /// At most this many entries (default 50, 0 = all).
    pub limit: Option<usize>,
}

fn journal_read(ctx: &ToolCtx<'_>, a: JournalReadArgs) -> R {
    let to = day_or_today(ctx, &a.date)?;
    let days = a.days.unwrap_or(1).clamp(1, 3660);
    let from = to - Duration::days(days - 1);
    let entries = ctx.store.read_journal(from, to, a.limit.unwrap_or(50))?;
    Ok(json!({ "from": from, "to": to, "entries": entries }))
}

// ---------------------------------------------------------------------------------------------
// Goals

fn pillar_id(ctx: &ToolCtx<'_>, key: &str) -> Result<String, ToolError> {
    ctx.store
        .list_pillars()?
        .into_iter()
        .find(|p| p.id == key.trim() || p.name.eq_ignore_ascii_case(key.trim()))
        .map(|p| p.id)
        .ok_or_else(|| ToolError::InvalidArgs(format!("no pillar '{key}' (see area_list)")))
}

/// Paise from either an integer or rupee text; at most one of them.
fn money(paise: Option<i64>, rupees: Option<&str>) -> Result<Option<i64>, ToolError> {
    match (paise, rupees) {
        (Some(_), Some(_)) => Err(ToolError::InvalidArgs(
            "give paise or rupees, not both".into(),
        )),
        (Some(p), None) => Ok(Some(p)),
        (None, Some(r)) => Ok(Some(parse_inr(r)?)),
        (None, None) => Ok(None),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalListArgs {
    /// Include reached and dropped goals.
    pub all: Option<bool>,
}

fn goal_list(ctx: &ToolCtx<'_>, a: GoalListArgs) -> R {
    let goals = ctx.store.list_goals(a.all.unwrap_or(false))?;
    let shown: Vec<Value> = goals
        .into_iter()
        .map(|g| {
            let mut v = json!(g);
            v["target"] = json!(g.target_paise.map(format_inr));
            v["earned"] = json!(format_inr(g.earned_paise));
            v
        })
        .collect();
    Ok(json!({ "goals": shown }))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalAddArgs {
    pub name: String,
    /// Pillar id or name (see area_list).
    pub pillar: Option<String>,
    /// Area id or name.
    pub area: Option<String>,
    /// Specs or features, one per item.
    #[serde(default)]
    pub specs: Vec<String>,
    /// Why it matters, in his words.
    pub why: Option<String>,
    /// Image paths, URLs or 3D model refs.
    #[serde(default)]
    pub images: Vec<String>,
    /// Price in paise (Rs 1 = 100 paise).
    pub target_paise: Option<i64>,
    /// Price as rupee text instead: "541800" or "5,41,800".
    pub target_rupees: Option<String>,
    /// Manual progress 0..1 for goals without a price.
    pub progress: Option<f64>,
}

fn goal_add(ctx: &ToolCtx<'_>, a: GoalAddArgs) -> R {
    let goal = ctx.store.add_goal(
        ctx.actor,
        NewGoal {
            id: None,
            pillar_id: a.pillar.as_deref().map(|k| pillar_id(ctx, k)).transpose()?,
            area_id: a.area.as_deref().map(|k| area_id(ctx, k)).transpose()?,
            name: a.name,
            specs: a.specs,
            why: a.why,
            images: a.images,
            target_paise: money(a.target_paise, a.target_rupees.as_deref())?,
            earned_paise: 0,
            progress: a.progress,
            sort: 0,
        },
    )?;
    Ok(json!(goal))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalUpdateArgs {
    /// Goal id, id prefix or name.
    pub goal: String,
    pub name: Option<String>,
    /// Pillar id or name; null clears it.
    #[serde(default, deserialize_with = "nullable")]
    pub pillar: Option<Option<String>>,
    /// Area id or name; null clears it.
    #[serde(default, deserialize_with = "nullable")]
    pub area: Option<Option<String>>,
    /// Replaces the whole specs list.
    pub specs: Option<Vec<String>>,
    #[serde(default, deserialize_with = "nullable")]
    pub why: Option<Option<String>>,
    /// Replaces the whole images list.
    pub images: Option<Vec<String>>,
    /// Price in paise; null removes the price.
    #[serde(default, deserialize_with = "nullable")]
    pub target_paise: Option<Option<i64>>,
    /// Price as rupee text instead.
    pub target_rupees: Option<String>,
    /// Manual progress 0..1 (goals without a price).
    #[serde(default, deserialize_with = "nullable")]
    pub progress: Option<Option<f64>>,
    /// active, reached or dropped.
    pub status: Option<String>,
}

fn goal_update(ctx: &ToolCtx<'_>, a: GoalUpdateArgs) -> R {
    let id = ctx.store.resolve_goal(&a.goal)?.id;
    let target = match (a.target_paise, a.target_rupees.as_deref()) {
        (Some(_), Some(_)) => {
            return Err(ToolError::InvalidArgs(
                "give target_paise or target_rupees, not both".into(),
            ));
        }
        (Some(p), None) => Some(p),
        (None, Some(r)) => Some(Some(parse_inr(r)?)),
        (None, None) => None,
    };
    let patch = GoalPatch {
        name: a.name,
        pillar_id: a
            .pillar
            .map(|v| v.map(|k| pillar_id(ctx, &k)).transpose())
            .transpose()?,
        area_id: a
            .area
            .map(|v| v.map(|k| area_id(ctx, &k)).transpose())
            .transpose()?,
        specs: a.specs,
        why: a.why,
        images: a.images,
        target_paise: target,
        progress: a.progress,
        status: a.status,
    };
    Ok(json!(ctx.store.update_goal(ctx.actor, &id, patch)?))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalEarnArgs {
    /// Goal id, id prefix or name.
    pub goal: String,
    /// Amount as rupee text: "2500", "5,41,800", "-200".
    pub rupees: Option<String>,
    /// Amount in paise instead.
    pub amount_paise: Option<i64>,
}

fn goal_earn(ctx: &ToolCtx<'_>, a: GoalEarnArgs) -> R {
    let id = ctx.store.resolve_goal(&a.goal)?.id;
    let amount = money(a.amount_paise, a.rupees.as_deref())?
        .ok_or_else(|| ToolError::InvalidArgs("give rupees or amount_paise".into()))?;
    let g = ctx.store.earn_toward_goal(ctx.actor, &id, amount)?;
    let mut v = json!(g);
    v["earned"] = json!(format_inr(g.earned_paise));
    v["remaining"] = json!(g.remaining_paise.map(format_inr));
    Ok(v)
}

// ---------------------------------------------------------------------------------------------
// Alarms

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlarmSetArgs {
    /// 07:30 (next time it comes), +15m, +2h, tomorrow 06:00, 2026-11-20 09:00, or ISO 8601.
    pub at: String,
    /// What it's for.
    pub label: String,
    /// alarm (default, rings) or reminder (a notification).
    pub kind: Option<String>,
    /// Task id this alarm is for.
    pub task: Option<String>,
}

fn alarm_set(ctx: &ToolCtx<'_>, a: AlarmSetArgs) -> R {
    let at = parse_when(&a.at, &ctx.now)?;
    if at < ctx.now_utc() {
        return Err(ToolError::InvalidArgs(format!("'{}' is in the past", a.at)));
    }
    let kind: AlarmKind = a.kind.as_deref().unwrap_or("alarm").parse()?;
    let task = a.task.as_deref().map(|k| task_id(ctx, k)).transpose()?;
    Ok(json!(
        ctx.store.set_alarm(ctx.actor, at, &a.label, kind, task)?
    ))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlarmListArgs {
    /// Include fired, cancelled and past alarms.
    pub all: Option<bool>,
}

fn alarm_list(ctx: &ToolCtx<'_>, a: AlarmListArgs) -> R {
    let alarms = ctx
        .store
        .list_alarms(ctx.now_utc(), a.all.unwrap_or(false))?;
    Ok(json!({ "alarms": alarms }))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlarmCancelArgs {
    /// Alarm id or a unique prefix.
    pub id: String,
}

fn alarm_cancel(ctx: &ToolCtx<'_>, a: AlarmCancelArgs) -> R {
    Ok(json!(ctx.store.cancel_alarm(ctx.actor, &a.id)?))
}

#[cfg(test)]
#[allow(clippy::inconsistent_digit_grouping)] // Indian grouping (lakh, crore) on purpose
mod tests {
    use super::*;
    use aos_core::Store;

    fn call(store: &Store, name: &str, args: Value) -> Value {
        let reg = crate::registry();
        let ctx = ToolCtx::new(store, "test");
        reg.call(&ctx, name, args)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn every_schema_is_an_object() {
        for t in crate::registry().tools() {
            assert_eq!(t.input_schema["type"], "object", "{}", t.name);
            assert!(
                !t.input_schema.to_string().contains("$ref"),
                "{} must be inlined",
                t.name
            );
            for p in t.positional {
                assert!(
                    t.property_names().iter().any(|n| n == p),
                    "{}: positional {p}",
                    t.name
                );
            }
        }
    }

    #[test]
    fn task_flow_through_tools() {
        let store = Store::open_temporary().unwrap();
        let parent = call(
            &store,
            "task_add",
            json!({"title": "Ship v1", "area": "Work", "due": "today", "xp_weight": 2}),
        );
        let pid = parent["id"].as_str().unwrap();
        let child = call(
            &store,
            "task.add",
            json!({"title": "Write tests", "parent": &pid[..6]}),
        );
        assert_eq!(child["area_id"], "work");

        let done = call(
            &store,
            "task_done",
            json!({"id": child["id"], "evidence": "commit 1"}),
        );
        assert_eq!(done["xp_delta"], 10);
        let list = call(&store, "task_list", json!({"due": "today"}));
        assert_eq!(list["count"], 1);
        assert_eq!(list["tasks"][0]["rollup"]["done"], 1);

        let upd = call(
            &store,
            "task_update",
            json!({"id": pid, "due": null, "title": "Ship v1.0"}),
        );
        assert!(upd["task"]["due"].is_null());
        assert_eq!(upd["task"]["title"], "Ship v1.0");

        let skip = call(
            &store,
            "task_skip",
            json!({"id": pid, "reason": "too big", "genuine": true}),
        );
        assert_eq!(skip["task"]["status"], "skipped");
    }

    #[test]
    fn bad_args_are_caller_errors() {
        let store = Store::open_temporary().unwrap();
        let reg = crate::registry();
        let ctx = ToolCtx::new(&store, "test");
        let e = reg
            .call(&ctx, "task_add", json!({"titel": "typo"}))
            .unwrap_err();
        assert_eq!(e.code(), "invalid_args");
        let e = reg
            .call(&ctx, "task_done", json!({"id": "nope"}))
            .unwrap_err();
        assert_eq!(e.code(), "not_found");
        let e = reg.call(&ctx, "nope", json!({})).unwrap_err();
        assert_eq!(e.code(), "unknown_tool");
    }

    #[test]
    fn habits_mood_journal_alarms_today() {
        let store = Store::open_temporary().unwrap();
        call(
            &store,
            "habit_add",
            json!({"name": "Journal", "area": "mindset", "anchor": true, "floor": "one line"}),
        );
        let log = call(
            &store,
            "habit_log",
            json!({"habit": "journal", "status": "floor"}),
        );
        assert_eq!(log["streak"]["current"], 1);
        call(
            &store,
            "mood_log",
            json!({"valence": 0.5, "energy": 4, "labels": ["calm"]}),
        );
        let j = call(&store, "journal_add", json!({"text": "  raw words \n"}));
        assert_eq!(j["entry"]["text"], "  raw words \n");
        let read = call(&store, "journal_read", json!({}));
        assert_eq!(read["entries"].as_array().unwrap().len(), 1);
        let alarm = call(
            &store,
            "alarm_set",
            json!({"at": "+30m", "label": "Stretch", "kind": "reminder"}),
        );
        assert_eq!(
            call(&store, "alarm_list", json!({}))["alarms"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        call(&store, "alarm_cancel", json!({"id": alarm["id"]}));

        let today = call(&store, "today", Value::Null);
        assert_eq!(today["habits"][0]["today"], "floor");
        assert_eq!(today["journal_written"], true);
        assert_eq!(today["moods"].as_array().unwrap().len(), 1);
        let xp = call(&store, "xp_summary", json!({}));
        assert_eq!(xp["overall"]["total_xp"], 3 + 10);
        assert_eq!(
            call(&store, "area_list", json!({}))["pillars"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert!(today["xp"]["level"]["stage"].is_number());
    }

    #[test]
    fn goals_through_tools() {
        let store = Store::open_temporary().unwrap();
        let g = call(
            &store,
            "goal_add",
            json!({"name": "Laptop", "pillar": "Wealth", "target_rupees": "1,00,000", "specs": ["16 inch"]}),
        );
        assert_eq!(g["target_paise"], 100_000_00);
        let e = call(
            &store,
            "goal_earn",
            json!({"goal": "laptop", "rupees": "25,000"}),
        );
        assert_eq!(e["earned"], "\u{20b9}25,000");
        assert_eq!(e["remaining"], "\u{20b9}75,000");
        let u = call(
            &store,
            "goal_update",
            json!({"goal": g["id"], "why": "build things"}),
        );
        assert_eq!(u["why"], "build things");
        let list = call(&store, "goal_list", json!({}));
        assert_eq!(list["goals"][0]["target"], "\u{20b9}1,00,000");
        assert_eq!(
            call(&store, "today", json!({}))["goals"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
