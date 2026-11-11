//! Seven dots for the rolling week. No streaks: a miss is a hollow dot and one hint.

use dioxus::prelude::*;

use crate::components::SectionHeader;
use crate::data::sample::{WEEK, WEEK_HITS};
use crate::data::DayMark;

fn mark_class(mark: DayMark) -> &'static str {
    match mark {
        DayMark::Hit => "hit",
        DayMark::Miss => "miss",
        DayMark::Today => "today",
    }
}

#[component]
pub fn WeekStrip() -> Element {
    let yesterday_missed = WEEK
        .len()
        .checked_sub(2)
        .map(|i| WEEK[i].mark == DayMark::Miss)
        .unwrap_or(false);

    rsx! {
        section { class: "week",
            SectionHeader { title: "Week", meta: "{WEEK_HITS} of {WEEK.len()}" }
            div { class: "week-dots", role: "list",
                for (i, day) in WEEK.iter().enumerate() {
                    div { key: "{i}", class: "wd", class: if day.mark == DayMark::Today { "today" }, role: "listitem",
                        span { class: "dot {mark_class(day.mark)}", aria_hidden: "true" }
                        span { "{day.label}" }
                    }
                }
            }
            if yesterday_missed {
                p { class: "week-hint", "Yesterday was a miss. Never miss twice." }
            }
        }
    }
}
