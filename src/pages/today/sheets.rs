//! Sheets that need the day's data: levels, mode, skip with a reason, morning read, pick 3, handoff.

use dioxus::prelude::*;

use crate::components::{Icon, IconName, Sheet};
use crate::data::sample::{CORE, LEVELS, TASKS, VISION_CARDS, WEEK, WEEK_HITS};
use crate::data::Mode;
use crate::state::{SkipReason, TodaySheet, TodayState};

#[component]
pub fn TodaySheets() -> Element {
    let mut today = use_context::<TodayState>();
    let Some(sheet) = (today.sheet)() else {
        return rsx! {};
    };
    let close = move |_: ()| today.close_sheet();

    match sheet {
        TodaySheet::Levels => rsx! {
            Sheet { title: "Level {LEVELS.level}", onclose: close, LevelsBody {} }
        },
        TodaySheet::Mode => rsx! {
            Sheet { title: "Mode", onclose: close, ModeBody {} }
        },
        TodaySheet::Skip(id) => rsx! {
            Sheet { title: "Skip with a reason", onclose: close, SkipBody { id } }
        },
        TodaySheet::Vision => rsx! {
            Sheet { title: "Morning read", onclose: close, VisionBody {} }
        },
        TodaySheet::PickThree => rsx! {
            Sheet { title: "Today's 3", onclose: close, PickBody {} }
        },
        TodaySheet::Handoff => rsx! {
            Sheet { title: "Night handoff", onclose: close, HandoffBody {} }
        },
    }
}

#[component]
fn LevelsBody() -> Element {
    let today = use_context::<TodayState>();
    let xp_today = (today.xp_today)();
    let xp = LEVELS.xp + xp_today;
    let to_next = LEVELS.xp_next.saturating_sub(xp);
    let next = LEVELS.level + 1;

    rsx! {
        p { class: "sheet-sub", "{xp} XP, {to_next} to level {next}. Today so far: +{xp_today}." }
        div { class: "stats",
            div { class: "stat", div { class: "stat-v", "{LEVELS.body}" } div { class: "stat-k", "Body" } }
            div { class: "stat", div { class: "stat-v", "{LEVELS.mind}" } div { class: "stat-k", "Mind" } }
            div { class: "stat", div { class: "stat-v", "{LEVELS.work}" } div { class: "stat-k", "Work" } }
            div { class: "stat", div { class: "stat-v", "{LEVELS.willpower}" } div { class: "stat-k", "Willpower" } }
        }
        p { class: "opt-title", "Week: {WEEK_HITS} of {WEEK.len()} days" }
        p { class: "meta", "No HP, no streaks. A miss costs nothing and asks for a reason. An urge beaten is the biggest Willpower gain." }
    }
}

#[component]
fn ModeBody() -> Element {
    let mut today = use_context::<TodayState>();
    let current = (today.mode)();

    rsx! {
        div { class: "list", role: "radiogroup", aria_label: "Mode",
            for m in Mode::ALL {
                button {
                    key: "{m}",
                    class: "opt-row",
                    r#type: "button",
                    role: "radio",
                    aria_checked: "{m == current}",
                    onclick: move |_| {
                        today.mode.set(m);
                        today.close_sheet();
                    },
                    div { class: "opt-main",
                        div { class: "opt-title", "{m.label()}" }
                        div { class: "opt-sub", "{m.hint()}" }
                    }
                    if m == current {
                        span { class: "opt-check", Icon { name: IconName::Check, size: 18, stroke: 2.25 } }
                    }
                }
            }
        }
    }
}

#[component]
fn SkipBody(id: &'static str) -> Element {
    let mut today = use_context::<TodayState>();
    let mut picked = use_signal(|| None::<SkipReason>);
    let title = TASKS
        .iter()
        .find(|t| t.id == id)
        .map(|t| t.title)
        .or_else(|| CORE.iter().find(|c| c.id == id).map(|c| c.title))
        .unwrap_or("This item");

    rsx! {
        if let Some(reason) = picked() {
            p { class: "sheet-sub", "Noted: {reason.label()}. Was that the real reason?" }
            div { class: "sheet-actions",
                button { class: "btn btn-secondary", r#type: "button", onclick: move |_| today.close_sheet(), "Yes" }
                button { class: "btn btn-secondary", r#type: "button", onclick: move |_| today.close_sheet(), "No, there's more" }
            }
        } else {
            p { class: "sheet-sub", "{title}. Why? One tap, no blame." }
            div { class: "reasons",
                for reason in SkipReason::ALL {
                    button {
                        key: "{reason.label()}",
                        class: "reason",
                        r#type: "button",
                        onclick: move |_| {
                            today.skip(id, reason);
                            picked.set(Some(reason));
                        },
                        "{reason.label()}"
                    }
                }
            }
        }
    }
}

#[component]
fn VisionBody() -> Element {
    let mut today = use_context::<TodayState>();
    rsx! {
        p { class: "sheet-sub", "Your own words, one card at a time. They live in Brain; this is the reading order." }
        div { class: "list",
            for (i, card) in VISION_CARDS.iter().enumerate() {
                div { key: "{i}", class: "opt-row",
                    div { class: "opt-main", div { class: "opt-title", "{card}" } }
                    Icon { name: IconName::ChevronRight, size: 18 }
                }
            }
        }
        div { class: "sheet-actions",
            button { class: "btn btn-primary", r#type: "button", onclick: move |_| today.close_sheet(), "Done" }
        }
    }
}

#[component]
fn PickBody() -> Element {
    let mut today = use_context::<TodayState>();
    rsx! {
        p { class: "sheet-sub", "Pre-filled from last night's handoff. One Big Thing first." }
        div { class: "list",
            for task in TASKS.iter() {
                div { key: "{task.id}", class: "opt-row",
                    div { class: "opt-main",
                        div { class: "opt-title", "{task.title}" }
                        if task.big {
                            div { class: "opt-sub", "One Big Thing" }
                        }
                    }
                }
            }
        }
        div { class: "sheet-actions",
            button { class: "btn btn-secondary", r#type: "button", onclick: move |_| today.close_sheet(), "Change" }
            button {
                class: "btn btn-primary",
                r#type: "button",
                onclick: move |_| {
                    today.three_picked.set(true);
                    today.close_sheet();
                },
                "Keep these 3"
            }
        }
    }
}

#[component]
fn HandoffBody() -> Element {
    let mut today = use_context::<TodayState>();
    const STEPS: [(&str, &str); 4] = [
        ("Tomorrow's 3", "Starts from what stayed open today"),
        ("Alarm on the first task", "One ring, then a text"),
        ("Phone docked outside the bed", "Nora asks once at night"),
        ("Shutdown routine", "Room, brush, face wash"),
    ];
    rsx! {
        div { class: "list",
            for (i, (title, sub)) in STEPS.iter().enumerate() {
                div { key: "{i}", class: "opt-row",
                    div { class: "opt-main",
                        div { class: "opt-title", "{title}" }
                        div { class: "opt-sub", "{sub}" }
                    }
                }
            }
        }
        div { class: "sheet-actions",
            button { class: "btn btn-primary", r#type: "button", onclick: move |_| today.close_sheet(), "Close" }
        }
    }
}
