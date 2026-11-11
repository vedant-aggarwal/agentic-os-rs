//! Tasks with subtasks.
//!
//! - A task can have a parent (`parent_id`); a parent's progress is the [`rollup`] of its
//!   direct subtasks. Parents are not closed automatically.
//! - Done tasks earn `TASK_XP_PER_WEIGHT * xp_weight` XP in the task's area (or the parent's).
//!   Reopening takes it back with a negative ledger row.
//! - A skip always carries a reason and whether that reason was the genuine one.

use chrono::NaiveDate;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid, not_found};
use crate::model::{Rollup, Task, TaskNode, TaskStatus};
use crate::store::{ChangeOp, Store, Tx, resolve_prefix};
use crate::xp::{self, TASK_XP_PER_WEIGHT};
use crate::{ids, time};

/// Input for [`Store::add_task`]. Ids must already be resolved.
#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub title: String,
    pub area_id: Option<String>,
    pub project_id: Option<String>,
    pub parent_id: Option<String>,
    pub due: Option<NaiveDate>,
    pub scheduled: Option<String>,
    pub xp_weight: Option<i64>,
    pub notes: Option<String>,
    pub source: Option<String>,
    pub external_id: Option<String>,
}

/// Changes for [`Store::update_task`]. `None` leaves a field alone; `Some(None)` clears it.
#[derive(Debug, Clone, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub notes: Option<Option<String>>,
    pub area_id: Option<Option<String>>,
    pub project_id: Option<Option<String>>,
    pub parent_id: Option<Option<String>>,
    pub due: Option<Option<NaiveDate>>,
    pub scheduled: Option<Option<String>>,
    pub xp_weight: Option<i64>,
    pub status: Option<TaskStatus>,
}

/// A task after a change, with the XP that change earned (negative when taken back).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskChange {
    pub task: Task,
    pub xp_delta: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusFilter {
    /// open or doing
    #[default]
    Active,
    Done,
    Skipped,
    Dropped,
    /// done, skipped or dropped
    Closed,
    All,
}

