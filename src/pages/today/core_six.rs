//! The core six: compact rows, each with its floor version one tap away. The anchor is marked.

use dioxus::prelude::*;

use crate::components::{Check, SectionHeader};
use crate::data::sample::CORE;
use crate::data::{CoreItem, Mode};
use crate::state::{ItemStatus, TodayState};

#[component]
pub fn CoreSix() -> Element {
    let today = use_context::<TodayState>();
    let low = (today.mode)() == Mode::Low;
    let shown = if low { 3 } else { CORE.len() };
    let done = today.core_done();

    rsx! {
        section { class: "core",
            SectionHeader { title: "Core", meta: "{done} of {CORE.len()}" }
            if low {
                p { class: "note", "Low energy: three items only." }
            }
            div { class: "list",
                for item in CORE.iter().take(shown) {
                    CoreRow { key: "{item.id}", item: item.clone() }
                }
            }
        }
    }
}

#[component]
fn CoreRow(item: CoreItem) -> Element {
    let mut today = use_context::<TodayState>();
    let mut float = use_signal(|| (0u32, 0u32));
    let status = today.status_of(item.id);
    let id = item.id;
    let xp = item.xp;
    let xp_floor = item.xp_floor;
    let (seq, amount) = float();

    let state_class = match status {
        ItemStatus::Done => "done",
        ItemStatus::Floor => "floor",
        ItemStatus::Skipped(_) => "skipped",
        ItemStatus::Open => "",
    };

    rsx! {
        div { class: "core-row {state_class}",
            button {
                class: "core-main",
                r#type: "button",
                aria_pressed: "{status.counts_done()}",
                onclick: move |_| {
                    if let Some(gain) = today.toggle(id, xp, xp_floor) {
                        float.set((seq + 1, gain));
                    }
                },
                span { class: "check-wrap",
                    Check { status, small: true }
                    if seq > 0 {
                        span { key: "xp-{seq}", class: "xp-float", aria_hidden: "true", "+{amount}" }
                    }
                }
                span { class: "core-text",
                    span { class: "core-title", "{item.title}" }
                    span { class: "core-cue",
                        if item.anchor {
                            span { class: "anchor-tag", "Anchor" }
                        }
                        "{item.cue}"
                        if let (Some(floor), ItemStatus::Open) = (item.floor, status) {
                            span { class: "core-floor", "Floor: {floor}" }
                        }
                    }
                }
            }
            if let (Some(floor), ItemStatus::Open) = (item.floor, status) {
                button {
                    class: "pill",
                    r#type: "button",
                    aria_label: "Did the floor: {floor}",
                    onclick: move |_| {
                        if let Some(gain) = today.floor(id, xp_floor) {
                            float.set((seq + 1, gain));
                        }
                    },
                    "Floor"
                }
            } else if status == ItemStatus::Floor {
                span { class: "pill on", "Floor done" }
            }
        }
    }
}
