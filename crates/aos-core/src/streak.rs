//! Habit cadence and streaks with the "never miss twice" rule.
//!
//! - A done (or floor) day adds one to the streak.
//! - One missed day **bends** the streak: it keeps its count, but the next due day matters.
//! - Two missed due days in a row **end** it: the count goes back to zero.
//! - An excused day (sick, travel) neither counts nor breaks.
//! - Today with no log yet is still open, so it never counts as a miss.
//! - Days the cadence doesn't ask for are skipped entirely.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use crate::error::{Error, invalid};
use crate::model::HabitLogStatus;

/// Which days a habit is due. Stored as text: `daily` or `mon,wed,fri`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Cadence {
    Daily,
    Weekdays(Vec<Weekday>),
}

impl Cadence {
    pub fn is_due(&self, day: NaiveDate) -> bool {
        match self {
            Cadence::Daily => true,
            Cadence::Weekdays(days) => days.contains(&day.weekday()),
        }
    }
}

impl fmt::Display for Cadence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cadence::Daily => f.write_str("daily"),
            Cadence::Weekdays(days) => {
                let names: Vec<String> =
                    days.iter().map(|d| d.to_string().to_lowercase()).collect();
                f.write_str(&names.join(","))
            }
        }
    }
}

impl FromStr for Cadence {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "" | "daily" | "every day" => return Ok(Cadence::Daily),
            "weekdays" => {
                return Ok(Cadence::Weekdays(vec![
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ]));
            }
            _ => {}
        }
        let mut days = Vec::new();
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let wd = crate::time::parse_weekday(part).ok_or_else(|| {
                invalid(format!(
                    "bad cadence day '{part}' (use daily or mon,wed,fri)"
                ))
            })?;
            if !days.contains(&wd) {
                days.push(wd);
            }
        }
        if days.is_empty() {
            return Err(invalid("empty cadence"));
        }
        days.sort_by_key(|d| d.num_days_from_monday());
        Ok(Cadence::Weekdays(days))
    }
}

impl From<Cadence> for String {
    fn from(c: Cadence) -> String {
        c.to_string()
    }
}

impl TryFrom<String> for Cadence {
    type Error = Error;
    fn try_from(s: String) -> Result<Self, Error> {
        s.parse()
    }
}

/// Streak state on a given day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreakState {
    /// No miss since the last done day.
    Intact,
    /// One miss: the next due day decides.
    Bent,
    /// Two misses in a row (or never started).
    Broken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreakInfo {
    /// Done days in the current run.
    pub current: u32,
    /// Longest run ever.
    pub best: u32,
    pub state: StreakState,
    pub last_done: Option<NaiveDate>,
}