impl StatusFilter {
    fn statuses(self) -> &'static [&'static str] {
        match self {
            StatusFilter::Active => &["open", "doing"],
            StatusFilter::Done => &["done"],
            StatusFilter::Skipped => &["skipped"],
            StatusFilter::Dropped => &["dropped"],
            StatusFilter::Closed => &["done", "skipped", "dropped"],
            StatusFilter::All => &["open", "doing", "done", "skipped", "dropped"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DueFilter {
    #[default]
    Any,
    /// Due or scheduled on or before the day (today plus overdue).
    OnOrBefore(NaiveDate),
    /// Due or scheduled exactly on the day.
    On(NaiveDate),
    /// Due strictly before the day.
    Overdue(NaiveDate),
    /// No due date and not scheduled.
    Undated,
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub status: StatusFilter,
    pub due: DueFilter,
    pub area_id: Option<String>,
    pub project_id: Option<String>,
    /// List the subtasks of this task instead of top-level tasks.
    pub parent_id: Option<String>,
    /// Case-insensitive match on the title.
    pub search: Option<String>,
    /// 0 means no limit.
    pub limit: usize,
}

/// Progress of a parent from its direct subtasks' statuses. Dropped subtasks don't count.
pub fn rollup(children: &[TaskStatus]) -> Rollup {
    let mut r = Rollup {
        total: children.len() as u32,
        ..Rollup::default()
    };
    for s in children {
        match s {
            TaskStatus::Done => r.done += 1,
            TaskStatus::Skipped => r.skipped += 1,
            TaskStatus::Dropped => r.dropped += 1,
            TaskStatus::Open | TaskStatus::Doing => r.open += 1,
        }
    }
    let counted = r.total - r.dropped;
    r.progress = if counted == 0 {
        0.0
    } else {
        f64::from(r.done) / f64::from(counted)
    };
    r
}

/// How a day went: done / (done + skipped + open). A task with subtasks counts through its
/// subtasks, so a mega task with 10 parts weighs 10.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct DayScore {
    pub done: u32,
    pub skipped: u32,
    pub open: u32,
    pub counted: u32,
    /// 0.0..1.0
    pub rate: f64,
}

pub fn day_score(nodes: &[TaskNode]) -> DayScore {
    let mut s = DayScore::default();
    let mut add = |status: TaskStatus| match status {
        TaskStatus::Done => s.done += 1,
        TaskStatus::Skipped => s.skipped += 1,
        TaskStatus::Open | TaskStatus::Doing => s.open += 1,
        TaskStatus::Dropped => {}
    };
    for n in nodes {
        if n.subtasks.is_empty() {
            add(n.task.status);
        } else {
            n.subtasks.iter().for_each(|t| add(t.status));
        }
    }
    s.counted = s.done + s.skipped + s.open;
    s.rate = if s.counted == 0 {
        0.0
    } else {
        f64::from(s.done) / f64::from(s.counted)
    };
    s
}

/// Normalise a scheduled value: a day form (`today`, `fri`, `2026-11-20`) optionally followed
/// by a time (`15:00`). Returns `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`.
pub fn normalize_scheduled(input: &str, today: NaiveDate) -> Result<String> {
    let s = input.trim();
    let (day, clock) = match s.split_once([' ', 'T']) {
        Some((d, t)) => (d, Some(t.trim())),
        None if s.contains(':') => ("today", Some(s)),
        None => (s, None),
    };
    let date = time::parse_day(day, today)?;
    match clock {
        Some(t) => {
            let t = chrono::NaiveTime::parse_from_str(t, "%H:%M")
                .map_err(|_| invalid(format!("bad time in '{input}' (use HH:MM)")))?;
            Ok(format!("{}T{}", date.format("%Y-%m-%d"), t.format("%H:%M")))
        }
        None => Ok(date.format("%Y-%m-%d").to_string()),
    }
}

pub(crate) const TASK_COLS: &str = "id, parent_id, area_id, project_id, title, notes, status, due, \
    scheduled, xp_weight, skip_reason, reason_genuine, evidence, sort, source, external_id, \
    created_at, updated_at, closed_at";

pub(crate) fn task_from_row(r: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get("id")?,
        parent_id: r.get("parent_id")?,
        area_id: r.get("area_id")?,
        project_id: r.get("project_id")?,
        title: r.get("title")?,
        notes: r.get("notes")?,
        status: r.get("status")?,
        due: r.get("due")?,
        scheduled: r.get("scheduled")?,
        xp_weight: r.get("xp_weight")?,
        skip_reason: r.get("skip_reason")?,
        reason_genuine: r.get("reason_genuine")?,
        evidence: r.get("evidence")?,
        sort: r.get("sort")?,
        source: r.get("source")?,
        external_id: r.get("external_id")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        closed_at: r.get("closed_at")?,
    })
}

pub(crate) fn get_task(c: &Connection, id: &str) -> Result<Task> {
    c.query_row(
        &format!("SELECT {TASK_COLS} FROM task WHERE id = ?1"),
        [id],
        task_from_row,
    )
    .optional()?
    .ok_or_else(|| not_found("task", id))
}

