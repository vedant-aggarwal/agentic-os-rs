//! Calendar blocks as a short list (phone and tablet). Collapsed after four.

use dioxus::prelude::*;

use crate::components::{Icon, IconName, SectionHeader};
use crate::data::sample::BLOCKS;
use crate::data::{Block, DayState, Phase, Time};
use crate::state::TodayState;

const COLLAPSED_ROWS: usize = 4;

#[component]
pub fn TimelineList() -> Element {
    let mut today = use_context::<TodayState>();
    let now = today.now;
    let open = (today.timeline_open)();
    let total = BLOCKS.len();
    let limit = if open { total } else { COLLAPSED_ROWS.min(total) };
    let title = match today.day {
        DayState::Day => "Rest of today",
        DayState::Morning | DayState::Night => "Today",
    };
    let has_current = BLOCKS.iter().any(|b| b.phase(now) == Phase::Now);

    rsx! {
        section { class: "tl-list",
            SectionHeader { title, meta: "{total} blocks" }
            div { class: "tl",
                for (i, block) in BLOCKS.iter().enumerate().take(limit) {
                    TimelineRow {
                        key: "{block.id}",
                        block: block.clone(),
                        now,
                        // A "now" rule before the first block still ahead, when nothing is running.
                        rule_before: !has_current
                            && block.phase(now) == Phase::Ahead
                            && (i == 0 || BLOCKS[i - 1].phase(now) == Phase::Past),
                    }
                }
            }
            if total > COLLAPSED_ROWS {
                button {
                    class: "tl-more",
                    r#type: "button",
                    aria_expanded: "{open}",
                    onclick: move |_| today.timeline_open.set(!open),
                    if open {
                        "Show less"
                        Icon { name: IconName::ChevronUp, size: 14, stroke: 2.0 }
                    } else {
                        "Show all {total}"
                        Icon { name: IconName::ChevronDown, size: 14, stroke: 2.0 }
                    }
                }
            }
        }
    }
}

#[component]
fn TimelineRow(block: Block, now: Time, rule_before: bool) -> Element {
    let phase_class = match block.phase(now) {
        Phase::Past => "past",
        Phase::Now => "now",
        Phase::Ahead => "ahead",
    };

    rsx! {
        if rule_before {
            div { class: "nowline", "{now}" }
        }
        div { class: "tl-row {phase_class}",
            div { class: "tl-time",
                "{block.start}"
                small { "{block.end}" }
            }
            div { class: "tl-bar", aria_hidden: "true" }
            div {
                div { class: "tl-title", "{block.title}" }
                if let Some(place) = block.place {
                    div { class: "tl-sub", "{place}" }
                }
            }
        }
    }
}
