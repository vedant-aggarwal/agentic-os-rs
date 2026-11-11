//! Sample day from HOME.md. Every number on the page comes from here.

use super::*;
use crate::state::ItemStatus;
use std::collections::HashMap;

pub const DATE_LABEL: &str = "Wed 11 Nov";

pub const BLOCKS: &[Block] = &[
    Block {
        id: "classes",
        title: "Classes",
        place: Some("College"),
        note: None,
        start: Time::new(9, 30),
        end: Time::new(13, 30),
    },
    Block {
        id: "shoot",
        title: "Catalog shoot",
        place: Some("The studio"),
        note: None,
        start: Time::new(14, 30),
        end: Time::new(16, 30),
    },
    Block {
        id: "gym",
        title: "Gym: shoulders + traps",
        place: None,
        note: Some("Skipped Monday. Never miss twice."),
        start: Time::new(17, 0),
        end: Time::new(18, 15),
    },
    Block {
        id: "call",
        title: "Client call",
        place: None,
        note: None,
        start: Time::new(19, 0),
        end: Time::new(19, 45),
    },
    Block {
        id: "deep",
        title: "Deep work",
        place: Some("Agentic OS"),
        note: None,
        start: Time::new(21, 0),
        end: Time::new(22, 30),
    },
];

pub const TASKS: &[Task] = &[
    Task {
        id: "ship-home",
        title: "Ship Agentic OS home page",
        detail: Some("Dioxus, phone and desktop"),
        big: true,
        xp: 120,
    },
    Task {
        id: "catalog",
        title: "Send the catalog photos to the team",
        detail: Some("From the 14:30 shoot"),
        big: false,
        xp: 40,
    },
    Task {
        id: "read",
        title: "Read 10 pages: Atomic Habits",
        detail: Some("You're on page 46"),
        big: false,
        xp: 40,
    },
];

pub const CORE: &[CoreItem] = &[
    CoreItem {
        id: "journal",
        title: "Journal",
        cue: "Night, with Nora",
        floor: None,
        anchor: false,
        xp: 30,
        xp_floor: 0,
    },
    CoreItem {
        id: "meditate",
        title: "Meditate",
        cue: "After waking",
        floor: Some("2 min"),
        anchor: false,
        xp: 20,
        xp_floor: 10,
    },
    CoreItem {
        id: "shutdown",
        title: "Shutdown",
        cue: "Before bed",
        floor: Some("Brush teeth"),
        anchor: false,
        xp: 20,
        xp_floor: 10,
    },
    CoreItem {
        id: "sleep",
        title: "Sleep before 12",
        cue: "Phone docked outside the bed",
        floor: None,
        anchor: false,
        xp: 20,
        xp_floor: 0,
    },
    CoreItem {
        id: "plan",
        title: "Plan tomorrow",
        cue: "Night handoff, tomorrow's 3",
        floor: None,
        anchor: false,
        xp: 20,
        xp_floor: 0,
    },
    CoreItem {
        id: "anchor",
        title: "Build with Claude",
        cue: "45 min",
        floor: Some("15 min"),
        anchor: true,
        xp: 30,
        xp_floor: 10,
    },
];

pub const ASKS: &[Ask] = &[
    Ask {
        id: "sam",
        from: "Nora",
        kind: "Person note",
        lead: "Sam, exam on the 20th.",
        code: None,
        options: &["Keep", "Edit", "Drop"],
    },
    Ask {
        id: "t3-axum",
        from: "T3 Code",
        kind: "Permission",
        lead: "agentic-os-rs wants to run",
        code: Some("cargo add axum"),
        options: &["Allow", "Deny"],
    },
];

pub const BRIEF: &[&str] = &[
    "Gym at 5. You skipped it Monday, so don't miss twice.",
    "The team asked for catalog photos.",
    "Book: you're on page 46 of Atomic Habits.",
];

/// Cards of the morning read. Their text is his own and lives in the brain, not here.
pub const VISION_CARDS: &[&str] = &[
    "Version I want to be",
    "Visualization",
    "Your reason for all this",
    "Core principles",
    "Stuck questions",
];

pub const LEVELS: Levels = Levels {
    level: 4,
    xp: 1740,
    xp_next: 2400,
    body: 2,
    mind: 4,
    work: 5,
    willpower: 3,
};

/// Rolling seven days ending today, so "4 of 7" is possible on a Wednesday.
pub const WEEK: &[WeekDay] = &[
    WeekDay { label: "T", mark: DayMark::Hit },
    WeekDay { label: "F", mark: DayMark::Miss },
    WeekDay { label: "S", mark: DayMark::Hit },
    WeekDay { label: "S", mark: DayMark::Hit },
    WeekDay { label: "M", mark: DayMark::Hit },
    WeekDay { label: "T", mark: DayMark::Miss },
    WeekDay { label: "W", mark: DayMark::Today },
];

pub const WEEK_HITS: usize = 4;

pub const SESSIONS: &[Session] = &[
    Session {
        id: "s1",
        repo: "agentic-os-rs",
        title: "Today page, Dioxus build",
        status: SessionStatus::Working,
        since: "12 min",
    },
    Session {
        id: "s2",
        repo: "storefront",
        title: "Catalog export script",
        status: SessionStatus::WaitingOnYou,
        since: "3 min",
    },
];

/// What is already ticked when each state opens, and the XP those ticks earned.
pub fn initial_status(state: DayState) -> (HashMap<&'static str, ItemStatus>, u32) {
    let mut map = HashMap::new();
    let mut xp = 0;
    let mut mark = |id: &'static str, status: ItemStatus, gained: u32| {
        map.insert(id, status);
        xp += gained;
    };
    match state {
        DayState::Morning => {}
        DayState::Day => {
            mark("meditate", ItemStatus::Done, 20);
        }
        DayState::Night => {
            mark("ship-home", ItemStatus::Done, 120);
            mark("catalog", ItemStatus::Done, 40);
            mark("meditate", ItemStatus::Done, 20);
            mark("anchor", ItemStatus::Floor, 10);
        }
    }
    (map, xp)
}
