//! Desktop column 1: the day to scale, 07:00 to 24:00, with a Now line.

use dioxus::prelude::*;

use crate::components::SectionHeader;
use crate::data::sample::BLOCKS;
use crate::data::{Block, Phase, Time};
use crate::state::TodayState;

const HOUR_PX: f32 = 44.0;
const FIRST_HOUR: i32 = 7;
const LAST_HOUR: i32 = 24;

fn y(minutes: i32) -> f32 {
    (minutes - FIRST_HOUR * 60) as f32 * HOUR_PX / 60.0
}

#[component]
pub fn DayGrid() -> Element {
    let today = use_context::<TodayState>();
    let now = today.now;
    let height = y(LAST_HOUR * 60);
    let now_top = y(now.minutes());

    rsx! {
        section { class: "daygrid",
            SectionHeader { title: "Calendar", meta: "{BLOCKS.len()} blocks" }
            div { class: "dg", style: "height:{height}px",
                for hour in FIRST_HOUR..LAST_HOUR {
                    HourLine { key: "h{hour}", hour }
                }
                for block in BLOCKS.iter() {
                    GridBlock { key: "{block.id}", block: block.clone(), now }
                }
                div { class: "dg-now", style: "top:{now_top}px", aria_hidden: "true",
                    span { "{now}" }
                }
            }
        }
    }
}

#[component]
fn HourLine(hour: i32) -> Element {
    let top = y(hour * 60);
    let label = format!("{hour:02}");
    rsx! {
        div { class: "dg-hour", style: "top:{top}px", aria_hidden: "true",
            span { "{label}" }
        }
    }
}

#[component]
fn GridBlock(block: Block, now: Time) -> Element {
    let top = y(block.start.minutes()) + 1.0;
    let height = y(block.end.minutes()) - y(block.start.minutes()) - 2.0;
    let phase_class = match block.phase(now) {
        Phase::Past => "past",
        Phase::Now => "now",
        Phase::Ahead => "ahead",
    };

    rsx! {
        div {
            class: "dg-block {phase_class}",
            class: if height < 44.0 { "short" },
            style: "top:{top}px;height:{height}px",
            div { class: "dg-bt", "{block.title}" }
            div { class: "dg-bs",
                "{block.start}–{block.end}"
                if let Some(place) = block.place { ", {place}" }
            }
        }
    }
}