/// Compute the streak from `start` up to and including `today`.
///
/// `logs` maps a day to its result. Due days before today with no log count as missed.
pub fn compute(
    cadence: &Cadence,
    start: NaiveDate,
    today: NaiveDate,
    logs: &BTreeMap<NaiveDate, HabitLogStatus>,
) -> StreakInfo {
    let mut current = 0u32;
    let mut best = 0u32;
    let mut misses_in_row = 0u32;
    let mut last_done = None;

    // A log before the start date (backfill) moves the start back.
    let first = logs.keys().next().copied().map_or(start, |d| d.min(start));
    let mut day = first;
    while day <= today {
        let logged = logs.get(&day).copied();
        if cadence.is_due(day) || logged.is_some_and(HabitLogStatus::counts_as_done) {
            match logged {
                Some(s) if s.counts_as_done() => {
                    current += 1;
                    misses_in_row = 0;
                    last_done = Some(day);
                }
                Some(HabitLogStatus::Excused) => {}
                // Today is still open: no log yet is not a miss.
                None if day == today => {}
                _ => {
                    misses_in_row += 1;
                    if misses_in_row >= 2 {
                        current = 0;
                    }
                }
            }
            best = best.max(current);
        }
        day += Duration::days(1);
    }

    let state = if current == 0 {
        StreakState::Broken
    } else if misses_in_row == 1 {
        StreakState::Bent
    } else {
        StreakState::Intact
    };
    StreakInfo {
        current,
        best,
        state,
        last_done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use HabitLogStatus::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// Build logs from a string, one char per day starting at `start`:
    /// `x` done, `f` floor, `m` missed, `e` excused, `.` nothing logged.
    fn logs(start: &str, pattern: &str) -> BTreeMap<NaiveDate, HabitLogStatus> {
        let start = d(start);
        pattern
            .chars()
            .enumerate()
            .filter_map(|(i, c)| {
                let status = match c {
                    'x' => Done,
                    'f' => Floor,
                    'm' => Missed,
                    'e' => Excused,
                    _ => return None,
                };
                Some((start + Duration::days(i as i64), status))
            })
            .collect()
    }

    fn daily(pattern: &str) -> StreakInfo {
        let start = "2026-11-01";
        let today = d(start) + Duration::days(pattern.len() as i64 - 1);
        compute(&Cadence::Daily, d(start), today, &logs(start, pattern))
    }

    #[test]
    fn all_done_is_intact() {
        let s = daily("xxxxx");
        assert_eq!((s.current, s.best, s.state), (5, 5, StreakState::Intact));
    }

    #[test]
    fn floor_counts_as_done() {
        assert_eq!(daily("xfxf").current, 4);
    }

    #[test]
    fn one_miss_bends_but_keeps_count() {
        let s = daily("xxxm");
        assert_eq!((s.current, s.state), (3, StreakState::Bent));
        // Recovering the next day continues the same streak.
        let s = daily("xxxmx");
        assert_eq!((s.current, s.state), (4, StreakState::Intact));
    }

    #[test]
    fn two_misses_in_a_row_end_it() {
        let s = daily("xxxmm");
        assert_eq!((s.current, s.best, s.state), (0, 3, StreakState::Broken));
        let s = daily("xxxmmxx");
        assert_eq!((s.current, s.best), (2, 3));
    }

    #[test]
    fn unlogged_past_day_is_a_miss() {
        // Nothing logged on day 4 or 5: two misses.
        let s = daily("xxx..");
        // Day 5 is today, so only day 4 counts as missed: bent.
        assert_eq!((s.current, s.state), (3, StreakState::Bent));
        let s = daily("xxx..x");
        assert_eq!(
            s.current, 1,
            "days 4 and 5 were both missed, so a new run starts"
        );
    }

    #[test]
    fn today_without_log_is_open() {
        let s = daily("xxx.");
        assert_eq!((s.current, s.state), (3, StreakState::Intact));
    }

    #[test]
    fn excused_days_are_neutral() {
        let s = daily("xxmeex");
        assert_eq!((s.current, s.state), (3, StreakState::Intact));
        // An excused day between two misses is skipped over, so the misses still join up.
        let s = daily("xxmem");
        assert_eq!(s.current, 0);
    }

    #[test]
    fn alternating_misses_never_break() {
        let s = daily("xmxmxmx");
        assert_eq!((s.current, s.state), (4, StreakState::Intact));
    }

    #[test]
    fn cadence_skips_off_days() {
        // Mon/Wed/Fri habit. 2026-11-02 is a Monday.
        let cad: Cadence = "mon,wed,fri".parse().unwrap();
        let start = d("2026-11-02");
        // Done Mon, Wed, Fri; nothing on Tue/Thu/Sat/Sun.
        let l = logs("2026-11-02", "x.x.x..");
        let today = d("2026-11-09"); // next Monday, not logged yet
        let s = compute(&cad, start, today, &l);
        assert_eq!((s.current, s.state), (3, StreakState::Intact));
    }

    #[test]
    fn never_started_is_broken() {
        let s = compute(
            &Cadence::Daily,
            d("2026-11-01"),
            d("2026-11-01"),
            &BTreeMap::new(),
        );
        assert_eq!((s.current, s.state), (0, StreakState::Broken));
    }

    #[test]
    fn cadence_parse_and_display() {
        assert_eq!("daily".parse::<Cadence>().unwrap(), Cadence::Daily);
        let c: Cadence = "fri, mon,wed".parse().unwrap();
        assert_eq!(c.to_string(), "mon,wed,fri");
        assert!("someday".parse::<Cadence>().is_err());
    }
}
