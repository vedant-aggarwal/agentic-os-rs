//! Wave (urge help) in Nora's day.
//!
//! - She can read the log (`wave_context`, `wave_stats`, `wave_list`) for the night journal and
//!   the close-the-day talk. The log is private: she never shames, never counts a slip as a
//!   failure of character, and never mentions it unless he does or the data shows a pattern.
//! - **Night guard.** When the setting is on, the scheduler asks once a night, at the set time
//!   (default 22:30), "Is your phone docked outside the bed?" as a reminder (so the push module
//!   carries it to the phone) and she records his yes or no with `wave_guard_answer`.

use anyhow::Result;
use chrono::{Local, NaiveTime};

use crate::nora::Nora;

/// Line for the chat system prompt.
pub const CHAT_RULE: &str = "- Wave (his urge help): read `wave_context` or `wave_stats` for the night journal and when he talks about urges. Facts only, no shame; a slip is data, and nothing resets. Use `wave_start` only when he asks you to start one. If he answers the night question \"phone docked outside the bed?\", record it with `wave_guard_answer`.";

/// What a call about a Wave should do (the app sends it as the call reason).
pub const CALL_RULE: &str = "He is riding out an urge right now. Be calm and short. Don't lecture and never shame. Keep him talking and moving for the rest of the ten minutes: slow breathing, cold water, a walk, one honest sentence about what is going on. If he asks to log how it ended, use `wave_finish`.";

/// Line for the close-the-day call script.
pub const CLOSE_DAY_STEP: &str = "7. Wave (only if `wave_context` shows urges today or this week): one gentle question about the hardest hour. If he slipped, ask what was happening and what he will change; never push, never shame. If he beat one, name it plainly.";

/// For the call context: the week in a few lines.
pub fn context(core: &aos_core::Store, max: usize) -> String {
    match core.wave_context(aos_core::time::local_today(), max) {
        Ok(s) if !s.contains("no urges logged") => s,
        _ => String::new(),
    }
}

/// What the write-up after a close-the-day call should do about Wave.
pub fn writeup_extra(close_day: bool) -> &'static str {
    if close_day {
        "\nWave: read `wave_context`. If there were urges today, add one factual line to the journal text (counts, the hour, what helped). No shame and no advice he didn't ask for."
    } else {
        ""
    }
}

/// Called by the scheduler every 15 s. Asks once a night when the guard is on, within the
/// hour after the set time. Returns true when it asked.
pub fn night_guard_tick(nora: &Nora) -> Result<bool> {
    let core = nora.core();
    let s = core.wave_settings()?;
    if !s.night_guard {
        return Ok(false);
    }
    let at = NaiveTime::parse_from_str(&s.night_guard_at, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(22, 30, 0).unwrap_or_default());
    let now = Local::now();
    let since = now.time().signed_duration_since(at).num_minutes();
    if !(0..60).contains(&since) {
        return Ok(false);
    }
    if !core.wave_guard_asked("nora", now.date_naive())? {
        return Ok(false);
    }
    core.set_alarm(
        "nora",
        aos_core::time::now() - chrono::Duration::seconds(1),
        "Is your phone docked outside the bed? Tell me yes or no.",
        aos_core::model::AlarmKind::Reminder,
        None,
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nora::testing::nora_with;

    #[test]
    fn night_guard_asks_once_inside_the_window() {
        let (nora, _e, _p) = nora_with(&[], None);
        assert!(!night_guard_tick(&nora).unwrap(), "off by default");
        let at = Local::now().time().format("%H:%M").to_string();
        nora.core().wave_set_settings("t", Some(true), Some(&at)).unwrap();
        assert!(night_guard_tick(&nora).unwrap());
        assert!(!night_guard_tick(&nora).unwrap(), "once a night");
        assert_eq!(crate::scheduler::fire_due_alarms(&nora).unwrap(), 1);
        let msgs = nora.thread(5, None).unwrap();
        assert!(msgs.last().unwrap().text.contains("docked outside the bed"));
        // Outside the window nothing happens.
        let later = (Local::now() + chrono::Duration::hours(3)).time().format("%H:%M").to_string();
        nora.core().wave_set_settings("t", None, Some(&later)).unwrap();
        assert!(!night_guard_tick(&nora).unwrap());
    }

    #[test]
    fn context_is_empty_without_urges() {
        let (nora, _e, _p) = nora_with(&[], None);
        assert!(context(nora.core(), 1000).is_empty());
    }
}
