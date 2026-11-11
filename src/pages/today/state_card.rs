//! The card at the top. Morning: start the day. Day: now and next. Night: close the day.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::data::sample::{BLOCKS, BRIEF, CORE, TASKS};
use crate::data::{span_label, DayState, Mode, Phase, Task};
use crate::state::{ItemStatus, Overlay, ShellState, SkipReason, TodaySheet, TodayState};

#[component]
pub fn StateCard() -> Element {
    let today = use_context::<TodayState>();
    match today.day {
        DayState::Morning => rsx! { MorningCard {} },
        DayState::Day => rsx! { NowCard {} },
        DayState::Night => rsx! { NightCard {} },
    }
}

#[component]
fn StepNumber(n: u8, done: bool, now: bool) -> Element {
    rsx! {
        span { class: "step-n", class: if done { "done" }, class: if now && !done { "now" }, aria_hidden: "true",
            if done {
                Icon { name: IconName::Check, size: 13, stroke: 2.75 }
            } else {
                "{n}"
            }
        }
    }
}

#[component]
fn ModeNote() -> Element {
    let today = use_context::<TodayState>();
    match (today.mode)().note() {
        Some(note) => rsx! { p { class: "mode-note", "{note}" } },
        None => rsx! {},
    }
}

#[component]
fn NowCard() -> Element {
    let today = use_context::<TodayState>();
    let now = today.now;
    let current = BLOCKS.iter().find(|b| b.phase(now) == Phase::Now);
    let next = BLOCKS.iter().find(|b| b.phase(now) == Phase::Ahead);
    let pct = current.map(|b| (b.progress(now) * 100.0).round()).unwrap_or(0.0);

    rsx! {
        section { class: "card state-card",
            div { class: "card-top",
                span { class: "eyebrow", "Now" }
                if let Some(b) = current {
                    span { class: "meta num now-left", "{span_label(b.minutes_left(now))} left" }
                } else {
                    span { class: "meta num", "{now}" }
                }
            }
            if let Some(b) = current {
                h2 { class: "card-title", "{b.title}" }
                p { class: "card-sub num",
                    "{b.start}–{b.end}"
                    if let Some(place) = b.place { ", {place}" }
                }
                div { class: "prog", style: "--p: {pct}%", aria_hidden: "true", i {} }
            } else {
                h2 { class: "card-title", "Free" }
                if let Some(n) = next {
                    p { class: "card-sub", "Until {n.start}, {span_label(n.minutes_until(now))} from now." }
                }
            }
            if let Some(n) = next {
                div { class: "next",
                    span { class: "next-label", "Next" }
                    span { class: "next-title", "{n.title}" }
                    span { class: "next-time num", "{n.start}" }
                }
                if let Some(note) = n.note {
                    p { class: "hint", "{note}" }
                }
            }
            ModeNote {}
        }
    }
}