fn subtasks(c: &Connection, parent: &str) -> Result<Vec<Task>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {TASK_COLS} FROM task WHERE parent_id = ?1 ORDER BY sort, created_at"
    ))?;
    let rows = stmt.query_map([parent], task_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn node(c: &Connection, task: Task) -> Result<TaskNode> {
    let subtasks = subtasks(c, &task.id)?;
    let rollup = (!subtasks.is_empty())
        .then(|| rollup(&subtasks.iter().map(|t| t.status).collect::<Vec<_>>()));
    Ok(TaskNode {
        task,
        subtasks,
        rollup,
    })
}

/// The task's area, or its parent's when it has none.
fn xp_area(c: &Connection, task: &Task) -> Result<Option<String>> {
    if task.area_id.is_some() {
        return Ok(task.area_id.clone());
    }
    match &task.parent_id {
        Some(p) => Ok(c
            .query_row("SELECT area_id FROM task WHERE id = ?1", [p], |r| r.get(0))
            .optional()?
            .flatten()),
        None => Ok(None),
    }
}

fn sync_task_xp(tx: &Tx<'_>, task: &Task, today: NaiveDate) -> Result<i64> {
    let target = if task.status == TaskStatus::Done {
        TASK_XP_PER_WEIGHT * task.xp_weight
    } else {
        0
    };
    let area = xp_area(tx, task)?;
    xp::set_award(tx, "task", &task.id, area.as_deref(), target, today)
}

/// Refuse a parent that would make a loop (a task under itself or its own subtask).
fn check_parent(c: &Connection, task_id: Option<&str>, parent_id: &str) -> Result<()> {
    let mut cursor = Some(parent_id.to_string());
    while let Some(id) = cursor {
        if Some(id.as_str()) == task_id {
            return Err(invalid(
                "a task can't be its own parent or sit under its own subtask",
            ));
        }
        cursor = c
            .query_row("SELECT parent_id FROM task WHERE id = ?1", [&id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| not_found("task", &id))?;
    }
    Ok(())
}

fn check_weight(w: i64) -> Result<i64> {
    if (0..=10).contains(&w) {
        Ok(w)
    } else {
        Err(invalid("xp_weight must be 0..10"))
    }
}

impl Store {
    pub fn add_task(&self, by: &str, new: NewTask) -> Result<Task> {
        let title = new.title.trim().to_string();
        if title.is_empty() {
            return Err(invalid("title is empty"));
        }
        let weight = check_weight(new.xp_weight.unwrap_or(1))?;
        self.write(by, |tx| {
            let mut area_id = new.area_id.clone();
            if let Some(parent) = &new.parent_id {
                check_parent(tx, None, parent)?;
                if area_id.is_none() {
                    area_id = get_task(tx, parent)?.area_id;
                }
            }
            let id = ids::new_id();
            // New tasks go to the end of their list.
            let sort: f64 = tx.query_row(
                "SELECT COALESCE(MAX(sort), 0) + 1 FROM task WHERE parent_id IS ?1",
                [&new.parent_id],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO task (id, parent_id, area_id, project_id, title, notes, status, due,
                     scheduled, xp_weight, sort, source, external_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
                params![
                    id,
                    new.parent_id,
                    area_id,
                    new.project_id,
                    title,
                    new.notes,
                    new.due,
                    new.scheduled,
                    weight,
                    sort,
                    new.source.as_deref().unwrap_or("app"),
                    new.external_id,
                    tx.now()
                ],
            )?;
            tx.record("task", &id, ChangeOp::Upsert)?;
            get_task(tx, &id)
        })
    }

    /// Exact id.
    pub fn get_task(&self, id: &str) -> Result<Task> {
        self.read(|c| get_task(c, id))
    }

    /// A task with its subtasks and rollup.
    pub fn get_task_node(&self, id: &str) -> Result<TaskNode> {
        self.read(|c| node(c, get_task(c, id)?))
    }

    /// Exact id or a unique prefix of at least 4 characters.
    pub fn resolve_task(&self, key: &str) -> Result<Task> {
        self.read(|c| {
            let id = resolve_prefix(c, "task", key)?;
            get_task(c, &id)
        })
    }

    pub fn update_task(
        &self,
        by: &str,
        id: &str,
        patch: TaskPatch,
        today: NaiveDate,
    ) -> Result<TaskChange> {
        self.write(by, |tx| {
            let mut t = get_task(tx, id)?;
            if let Some(title) = patch.title {
                let title = title.trim().to_string();
                if title.is_empty() {
                    return Err(invalid("title is empty"));
                }
                t.title = title;
            }
            if let Some(v) = patch.notes {
                t.notes = v;
            }
            if let Some(v) = patch.area_id {
                t.area_id = v;
            }
            if let Some(v) = patch.project_id {
                t.project_id = v;
            }
            if let Some(v) = patch.due {
                t.due = v;
            }
            if let Some(v) = patch.scheduled {
                t.scheduled = v;
            }
            if let Some(w) = patch.xp_weight {
                t.xp_weight = check_weight(w)?;
            }
            if let Some(parent) = patch.parent_id {
                if let Some(p) = &parent {
                    check_parent(tx, Some(&t.id), p)?;
                }
                t.parent_id = parent;
            }
            if let Some(status) = patch.status {
                match status {
                    TaskStatus::Open | TaskStatus::Doing => {
                        // Reopen: forget the closing details.
                        t.closed_at = None;
                        t.skip_reason = None;
                        t.reason_genuine = None;
                    }
                    TaskStatus::Dropped if t.status != TaskStatus::Dropped => {
                        t.closed_at = Some(tx.now().to_string());
                    }
                    TaskStatus::Done | TaskStatus::Skipped => {
                        return Err(invalid("use task_done or task_skip to close a task"));
                    }
                    TaskStatus::Dropped => {}
                }
                t.status = status;
            }
            write_task(tx, &t)?;
            let xp_delta = sync_task_xp(tx, &t, today)?;
            Ok(TaskChange {
                task: get_task(tx, id)?,
                xp_delta,
            })
        })
    }

    /// Mark done, with optional evidence (a commit, a link, a file, a sentence).
    pub fn complete_task(
        &self,
        by: &str,
        id: &str,
        evidence: Option<String>,
        today: NaiveDate,
    ) -> Result<TaskChange> {
        self.write(by, |tx| {
            let mut t = get_task(tx, id)?;
            if t.status != TaskStatus::Done {
                t.closed_at = Some(tx.now().to_string());
            }
            t.status = TaskStatus::Done;
            t.skip_reason = None;
            t.reason_genuine = None;
            if evidence.is_some() {
                t.evidence = evidence;
            }
            write_task(tx, &t)?;
            let xp_delta = sync_task_xp(tx, &t, today)?;
            Ok(TaskChange {
                task: get_task(tx, id)?,
                xp_delta,
            })
        })
    }

    /// Skip with a reason (tired, forgot, too big, avoided, phone, sick, other...) and whether
    /// that reason is the real one.
    pub fn skip_task(
        &self,
        by: &str,
        id: &str,
        reason: &str,
        genuine: bool,
        today: NaiveDate,
    ) -> Result<TaskChange> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(invalid("a skip needs a reason"));
        }
        self.write(by, |tx| {
            let mut t = get_task(tx, id)?;
            if t.status != TaskStatus::Skipped {
                t.closed_at = Some(tx.now().to_string());
            }
            t.status = TaskStatus::Skipped;
            t.skip_reason = Some(reason.to_string());
            t.reason_genuine = Some(genuine);
            write_task(tx, &t)?;
            let xp_delta = sync_task_xp(tx, &t, today)?;
            Ok(TaskChange {
                task: get_task(tx, id)?,
                xp_delta,
            })
        })
    }

    /// Tasks matching the filter, each with its subtasks and rollup.
    pub fn list_tasks(&self, f: &TaskFilter) -> Result<Vec<TaskNode>> {
        self.read(|c| {
            let mut sql = format!("SELECT {TASK_COLS} FROM task WHERE 1 = 1");
            let mut args: Vec<Value> = Vec::new();
            let arg = |v: Value, args: &mut Vec<Value>| {
                args.push(v);
                format!("?{}", args.len())
            };

            match &f.parent_id {
                Some(p) => {
                    let a = arg(p.clone().into(), &mut args);
                    sql += &format!(" AND parent_id = {a}");
                }
                None => sql += " AND parent_id IS NULL",
            }
            let statuses: Vec<String> = f
                .status
                .statuses()
                .iter()
                .map(|s| arg(s.to_string().into(), &mut args))
                .collect();
            sql += &format!(" AND status IN ({})", statuses.join(", "));

            let day_of = |d: NaiveDate| Value::from(d.format("%Y-%m-%d").to_string());
            match f.due {
                DueFilter::Any => {}
                DueFilter::OnOrBefore(d) => {
                    let a = arg(day_of(d), &mut args);
                    sql += &format!(" AND (due <= {a} OR substr(scheduled, 1, 10) <= {a})");
                }
                DueFilter::On(d) => {
                    let a = arg(day_of(d), &mut args);
                    sql += &format!(" AND (due = {a} OR substr(scheduled, 1, 10) = {a})");
                }
                DueFilter::Overdue(d) => {
                    let a = arg(day_of(d), &mut args);
                    sql += &format!(" AND due < {a}");
                }
                DueFilter::Undated => sql += " AND due IS NULL AND scheduled IS NULL",
            }
            if let Some(area) = &f.area_id {
                let a = arg(area.clone().into(), &mut args);
                sql += &format!(" AND area_id = {a}");
            }
            if let Some(project) = &f.project_id {
                let a = arg(project.clone().into(), &mut args);
                sql += &format!(" AND project_id = {a}");
            }
            if let Some(q) = &f.search {
                let a = arg(format!("%{}%", q.to_lowercase()).into(), &mut args);
                sql += &format!(" AND lower(title) LIKE {a}");
            }
            sql += " ORDER BY due IS NULL, due, scheduled IS NULL, scheduled, sort, created_at";
            if f.limit > 0 {
                sql += &format!(" LIMIT {}", f.limit);
            }

            let mut stmt = c.prepare(&sql)?;
            let tasks = stmt
                .query_map(params_from_iter(args), task_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tasks.into_iter().map(|t| node(c, t)).collect()
        })
    }

    /// The day's list: unfinished tasks due or scheduled on or before `day`, plus tasks closed
    /// on `day`. A matching subtask brings its top-level parent along.
    pub fn day_tasks(&self, day: NaiveDate) -> Result<Vec<TaskNode>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, parent_id FROM task
                 WHERE (status IN ('open', 'doing')
                        AND (due <= ?1 OR substr(scheduled, 1, 10) <= ?1))
                    OR (status IN ('done', 'skipped') AND date(closed_at, 'localtime') = ?1)
                    OR (status != 'dropped' AND (due = ?1 OR substr(scheduled, 1, 10) = ?1))",
            )?;
            let hits = stmt
                .query_map([day], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut roots: Vec<String> = Vec::new();
            for (id, parent) in hits {
                let mut root = id;
                let mut up = parent;
                while let Some(p) = up {
                    up = c
                        .query_row("SELECT parent_id FROM task WHERE id = ?1", [&p], |r| {
                            r.get(0)
                        })
                        .optional()?
                        .flatten();
                    root = p;
                }
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
            let mut nodes = roots
                .iter()
                .map(|id| node(c, get_task(c, id)?))
                .collect::<Result<Vec<_>>>()?;
            // Unfinished first, then by scheduled time, then by list order.
            nodes.sort_by(|a, b| {
                b.task
                    .status
                    .is_active()
                    .cmp(&a.task.status.is_active())
                    .then_with(|| a.task.scheduled.cmp(&b.task.scheduled))
                    .then_with(|| a.task.sort.total_cmp(&b.task.sort))
            });
            Ok(nodes)
        })
    }
}

