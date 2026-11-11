//! Domain types shared by the store, the tools and (later) the UI.
//!
//! Timestamps are RFC 3339 UTC strings (as stored); calendar days are [`NaiveDate`].

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::streak::{Cadence, StreakInfo};

/// Declares a text-backed enum: serde, `as_str`, `FromStr`, and SQLite conversions in one go.
macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::Error;
            fn from_str(s: &str) -> crate::Result<Self> {
                match s.trim().to_ascii_lowercase().as_str() {
                    $($text => Ok($name::$variant),)+
                    other => Err(crate::error::invalid(format!(
                        "unknown {} '{}' (expected one of: {})",
                        stringify!($name),
                        other,
                        [$($text),+].join(", ")
                    ))),
                }
            }
        }

        impl rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                v.as_str()?
                    .parse()
                    .map_err(|e: crate::Error| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }
    };
}

text_enum!(
    /// Where a task stands. `open` and `doing` count as not finished.
    TaskStatus {
        Open => "open",
        Doing => "doing",
        Done => "done",
        Skipped => "skipped",
        Dropped => "dropped",
    }
);

impl TaskStatus {
    /// True for `open` and `doing`.
    pub fn is_active(self) -> bool {
        matches!(self, TaskStatus::Open | TaskStatus::Doing)
    }
}

text_enum!(
    /// One day's result for a habit. `floor` is the minimum version and counts as done.
    HabitLogStatus {
        Done => "done",
        Floor => "floor",
        Missed => "missed",
        Excused => "excused",
    }
);

impl HabitLogStatus {
    pub fn counts_as_done(self) -> bool {
        matches!(self, HabitLogStatus::Done | HabitLogStatus::Floor)
    }
}

text_enum!(
    AlarmKind {
        Alarm => "alarm",
        Reminder => "reminder",
    }
);

text_enum!(
    AlarmStatus {
        Scheduled => "scheduled",
        Fired => "fired",
        Cancelled => "cancelled",
    }
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pillar {
    pub id: String,
    pub name: String,
    pub sort: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    pub pillar_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub sort: i64,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub area_id: Option<String>,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub area_id: Option<String>,
    pub project_id: Option<String>,
    pub title: String,
    pub notes: Option<String>,
    pub status: TaskStatus,
    pub due: Option<NaiveDate>,
    /// A day (`YYYY-MM-DD`) or a day and time (`YYYY-MM-DDTHH:MM`), local.
    pub scheduled: Option<String>,
    pub xp_weight: i64,
    pub skip_reason: Option<String>,
    /// Was the skip reason the real one? Asked at close of day.
    pub reason_genuine: Option<bool>,
    pub evidence: Option<String>,
    pub sort: f64,
    pub source: String,
    pub external_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// When it became done, skipped or dropped.
    pub closed_at: Option<String>,
}

/// Progress of a parent task from its direct subtasks.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rollup {
    pub total: u32,
    pub done: u32,
    pub skipped: u32,
    pub dropped: u32,
    pub open: u32,
    /// done / (total - dropped), 0.0 when nothing counts.
    pub progress: f64,
}

/// A task with its subtasks, as lists and the Today view show it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskNode {
    #[serde(flatten)]
    pub task: Task,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub subtasks: Vec<Task>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rollup: Option<Rollup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Habit {
    pub id: String,
    pub area_id: Option<String>,
    pub name: String,
    /// The anchor cue: "after I ___".
    pub cue: Option<String>,
    /// The normal version.
    pub normal: Option<String>,
    /// The minimum version that still counts.
    pub floor: Option<String>,
    pub cadence: Cadence,
    pub xp: i64,
    /// The anchor habit (journaling): the streak that matters most.
    pub anchor: bool,
    pub archived: bool,
    pub start_date: NaiveDate,
    pub sort: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HabitLog {
    pub id: String,
    pub habit_id: String,
    pub date: NaiveDate,
    pub status: HabitLogStatus,
    pub note: Option<String>,
}

/// A habit with today's state and its streak.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HabitView {
    #[serde(flatten)]
    pub habit: Habit,
    /// Is the habit due on this day (cadence)?
    pub due_today: bool,
    pub today: Option<HabitLogStatus>,
    pub streak: StreakInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XpEvent {
    pub id: String,
    pub source: String,
    pub source_id: Option<String>,
    pub area_id: Option<String>,
    pub amount: i64,
    pub note: Option<String>,
    pub date: NaiveDate,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mood {
    pub id: String,
    pub date: NaiveDate,
    pub at: String,
    /// -1.0 (very unpleasant) to 1.0 (very pleasant), like Apple Health's State of Mind.
    pub valence: f64,
    /// 1 (drained) to 5 (full of energy).
    pub energy: Option<i64>,
    pub labels: Vec<String>,
    pub reasons: Vec<String>,
    pub note: Option<String>,
    pub healthkit_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub date: NaiveDate,
    pub at: String,
    /// `journal`, `win`, `challenge`, `improve`, or anything else the caller uses.
    pub kind: String,
    /// His words exactly as given.
    pub text: String,
    pub nora_read: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alarm {
    pub id: String,
    /// RFC 3339 UTC.
    pub at: String,
    pub label: String,
    pub kind: AlarmKind,
    pub status: AlarmStatus,
    pub task_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_enums_round_trip() {
        for s in TaskStatus::ALL {
            assert_eq!(s.as_str().parse::<TaskStatus>().unwrap(), *s);
        }
        assert_eq!(" DONE ".parse::<TaskStatus>().unwrap(), TaskStatus::Done);
        assert!("finished".parse::<TaskStatus>().is_err());
        assert_eq!(
            serde_json::to_string(&HabitLogStatus::Floor).unwrap(),
            "\"floor\""
        );
    }
}