#[component]
fn MorningCard() -> Element {
    let mut today = use_context::<TodayState>();
    let now = today.now;
    let first = BLOCKS.iter().find(|b| b.phase(now) == Phase::Ahead);
    let read_done = (today.read_done)();
    let picked = (today.three_picked)();
    let mode = (today.mode)();

    rsx! {
        section { class: "card state-card",
            div { class: "card-top",
                span { class: "eyebrow", "Morning" }
                span { class: "meta num", "{now}" }
            }
            h2 { class: "card-title", "Start your day" }
            if let Some(b) = first {
                p { class: "card-sub", "Five minutes. {b.title} at {b.start}." }
            }
            div { class: "steps",
                div { class: "step",
                    StepNumber { n: 1, done: read_done, now: !read_done }
                    div {
                        div { class: "step-title", "Morning read" }
                        div { class: "step-sub", "Vision, visualization, your reason" }
                    }
                    button {
                        class: if read_done { "btn btn-sm btn-secondary" } else { "btn btn-sm btn-primary" },
                        r#type: "button",
                        onclick: move |_| {
                            today.read_done.set(true);
                            today.open_sheet(TodaySheet::Vision);
                        },
                        if read_done { "Again" } else { "Read" }
                    }
                }
                div { class: "step",
                    StepNumber { n: 2, done: true, now: false }
                    div { div { class: "step-title", "Nora's brief" } }
                    span {}
                    div { class: "step-body brief",
                        for line in BRIEF.iter() {
                            p { "{line}" }
                        }
                    }
                }
                div { class: "step",
                    StepNumber { n: 3, done: picked, now: read_done && !picked }
                    div {
                        div { class: "step-title", "Pick today's 3" }
                        div { class: "step-sub",
                            if picked { "Picked. They're below." } else { "Pre-filled from last night's handoff" }
                        }
                    }
                    if !picked {
                        button {
                            class: "btn btn-sm btn-secondary",
                            r#type: "button",
                            onclick: move |_| today.open_sheet(TodaySheet::PickThree),
                            "Review"
                        }
                    }
                }
                div { class: "step",
                    StepNumber { n: 4, done: false, now: false }
                    div { div { class: "step-title", "Mode" } }
                    span {}
                    div { class: "step-body",
                        div { class: "seg", role: "group", aria_label: "Mode",
                            for m in Mode::ALL {
                                button {
                                    key: "{m}",
                                    r#type: "button",
                                    class: if m == mode { "on" },
                                    aria_pressed: "{m == mode}",
                                    onclick: move |_| today.mode.set(m),
                                    "{m.short()}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NightCard() -> Element {
    let mut today = use_context::<TodayState>();
    let mut shell = use_context::<ShellState>();
    let now = today.now;
    let done = today.tasks_done();
    let core_done = today.core_done();
    // The first task still open asks for a reason; then "Real reason?" once.
    let open_task: Option<&Task> = TASKS
        .iter()
        .find(|t| today.status_of(t.id) == ItemStatus::Open);
    let mut asking_real = use_signal(|| false);
    let checkin_done = open_task.is_none() && !asking_real();

    rsx! {
        section { class: "card state-card",
            div { class: "card-top",
                span { class: "eyebrow", "Night" }
                span { class: "meta num", "{now}" }
            }
            h2 { class: "card-title", "Close the day" }
            p { class: "card-sub", "{done} of 3 done. Core {core_done} of {CORE.len()}." }
            div { class: "steps",
                div { class: "step",
                    StepNumber { n: 1, done: checkin_done, now: !checkin_done }
                    div {
                        div { class: "step-title", "Check-in" }
                        div { class: "step-sub",
                            if asking_real() {
                                "Noted. Real reason?"
                            } else if let Some(task) = open_task {
                                "{task.title} is open. Why?"
                            } else {
                                "All three accounted for."
                            }
                        }
                    }
                    span {}
                    if asking_real() {
                        div { class: "step-body",
                            div { class: "inline-chips",
                                button { class: "reason", r#type: "button", onclick: move |_| asking_real.set(false), "Yes" }
                                button { class: "reason", r#type: "button", onclick: move |_| asking_real.set(false), "No, there's more" }
                            }
                        }
                    } else if let Some(task) = open_task {
                        ReasonChips {
                            id: task.id,
                            onpick: move |reason| {
                                today.skip(task.id, reason);
                                asking_real.set(true);
                            },
                        }
                    }
                }
                div { class: "step",
                    StepNumber { n: 2, done: false, now: checkin_done }
                    div {
                        div { class: "step-title", "Journal with Nora" }
                        div { class: "step-sub", "Three hard questions from today" }
                    }
                    div { class: "step-actions",
                        button { class: "btn btn-sm btn-primary", r#type: "button", onclick: move |_| shell.open(Overlay::NoraCall), "Call" }
                        button { class: "btn btn-sm btn-secondary", r#type: "button", onclick: move |_| shell.open(Overlay::Nora), "Chat" }
                    }
                }
                div { class: "step",
                    StepNumber { n: 3, done: false, now: false }
                    div {
                        div { class: "step-title", "Handoff" }
                        div { class: "step-sub", "Tomorrow's 3, alarm, phone docked" }
                    }
                    button {
                        class: "icon-btn",
                        r#type: "button",
                        aria_label: "Open handoff",
                        onclick: move |_| today.open_sheet(TodaySheet::Handoff),
                        Icon { name: IconName::ChevronRight, size: 18 }
                    }
                }
            }
            ModeNote {}
        }
    }
}

/// One-tap reasons, inline in the night card.
#[component]
fn ReasonChips(id: &'static str, onpick: EventHandler<SkipReason>) -> Element {
    rsx! {
        div { class: "step-body",
            div { class: "inline-chips", role: "group", aria_label: "Reason for {id}",
                for reason in SkipReason::ALL {
                    button {
                        key: "{reason.label()}",
                        class: "reason",
                        r#type: "button",
                        onclick: move |_| onpick.call(reason),
                        "{reason.label()}"
                    }
                }
            }
        }
    }
}
