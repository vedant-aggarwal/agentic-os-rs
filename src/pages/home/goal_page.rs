//! The two pages behind a goal. First tap: the goal page rises under the lifted goal
//! (price, what he has earned toward it, his reasons, specs). Second tap on the goal,
//! or the button at the bottom: the pillar page slides in.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::data::goal::{inr, Goal};
use crate::data::levels::{companion_vivid, next_milestone};
use crate::live::{self, Live};
use crate::state::AppState;

use super::hero::{goal_img, FACE_COMPANION};

#[component]
pub fn GoalPage() -> Element {
    let mut state = use_context::<AppState>();
    let open = (state.goal)().is_some();
    let goal = (state.goal_shown)();
    let spec = goal.spec();

    rsx! {
        section {
            class: "goal-page",
            class: if open { "open" },
            "data-hue": goal.slug(),
            "data-panel": "goal",
            "inert": if !open { "true" },
            aria_label: "{spec.name}",
            div { class: "gp-handle", "data-handle": "1",
                span { class: "sheet-grab" }
            }
            div { class: "gp-scroll",
                header { class: "gp-head",
                    span { class: "gp-pillar", "{goal.pillar()}" }
                    h2 { class: "gp-title", "{spec.name}" }
                    p { class: "gp-variant", "{spec.variant}" }
                }
                if let Some(price) = spec.price {
                    EarnedCard { goal, price }
                } else if goal == Goal::Mind {
                    CompanionStage {}
                }
                WordsCard { goal }
                section { class: "gp-specs",
                    h3 { class: "gp-h", if goal == Goal::Mind { "What she stands for" } else { "Specs" } }
                    dl {
                        for (k, v) in spec.specs.iter() {
                            div { key: "{k}{v}", class: "gp-spec",
                                dt { "{k}" }
                                dd { "{v}" }
                            }
                        }
                    }
                }
                button {
                    class: "btn btn-primary gp-open",
                    r#type: "button",
                    onclick: move |_| state.open_pillar(goal),
                    "Open {goal.pillar()}"
                    Icon { name: IconName::ChevronRight, size: 18, stroke: 2.0 }
                }
                p { class: "gp-hint", "Or tap the {goal_word(goal)} again." }
            }
        }
    }
}

fn goal_word(goal: Goal) -> &'static str {
    match goal {
        Goal::Mind => "companion",
        _ => "goal",
    }
}

/// Earned against the price. Nothing is logged yet, so it reads zero and says so.
#[component]
fn EarnedCard(goal: Goal, price: u32) -> Element {
    let mut state = use_context::<AppState>();
    let spec = goal.spec();
    let earned = spec.earned.unwrap_or(0);
    let pct = (earned as f32 / price as f32).clamp(0.0, 1.0);
    let left = inr(price.saturating_sub(earned));
    let price_s = inr(price);
    let earned_s = inr(earned);
    let pct_label = format!("{} percent earned", (pct * 100.0).round());

    rsx! {
        section { class: "earned",
            div { class: "earned-top",
                div {
                    span { class: "earned-label", "Price" }
                    span { class: "earned-price", "₹{price_s}" }
                }
                div { class: "earned-right",
                    span { class: "earned-label", "Earned" }
                    span { class: "earned-value", "₹{earned_s}" }
                }
            }
            div { class: "earned-bar", style: "--p: {pct:.4}", role: "img", aria_label: "{pct_label}" }
            p { class: "earned-note",
                if spec.earned.is_none() {
                    "Nothing logged yet. ₹{left} to go."
                } else {
                    "₹{left} to go."
                }
            }
            if let Some(note) = spec.price_note.clone() {
                p { class: "earned-fine", "{note}" }
            }
            button {
                class: "btn btn-ghost earned-log",
                r#type: "button",
                onclick: move |_| {
                    state.capture_for(
                        format!("Log an earning: {}", goal.spec().name),
                        "Amount and where it came from. It waits in the Inbox until earnings tracking exists.",
                    )
                },
                Icon { name: IconName::Plus, size: 16, stroke: 2.0 }
                "Log an earning"
            }
        }
    }
}

/// Mind's goal has no price: it shows how clearly she appears at his level.
#[component]
fn CompanionStage() -> Element {
    let level = live::level().level.min(255) as u8;
    let vivid = companion_vivid(level);
    let next = next_milestone(level)
        .map(|m| format!("Level {}: {}", m.level, m.companion.unwrap_or("")));
    rsx! {
        section { class: "earned companion-stage",
            img { class: "cs-face", src: FACE_COMPANION, alt: "", style: "--vivid: {vivid:.2}" }
            div {
                span { class: "earned-label", "Her stage" }
                p { class: "cs-now", "Faint at level {level}" }
                if let Some(line) = next {
                    p { class: "earned-note", "{line}" }
                }
            }
        }
    }
}

