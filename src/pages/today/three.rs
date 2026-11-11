//! Today's 3. Tap ticks, hold skips with a reason. The One Big Thing is the first and largest row.

use dioxus::prelude::*;

use crate::components::{use_press, Check, SectionHeader};
use crate::data::sample::TASKS;
use crate::data::{Mode, Task};
use crate::state::{ItemStatus, TodaySheet, TodayState};

#[component]
pub fn TodaysThree() -> Element {
    let today = use_context::<TodayState>();
    let done = today.tasks_done();
    let exam = (today.mode)() == Mode::Exam;

    rsx! {
        section { class: "three",
            SectionHeader { title: "Today's 3", meta: "{done} of {TASKS.len()}" }
            if exam {
                p { class: "note", "Exam mode: core only. The three wait for you." }
            } else {
                div { class: "list",
                    for task in TASKS.iter() {
                        TaskRow { key: "{task.id}", task: task.clone() }
                    }
                }
            }
        }
    }
}

#[component]
fn TaskRow(task: Task) -> Element {
    let mut today = use_context::<TodayState>();
    let mut press = use_press();
    // (sequence, amount): a new sequence remounts the float so its animation replays.
    let mut float = use_signal(|| (0u32, 0u32));
    let status = today.status_of(task.id);
    let id = task.id;
    let xp = task.xp;
    let (seq, amount) = float();

    let mut toggle = move || {
        if let Some(gain) = today.toggle(id, xp, 0) {
            float.set((seq + 1, gain));
        }
    };

    let state_class = match status {
        ItemStatus::Done | ItemStatus::Floor => "done",
        ItemStatus::Skipped(_) => "skipped",
        ItemStatus::Open => "",
    };

    rsx! {
        div {
            class: "task {state_class}",
            class: if task.big { "big" },
            role: "button",
            tabindex: "0",
            aria_pressed: "{status.counts_done()}",
            onpointerdown: move |_| press.down(),
            onpointercancel: move |_| press.cancel(),
            onpointerleave: move |_| press.cancel(),
            onpointerup: move |_| match press.up() {
                Some(true) => today.open_sheet(TodaySheet::Skip(id)),
                Some(false) => toggle(),
                None => {}
            },
            oncontextmenu: move |e| e.prevent_default(),
            onkeydown: move |e| {
                if e.key() == Key::Enter {
                    toggle();
                }
            },
            span { class: "check-wrap",
                Check { status }
                if seq > 0 {
                    span { key: "xp-{seq}", class: "xp-float", aria_hidden: "true", "+{amount}" }
                }
            }
            div { class: "task-body",
                if task.big {
                    div { class: "task-tag", "One Big Thing" }
                }
                div { class: "task-title", "{task.title}" }
                {match status {
                    ItemStatus::Skipped(reason) => rsx! { div { class: "task-detail", "Skipped: {reason.label()}" } },
                    _ => rsx! {
                        if let Some(detail) = task.detail {
                            div { class: "task-detail", "{detail}" }
                        }
                    },
                }}
            }
            // Hover affordance for pointers; touch uses the hold.
            button {
                class: "task-skip",
                r#type: "button",
                tabindex: "-1",
                onpointerdown: move |e| e.stop_propagation(),
                onpointerup: move |e| {
                    e.stop_propagation();
                    today.open_sheet(TodaySheet::Skip(id));
                },
                "Skip"
            }
        }
    }
}
