//! Typed model of one day. Sample values live in `sample.rs`; nothing here reads a clock.
//! Not every type is on screen yet: the bottom half of the home (timeline, habits) reuses them.

pub mod goal;
pub mod levels;
pub mod mood;
pub mod sample;

use std::{fmt, str::FromStr};

/// Which card sits on top. Derived from the clock in the real app; a query param here.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum DayState {
    Morning,
    #[default]
    Day,
    Night,
}

impl DayState {
    /// A fixed clock per state keeps screenshots reproducible.
    pub fn sample_now(self) -> Time {
        match self {
            DayState::Morning => Time::new(7, 40),
            DayState::Day => Time::new(15, 42),
            DayState::Night => Time::new(22, 40),
        }
    }
}

impl FromStr for DayState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "morning" => Ok(DayState::Morning),
            "day" => Ok(DayState::Day),
            "night" => Ok(DayState::Night),
            other => Err(format!("unknown state '{other}'")),
        }
    }
}

impl fmt::Display for DayState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DayState::Morning => "morning",
            DayState::Day => "day",
            DayState::Night => "night",
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    Exam,
    Low,
    Sunday,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Normal, Mode::Exam, Mode::Low, Mode::Sunday];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "Normal",
            Mode::Exam => "Exam",
            Mode::Low => "Low energy",
            Mode::Sunday => "Sunday",
        }
    }

    /// Segmented control, where "Mode" is already the row title.
    pub fn short(self) -> &'static str {
        match self {
            Mode::Low => "Low",
            other => other.label(),
        }
    }

    /// Header chip, which has to make sense on its own.
    pub fn chip_label(self) -> &'static str {
        match self {
            Mode::Normal => "Normal day",
            Mode::Exam => "Exam mode",
            Mode::Low => "Low energy",
            Mode::Sunday => "Sunday",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Mode::Normal => "The core six and today's 3.",
            Mode::Exam => "Core only. The three wait.",
            Mode::Low => "Three core items only.",
            Mode::Sunday => "System day: the weekly review.",
        }
    }

    /// Line shown under the state card when the day is not a normal one.
    pub fn note(self) -> Option<&'static str> {
        match self {
            Mode::Normal => None,
            Mode::Exam => Some("Exam mode: core only today."),
            Mode::Low => Some("Low energy: three core items, nothing else."),
            Mode::Sunday => Some("Sunday: system day. The review opens from the week strip."),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Normal => "normal",
            Mode::Exam => "exam",
            Mode::Low => "low",
            Mode::Sunday => "sunday",
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Time {
    pub h: u8,
    pub m: u8,
}

impl Time {
    pub const fn new(h: u8, m: u8) -> Self {
        Time { h, m }
    }

    pub const fn minutes(self) -> i32 {
        self.h as i32 * 60 + self.m as i32
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.h, self.m)
    }
}

/// "48 min", "1 h", "1 h 18 min".
pub fn span_label(mins: i32) -> String {
    let mins = mins.max(0);
    match (mins / 60, mins % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Past,
    Now,
    Ahead,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Block {
    pub id: &'static str,
    pub title: &'static str,
    pub place: Option<&'static str>,
    /// A system hint tied to this block ("never miss twice").
    pub note: Option<&'static str>,
    pub start: Time,
    pub end: Time,
}

impl Block {
    pub fn phase(&self, now: Time) -> Phase {
        if now < self.start {
            Phase::Ahead
        } else if now >= self.end {
            Phase::Past
        } else {
            Phase::Now
        }
    }

    pub fn progress(&self, now: Time) -> f32 {
        let total = (self.end.minutes() - self.start.minutes()).max(1) as f32;
        ((now.minutes() - self.start.minutes()) as f32 / total).clamp(0.0, 1.0)
    }

    pub fn minutes_left(&self, now: Time) -> i32 {
        self.end.minutes() - now.minutes()
    }

    pub fn minutes_until(&self, now: Time) -> i32 {
        self.start.minutes() - now.minutes()
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Task {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: Option<&'static str>,
    pub big: bool,
    pub xp: u32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CoreItem {
    pub id: &'static str,
    pub title: &'static str,
    /// The anchor cue: when this shows up.
    pub cue: &'static str,
    /// The smallest version that still counts.
    pub floor: Option<&'static str>,
    pub anchor: bool,
    pub xp: u32,
    pub xp_floor: u32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Ask {
    pub id: &'static str,
    pub from: &'static str,
    pub kind: &'static str,
    pub lead: &'static str,
    pub code: Option<&'static str>,
    pub options: &'static [&'static str],
}

#[derive(Clone, PartialEq, Debug)]
pub struct Levels {
    pub level: u8,
    pub xp: u32,
    pub xp_next: u32,
    pub body: u8,
    pub mind: u8,
    pub work: u8,
    pub willpower: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DayMark {
    Hit,
    Miss,
    Today,
}

#[derive(Clone, PartialEq, Debug)]
pub struct WeekDay {
    pub label: &'static str,
    pub mark: DayMark,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionStatus {
    Working,
    WaitingOnYou,
    Done,
}

impl SessionStatus {
    pub fn word(self) -> &'static str {
        match self {
            SessionStatus::Working => "Working",
            SessionStatus::WaitingOnYou => "Waiting on you",
            SessionStatus::Done => "Done",
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            SessionStatus::Working => "working",
            SessionStatus::WaitingOnYou => "waiting",
            SessionStatus::Done => "done",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Session {
    pub id: &'static str,
    pub repo: &'static str,
    pub title: &'static str,
    pub status: SessionStatus,
    pub since: &'static str,
}