/// His own reasons. Empty until he says them; one card, one action.
#[component]
fn WordsCard(goal: Goal) -> Element {
    let mut state = use_context::<AppState>();
    let (why, feel) = if goal == Goal::Mind {
        ("Why it matters to you.", "How you will speak, think and carry yourself.")
    } else {
        ("Why you want it.", "How you will look and feel the day it is yours.")
    };
    let spec = goal.spec();
    let name = spec.name.clone();
    rsx! {
        section { class: "words",
            div { class: "words-text",
                h3 { class: "gp-h", "In your words" }
                if let Some(w) = spec.why.clone() {
                    p { "{w}" }
                } else {
                    p { class: "words-empty", "{why}" }
                    p { class: "words-empty", "{feel}" }
                }
            }
            button {
                class: "words-btn",
                r#type: "button",
                onclick: move |_| {
                    state.capture_for(
                        format!("In your words: {name}"),
                        "Why you want it and how it will feel. The Inbox keeps it until goals sync.",
                    )
                },
                Icon { name: IconName::Mic, size: 15, stroke: 2.0 }
                "Say it"
            }
        }
    }
}

#[component]
pub fn PillarPage() -> Element {
    let mut state = use_context::<AppState>();
    let live = use_context::<Live>();
    let open = (state.pillar)().is_some();
    let goal = (state.pillar_shown)();
    let safe = (state.safe)();
    let spec = goal.spec();
    let tree = live.areas.read().clone();
    let areas: Vec<live::Area> = tree.pillars.iter().filter(|p| p.id == goal.pillar_id()).flat_map(|p| p.areas.clone()).collect();
    let area_ids: Vec<String> = areas.iter().map(|a| a.id.clone()).collect();
    let today = live.today.read().clone().unwrap_or_default();
    let in_pillar = |a: &Option<String>| a.as_ref().is_some_and(|a| area_ids.contains(a));
    let tasks: Vec<live::TaskNode> = today.areas.iter().filter(|g| in_pillar(&g.area_id)).flat_map(|g| g.tasks.clone()).collect();
    let private = live::PROFILE.read().private_habits.clone();
    let habits: Vec<live::HabitView> = today
        .habits
        .iter()
        .filter(|h| in_pillar(&h.area_id) && !(safe && private.contains(&h.id)))
        .cloned()
        .collect();

    rsx! {
        section {
            class: "pillar-page",
            class: if open { "open" },
            "data-hue": goal.slug(),
            "data-panel": "pillar",
            "data-edge": "1",
            "inert": if !open { "true" },
            aria_label: "{goal.pillar()}",
            header { class: "pp-top",
                button {
                    class: "icon-btn pp-back",
                    r#type: "button",
                    aria_label: "Back to the goal",
                    onclick: move |_| state.pillar.set(None),
                    Icon { name: IconName::ChevronLeft, size: 22, stroke: 2.0 }
                }
                span { class: "pp-top-title", "{goal.pillar()}" }
                span { class: "pp-top-gap" }
            }
            div { class: "pp-scroll",
                div { class: "pp-hero",
                    img { class: "pp-art", src: goal_img(goal), alt: "", aria_hidden: "true" }
                    h1 { class: "pp-title", "{goal.pillar()}" }
                    p { class: "pp-about", "{spec.pillar_about}" }
                    span { class: "pp-level", "{goal.stat()} level {goal.stat_level()}" }
                }
                if !habits.is_empty() {
                    section { class: "pp-sec",
                        h3 { class: "pp-h", "Habits" }
                        for h in habits {
                            div { key: "{h.id}", class: "pp-row", class: if h.counts() { "done" },
                                span { class: "pp-dot", aria_hidden: "true",
                                    if h.counts() { Icon { name: IconName::Check, size: 12, stroke: 3.0 } }
                                }
                                span { class: "pp-main",
                                    span { class: "pp-name", "{h.name}" }
                                    span { class: "pp-sub", "{h.streak.current}-day streak" }
                                }
                            }
                        }
                    }
                }
                section { class: "pp-sec",
                    h3 { class: "pp-h", "Tasks today" }
                    if tasks.is_empty() {
                        p { class: "pp-sub", "Nothing due here today. Add one on Today, or tell Nora." }
                    }
                    for n in tasks {
                        div { key: "{n.task.id}", class: "pp-row", class: if n.task.done() { "done" },
                            span { class: "pp-dot", aria_hidden: "true",
                                if n.task.done() { Icon { name: IconName::Check, size: 12, stroke: 3.0 } }
                            }
                            span { class: "pp-main",
                                span { class: "pp-name", "{n.task.title}" }
                                if !n.subtasks.is_empty() {
                                    span { class: "pp-sub", "{n.subtasks.iter().filter(|t| t.done()).count()} of {n.subtasks.len()} steps" }
                                }
                            }
                        }
                    }
                }
                if !areas.is_empty() {
                    section { class: "pp-sec",
                        h3 { class: "pp-h", "Areas" }
                        for a in areas {
                            div { key: "{a.id}", class: "pp-row",
                                span { class: "pp-main", span { class: "pp-name", "{a.name}" } }
                            }
                        }
                    }
                }
                if goal == Goal::Money {
                    p { class: "pp-foot", "Work sessions and T3 projects live in the Work tab." }
                }
                button {
                    class: "pp-goal",
                    r#type: "button",
                    onclick: move |_| state.pillar.set(None),
                    img { src: if goal == Goal::Mind { FACE_COMPANION } else { goal_img(goal) }, alt: "" }
                    span { class: "pp-goal-text",
                        span { class: "pp-sub", "Goal" }
                        span { class: "pp-name", "{spec.name}" }
                    }
                    Icon { name: IconName::ChevronRight, size: 18, stroke: 1.75 }
                }
            }
        }
    }
}