fn write_task(tx: &Tx<'_>, t: &Task) -> Result<()> {
    tx.execute(
        "UPDATE task SET parent_id = ?2, area_id = ?3, project_id = ?4, title = ?5, notes = ?6,
             status = ?7, due = ?8, scheduled = ?9, xp_weight = ?10, skip_reason = ?11,
             reason_genuine = ?12, evidence = ?13, closed_at = ?14, updated_at = ?15
         WHERE id = ?1",
        params![
            t.id,
            t.parent_id,
            t.area_id,
            t.project_id,
            t.title,
            t.notes,
            t.status,
            t.due,
            t.scheduled,
            t.xp_weight,
            t.skip_reason,
            t.reason_genuine,
            t.evidence,
            t.closed_at,
            tx.now()
        ],
    )?;
    tx.record("task", &t.id, ChangeOp::Upsert)
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskStatus::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn add(store: &Store, title: &str, f: impl FnOnce(&mut NewTask)) -> Task {
        let mut n = NewTask {
            title: title.into(),
            ..Default::default()
        };
        f(&mut n);
        store.add_task("test", n).unwrap()
    }

    #[test]
    fn rollup_counts() {
        let r = rollup(&[Done, Done, Open, Skipped, Dropped]);
        assert_eq!(
            (r.total, r.done, r.open, r.skipped, r.dropped),
            (5, 2, 1, 1, 1)
        );
        assert!((r.progress - 0.5).abs() < 1e-9);
        assert_eq!(rollup(&[]).progress, 0.0);
        assert_eq!(rollup(&[Dropped]).progress, 0.0);
    }

    #[test]
    fn subtasks_roll_up_and_inherit_area() {
        let store = Store::open_temporary().unwrap();
        let today = day("2026-11-11");
        let parent = add(&store, "Mega task", |n| n.area_id = Some("fitness".into()));
        let a = add(&store, "Part A", |n| n.parent_id = Some(parent.id.clone()));
        let b = add(&store, "Part B", |n| n.parent_id = Some(parent.id.clone()));
        add(&store, "Part C", |n| n.parent_id = Some(parent.id.clone()));
        assert_eq!(a.area_id.as_deref(), Some("fitness"));

        store.complete_task("test", &a.id, None, today).unwrap();
        store
            .skip_task("test", &b.id, "too big", true, today)
            .unwrap();
        let node = store.get_task_node(&parent.id).unwrap();
        let r = node.rollup.unwrap();
        assert_eq!((r.total, r.done, r.skipped, r.open), (3, 1, 1, 1));
        assert!((r.progress - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn done_awards_xp_and_reopen_takes_it_back() {
        let store = Store::open_temporary().unwrap();
        let today = day("2026-11-11");
        let t = add(&store, "Hard thing", |n| {
            n.area_id = Some("work".into());
            n.xp_weight = Some(3);
        });
        let done = store
            .complete_task("test", &t.id, Some("commit abc123".into()), today)
            .unwrap();
        assert_eq!(done.xp_delta, 30);
        assert_eq!(done.task.status, Done);
        assert_eq!(done.task.evidence.as_deref(), Some("commit abc123"));
        assert!(done.task.closed_at.is_some());

        // Completing twice doesn't double count.
        assert_eq!(
            store
                .complete_task("test", &t.id, None, today)
                .unwrap()
                .xp_delta,
            0
        );

        let reopened = store
            .update_task(
                "test",
                &t.id,
                TaskPatch {
                    status: Some(Open),
                    ..Default::default()
                },
                today,
            )
            .unwrap();
        assert_eq!(reopened.xp_delta, -30);
        assert!(reopened.task.closed_at.is_none());
        assert_eq!(store.xp_summary(today).unwrap().overall.total_xp, 0);
    }

    #[test]
    fn skip_needs_reason_and_keeps_genuine_flag() {
        let store = Store::open_temporary().unwrap();
        let today = day("2026-11-11");
        let t = add(&store, "Read", |_| {});
        assert!(store.skip_task("test", &t.id, "  ", false, today).is_err());
        let s = store
            .skip_task("test", &t.id, "phone", false, today)
            .unwrap();
        assert_eq!(s.task.status, Skipped);
        assert_eq!(s.task.skip_reason.as_deref(), Some("phone"));
        assert_eq!(s.task.reason_genuine, Some(false));
        assert_eq!(s.xp_delta, 0);
    }

    #[test]
    fn parent_loops_are_refused() {
        let store = Store::open_temporary().unwrap();
        let today = day("2026-11-11");
        let a = add(&store, "A", |_| {});
        let b = add(&store, "B", |n| n.parent_id = Some(a.id.clone()));
        let patch = TaskPatch {
            parent_id: Some(Some(b.id.clone())),
            ..Default::default()
        };
        assert!(store.update_task("test", &a.id, patch, today).is_err());
    }

    #[test]
    fn list_filters() {
        let store = Store::open_temporary().unwrap();
        let today = day("2026-11-11");
        add(&store, "Overdue", |n| n.due = Some(day("2026-11-09")));
        add(&store, "Today", |n| n.due = Some(today));
        add(&store, "Scheduled today", |n| {
            n.scheduled = Some("2026-11-11T15:00".into())
        });
        add(&store, "Later", |n| n.due = Some(day("2026-11-20")));
        add(&store, "Someday", |_| {});

        let titles = |f: TaskFilter| -> Vec<String> {
            store
                .list_tasks(&f)
                .unwrap()
                .into_iter()
                .map(|n| n.task.title)
                .collect()
        };
        assert_eq!(titles(TaskFilter::default()).len(), 5);
        assert_eq!(
            titles(TaskFilter {
                due: DueFilter::OnOrBefore(today),
                ..Default::default()
            }),
            vec!["Overdue", "Today", "Scheduled today"]
        );
        assert_eq!(
            titles(TaskFilter {
                due: DueFilter::Overdue(today),
                ..Default::default()
            }),
            vec!["Overdue"]
        );
        assert_eq!(
            titles(TaskFilter {
                due: DueFilter::Undated,
                ..Default::default()
            }),
            vec!["Someday"]
        );
        assert_eq!(
            titles(TaskFilter {
                search: Some("LATE".into()),
                ..Default::default()
            }),
            vec!["Later"]
        );
    }

    #[test]
    fn day_score_counts_through_subtasks() {
        let store = Store::open_temporary().unwrap();
        let today = day("2026-11-11");
        let parent = add(&store, "Mega", |n| n.due = Some(today));
        for i in 0..4 {
            let s = add(&store, &format!("part {i}"), |n| {
                n.parent_id = Some(parent.id.clone())
            });
            if i < 3 {
                store.complete_task("test", &s.id, None, today).unwrap();
            }
        }
        let single = add(&store, "Single", |n| n.due = Some(today));
        store
            .skip_task("test", &single.id, "tired", true, today)
            .unwrap();
        add(&store, "Tomorrow", |n| n.due = Some(day("2026-11-12")));

        let nodes = store.day_tasks(today).unwrap();
        assert_eq!(nodes.len(), 2);
        let score = day_score(&nodes);
        assert_eq!(
            (score.done, score.skipped, score.open, score.counted),
            (3, 1, 1, 5)
        );
        assert!((score.rate - 0.6).abs() < 1e-9);
    }

    #[test]
    fn scheduled_forms() {
        let today = day("2026-11-11");
        assert_eq!(normalize_scheduled("today", today).unwrap(), "2026-11-11");
        assert_eq!(
            normalize_scheduled("tomorrow 06:30", today).unwrap(),
            "2026-11-12T06:30"
        );
        assert_eq!(
            normalize_scheduled("15:00", today).unwrap(),
            "2026-11-11T15:00"
        );
        assert_eq!(
            normalize_scheduled("2026-11-20T09:00", today).unwrap(),
            "2026-11-20T09:00"
        );
        assert!(normalize_scheduled("tomorrow 25:00", today).is_err());
    }

    #[test]
    fn resolve_by_prefix() {
        let store = Store::open_temporary().unwrap();
        let t = add(&store, "Find me", |_| {});
        assert_eq!(store.resolve_task(&t.id[..6]).unwrap().id, t.id);
        assert!(store.resolve_task("zz").is_err(), "too short");
    }

    #[test]
    fn writes_are_logged_for_sync() {
        let store = Store::open_temporary().unwrap();
        let before = store.latest_version().unwrap();
        let t = add(&store, "Logged", |_| {});
        store
            .complete_task("test", &t.id, None, day("2026-11-11"))
            .unwrap();
        let batch = store.changes_since(before, 100).unwrap();
        let entities: Vec<&str> = batch.changes.iter().map(|c| c.entity.as_str()).collect();
        assert!(entities.contains(&"task"));
        assert!(entities.contains(&"xp_event"));
    }
}
