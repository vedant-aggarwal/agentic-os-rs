//! App-wide UI state. One context, shared by the shell, the home and every layer.
//! Layers (sheets, Nora, goal and pillar pages) stay mounted; these signals only
//! say which one is open, so CSS can animate both the way in and the way out.

use dioxus::prelude::*;

use crate::data::goal::Goal;
use crate::data::mood::Checkin;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lock {
    Locked,
    /// Face ID accepted; the glyph plays its tick, then the home reveals.
    Opening,
    Open,
}

impl Lock {
    pub fn slug(self) -> &'static str {
        match self {
            Lock::Locked => "locked",
            Lock::Opening => "opening",
            Lock::Open => "open",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SheetKind {
    Levels,
    Mood,
    Call,
    Wave,
    Capture,
    Print,
    Launch,
    Profile,
}

impl SheetKind {
    pub fn slug(self) -> &'static str {
        match self {
            SheetKind::Levels => "levels",
            SheetKind::Mood => "mood",
            SheetKind::Call => "call",
            SheetKind::Wave => "wave",
            SheetKind::Capture => "capture",
            SheetKind::Print => "print",
            SheetKind::Launch => "launch",
            SheetKind::Profile => "profile",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            SheetKind::Levels => "Level roadmap",
            SheetKind::Mood => "Check in",
            SheetKind::Call => "Calling Nora",
            SheetKind::Wave => "Wave",
            SheetKind::Capture => "Capture",
            SheetKind::Print => "Print today's plan",
            SheetKind::Launch => "Launch work",
            SheetKind::Profile => "You",
        }
    }
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub lock: Signal<Lock>,
    pub safe: Signal<bool>,
    pub sheet: Signal<Option<SheetKind>>,
    /// Last sheet opened. Stays set while the sheet slides out.
    pub sheet_shown: Signal<SheetKind>,
    /// Bumped on every open, so a sheet's own state starts fresh.
    pub sheet_seq: Signal<u32>,
    pub nora: Signal<bool>,
    /// First tap on a goal: its detail page.
    pub goal: Signal<Option<Goal>>,
    pub goal_shown: Signal<Goal>,
    /// Second tap (or the button on the detail page): the pillar page.
    pub pillar: Signal<Option<Goal>>,
    pub pillar_shown: Signal<Goal>,
    pub checkin: Signal<Option<Checkin>>,
    pub toast: Signal<Option<(u32, String)>>,
    pub captures: Signal<Vec<String>>,
    /// What a capture is for, when a page opened it with a purpose (title, hint).
    pub capture_for: Signal<Option<(String, String)>>,
}

impl AppState {
    pub fn new(locked: bool, safe: bool) -> Self {
        AppState {
            lock: Signal::new(if locked { Lock::Locked } else { Lock::Open }),
            safe: Signal::new(safe),
            sheet: Signal::new(None),
            sheet_shown: Signal::new(SheetKind::Levels),
            sheet_seq: Signal::new(0),
            nora: Signal::new(false),
            goal: Signal::new(None),
            goal_shown: Signal::new(Goal::Money),
            pillar: Signal::new(None),
            pillar_shown: Signal::new(Goal::Money),
            checkin: Signal::new(None),
            toast: Signal::new(None),
            captures: Signal::new(Vec::new()),
            capture_for: Signal::new(None),
        }
    }

    pub fn open_sheet(&mut self, kind: SheetKind) {
        if kind == SheetKind::Capture {
            self.capture_for.set(None);
        }
        self.sheet_shown.set(kind);
        *self.sheet_seq.write() += 1;
        self.sheet.set(Some(kind));
    }

    pub fn close_sheet(&mut self) {
        self.sheet.set(None);
    }

    /// Capture with a purpose: the sheet shows `title` and `hint` instead of the generic copy.
    pub fn capture_for(&mut self, title: impl Into<String>, hint: impl Into<String>) {
        self.open_sheet(SheetKind::Capture);
        self.capture_for.set(Some((title.into(), hint.into())));
    }

    /// Two-step goal tap: the first opens the goal, a tap on the open goal opens its pillar.
    pub fn tap_goal(&mut self, goal: Goal) {
        if goal.private() && *self.safe.peek() {
            return;
        }
        if *self.goal.peek() == Some(goal) {
            self.open_pillar(goal);
        } else {
            self.pillar.set(None);
            self.goal_shown.set(goal);
            self.goal.set(Some(goal));
        }
    }

    pub fn open_pillar(&mut self, goal: Goal) {
        self.pillar_shown.set(goal);
        self.pillar.set(Some(goal));
    }

    /// One step back: pillar page, then goal page.
    pub fn back(&mut self) {
        if self.pillar.peek().is_some() {
            self.pillar.set(None);
        } else {
            self.goal.set(None);
        }
    }

    /// Escape closes the top layer.
    pub fn escape(&mut self) {
        if self.sheet.peek().is_some() {
            self.close_sheet();
        } else if *self.nora.peek() {
            self.nora.set(false);
        } else {
            self.back();
        }
    }

    pub fn set_safe(&mut self, safe: bool) {
        self.safe.set(safe);
        if safe && self.goal.peek().is_some_and(|g| g.private()) {
            self.pillar.set(None);
            self.goal.set(None);
        }
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        let id = self.toast.peek().as_ref().map_or(1, |(id, _)| id + 1);
        self.toast.set(Some((id, text.into())));
    }
}
