//! Client copies of the server's JSON shapes (aos-core can't build for wasm: it bundles SQLite).
//! Every struct takes `#[serde(default)]`, so a field the server adds or drops never breaks a read.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub area_id: Option<String>,
    pub title: String,
    pub notes: Option<String>,
    pub status: String,
    pub due: Option<String>,
    pub scheduled: Option<String>,
    pub xp_weight: i64,
    pub skip_reason: Option<String>,
}

impl Task {
    pub fn done(&self) -> bool {
        self.status == "done"
    }
    pub fn closed(&self) -> bool {
        matches!(self.status.as_str(), "done" | "skipped" | "dropped")
    }
    pub fn xp(&self) -> i64 {
        10 * self.xp_weight.max(1)
    }
    /// Created on this device and not yet confirmed by the server.
    pub fn local(&self) -> bool {
        self.id.starts_with("local-")
    }
    /// "15:30" when the task is scheduled at a time today.
    pub fn time(&self) -> Option<String> {
        let s = self.scheduled.as_deref()?;
        let t = s.split(['T', ' ']).nth(1)?;
        (t.len() >= 5).then(|| t[..5].to_string())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rollup {
    pub total: u32,
    pub done: u32,
    pub progress: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskNode {
    #[serde(flatten)]
    pub task: Task,
    pub subtasks: Vec<Task>,
    pub rollup: Option<Rollup>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AreaTasks {
    pub area_id: Option<String>,
    pub area: Option<String>,
    pub pillar_id: Option<String>,
    pub tasks: Vec<TaskNode>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Streak {
    pub current: u32,
    pub best: u32,
    /// intact, bent (one miss: don't miss twice) or broken.
    pub state: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HabitView {
    pub id: String,
    pub area_id: Option<String>,
    pub name: String,
    pub cue: Option<String>,
    pub normal: Option<String>,
    pub floor: Option<String>,
    pub xp: i64,
    pub anchor: bool,
    pub due_today: bool,
    /// done, floor, missed, excused, or none yet.
    pub today: Option<String>,
    pub streak: Streak,
}

impl HabitView {
    pub fn counts(&self) -> bool {
        matches!(self.today.as_deref(), Some("done" | "floor"))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mood {
    pub id: String,
    pub at: String,
    pub valence: f64,
    pub energy: Option<i64>,
    pub labels: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Level {
    pub level: u32,
    pub level_exact: f64,
    pub stage: u32,
    pub evolution: f64,
    pub total_xp: i64,
    pub into_level: i64,
    pub level_size: i64,
    pub to_next: i64,
    pub progress: f64,
}

impl Default for Level {
    fn default() -> Self {
        Level { level: 1, level_exact: 1.0, stage: 1, evolution: 0.0, total_xp: 0, into_level: 0, level_size: 100, to_next: 100, progress: 0.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodayXp {
    pub level: Level,
    pub today: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alarm {
    pub id: String,
    /// RFC 3339 UTC.
    pub at: String,
    pub label: String,
    pub kind: String,
    pub status: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Score {
    pub done: u32,
    pub skipped: u32,
    pub open: u32,
    pub counted: u32,
    pub rate: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodayView {
    pub date: String,
    pub score: Score,
    pub areas: Vec<AreaTasks>,
    pub habits: Vec<HabitView>,
    pub moods: Vec<Mood>,
    pub journal_written: bool,
    pub alarms: Vec<Alarm>,
    pub xp: TodayXp,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Area {
    pub id: String,
    pub pillar_id: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PillarTree {
    pub id: String,
    pub name: String,
    pub areas: Vec<Area>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AreaTree {
    pub pillars: Vec<PillarTree>,
    pub unfiled: Vec<Area>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JournalEntry {
    pub id: String,
    pub date: String,
    pub at: String,
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InboxItem {
    pub id: String,
    pub source: String,
    pub chat_name: String,
    pub sender_name: String,
    pub subject: String,
    pub text: String,
    pub at: String,
    pub handled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PillarXp {
    pub id: String,
    pub name: String,
    pub xp: i64,
    pub level: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct XpSummary {
    pub today: i64,
    pub week: i64,
    pub pillars: Vec<PillarXp>,
}

/// One message in Nora's merged thread.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoraMsg {
    pub id: i64,
    /// user, claude, grok or system.
    pub engine: String,
    /// app, grok, scheduler...
    pub source: String,
    /// message, ack, progress, final, to_grok...
    pub kind: String,
    pub text: String,
    pub at: i64,
}

/// A queued write: one tool call, sent through `/api/sync/push` (idempotent by `op_id`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Op {
    pub op_id: String,
    pub tool: String,
    pub args: serde_json::Value,
}

// ---------------------------------------------------------------------------------------------
// Profile: name, avatar and the goal in each hero slot. Same shape as the server's `profile`
// tool and the bundled `assets/personal/profile.json`.

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: Option<String>,
    pub avatar: Option<String>,
    pub companion: bool,
    pub private_habits: Vec<String>,
    pub hero: Vec<HeroGoal>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeroGoal {
    /// physical, money or mind.
    pub slot: String,
    pub goal_id: Option<String>,
    pub name: Option<String>,
    pub variant: Option<String>,
    /// Rupees.
    pub price: Option<i64>,
    pub earned: Option<i64>,
    pub progress: Option<f64>,
    pub price_note: Option<String>,
    /// "Label: value" lines.
    pub specs: Vec<String>,
    pub why: Option<String>,
    pub about: Option<String>,
}

impl Profile {
    /// Built into this binary from `assets/personal/profile.json`, or empty.
    pub fn bundled() -> Profile {
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/profile.json"))).unwrap_or_default()
    }

    pub fn display_name(&self) -> String {
        self.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "You".into())
    }

    pub fn hero(&self, slot: &str) -> Option<&HeroGoal> {
        self.hero.iter().find(|h| h.slot == slot)
    }

    /// The server's answer wins field by field over what we have (empty server fields keep ours).
    pub fn merged_with(&self, server: Profile) -> Profile {
        let mut out = self.clone();
        if server.name.is_some() {
            out.name = server.name;
        }
        if server.avatar.is_some() {
            out.avatar = server.avatar;
        }
        if !server.hero.is_empty() {
            out.companion = server.companion;
            out.private_habits = server.private_habits;
            out.hero = server.hero;
        }
        out
    }
}
