//! Local UI state. Two layers: the shell (dock, rail, overlays reachable everywhere)
//! and the Today page (ticks, mode, sheets that need the day's data).

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::app::Route;
use crate::data::{sample, DayState, Mode, Time};
use crate::theme::ThemePref;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkipReason {
    Tired,
    Forgot,
    TooBig,
    Avoided,
    Phone,
    Sick,
    Other,
}

impl SkipReason {
    pub const ALL: [SkipReason; 7] = [
        SkipReason::Tired,
        SkipReason::Forgot,
        SkipReason::TooBig,
        SkipReason::Avoided,
        SkipReason::Phone,
        SkipReason::Sick,
        SkipReason::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SkipReason::Tired => "Tired",
            SkipReason::Forgot => "Forgot",
            SkipReason::TooBig => "Too big",
            SkipReason::Avoided => "Avoided it",
            SkipReason::Phone => "Phone, reels",
            SkipReason::Sick => "Sick",
            SkipReason::Other => "Other",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ItemStatus {
    #[default]
    Open,
    Done,
    /// The floor version was done. Counts.
    Floor,
    Skipped(SkipReason),
}

impl ItemStatus {
    pub fn counts_done(self) -> bool {
        matches!(self, ItemStatus::Done | ItemStatus::Floor)
    }
}

/// Overlays the dock and rail can open on any page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    Nora,
    NoraCall,
    Wave,
    Placeholder(&'static str),
}

#[derive(Clone, Copy)]
pub struct ShellState {
    pub overlay: Signal<Option<Overlay>>,
    pub captures: Signal<Vec<String>>,
    /// Bumped per capture so the "Saved" toast replays.
    pub capture_seq: Signal<u32>,
    /// Dev params of the last Today view, so the Today tab returns to the same screen.
    pub today_params: Signal<(Option<DayState>, Option<ThemePref>)>,
}

impl ShellState {
    pub fn new() -> Self {
        ShellState {
            overlay: Signal::new(None),
            captures: Signal::new(Vec::new()),
            capture_seq: Signal::new(0),
            today_params: Signal::new((None, None)),
        }
    }

    pub fn open(&mut self, overlay: Overlay) {
        self.overlay.set(Some(overlay));
    }

    pub fn close(&mut self) {
        self.overlay.set(None);
    }

    pub fn capture(&mut self, text: String) {
        self.captures.write().push(text);
        *self.capture_seq.write() += 1;
    }

    pub fn today_route(&self) -> Route {
        let (state, theme) = (self.today_params)();
        Route::Today { state, theme }
    }
}

/// Sheets that belong to the Today page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TodaySheet {
    Levels,
    Mode,
    Skip(&'static str),
    Vision,
    PickThree,
    Handoff,
}

#[derive(Clone, Copy)]
pub struct TodayState {
    pub day: DayState,
    pub now: Time,
    pub mode: Signal<Mode>,
    pub status: Signal<HashMap<&'static str, ItemStatus>>,
    pub xp_today: Signal<u32>,
    pub timeline_open: Signal<bool>,
    pub dismissed: Signal<Vec<&'static str>>,
    pub sheet: Signal<Option<TodaySheet>>,
    pub read_done: Signal<bool>,
    pub three_picked: Signal<bool>,
}

impl TodayState {
    pub fn new(day: DayState) -> Self {
        let (status, xp) = sample::initial_status(day);
        TodayState {
            day,
            now: day.sample_now(),
            mode: Signal::new(Mode::Normal),
            status: Signal::new(status),
            xp_today: Signal::new(xp),
            timeline_open: Signal::new(false),
            dismissed: Signal::new(Vec::new()),
            sheet: Signal::new(None),
            read_done: Signal::new(false),
            three_picked: Signal::new(false),
        }
    }

    pub fn status_of(&self, id: &str) -> ItemStatus {
        self.status.read().get(id).copied().unwrap_or_default()
    }

    /// Tap on an item. Returns the XP gained when it became done, so the row can float it.
    pub fn toggle(&mut self, id: &'static str, xp_done: u32, xp_floor: u32) -> Option<u32> {
        match self.status_of(id) {
            ItemStatus::Done => {
                self.status.write().insert(id, ItemStatus::Open);
                let mut xp = self.xp_today.write();
                *xp = xp.saturating_sub(xp_done);
                None
            }
            ItemStatus::Floor => {
                let gain = xp_done.saturating_sub(xp_floor);
                self.status.write().insert(id, ItemStatus::Done);
                *self.xp_today.write() += gain;
                Some(gain)
            }
            ItemStatus::Open | ItemStatus::Skipped(_) => {
                self.status.write().insert(id, ItemStatus::Done);
                *self.xp_today.write() += xp_done;
                Some(xp_done)
            }
        }
    }

    pub fn floor(&mut self, id: &'static str, xp_floor: u32) -> Option<u32> {
        if self.status_of(id) != ItemStatus::Open {
            return None;
        }
        self.status.write().insert(id, ItemStatus::Floor);
        *self.xp_today.write() += xp_floor;
        Some(xp_floor)
    }

    pub fn skip(&mut self, id: &'static str, reason: SkipReason) {
        self.status.write().insert(id, ItemStatus::Skipped(reason));
    }

    pub fn dismiss(&mut self, id: &'static str) {
        self.dismissed.write().push(id);
    }

    pub fn open_sheet(&mut self, sheet: TodaySheet) {
        self.sheet.set(Some(sheet));
    }

    pub fn close_sheet(&mut self) {
        self.sheet.set(None);
    }

    pub fn tasks_done(&self) -> usize {
        sample::TASKS
            .iter()
            .filter(|t| self.status_of(t.id).counts_done())
            .count()
    }

    pub fn core_done(&self) -> usize {
        sample::CORE
            .iter()
            .filter(|c| self.status_of(c.id).counts_done())
            .count()
    }
}
