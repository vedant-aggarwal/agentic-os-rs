//! The shortcut sheets: Call Nora, Wave, Capture, Print plan, Launch work.
//! Each is the real screen with the back end stubbed; nothing claims to have happened.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::data::sample::{BLOCKS, CORE, DATE_LABEL, SESSIONS, TASKS};
use crate::pages::home::topbar::NORA_ROUND;
use crate::state::AppState;

#[component]
pub fn CallSheet() -> Element {
    let mut state = use_context::<AppState>();
    let mut muted = use_signal(|| false);
    let mut speaker = use_signal(|| false);
    rsx! {
        div { class: "sheet-body call",
            div { class: "call-orb", aria_hidden: "true",
                span { class: "call-core",
                    img { src: NORA_ROUND, alt: "" }
                }
            }
            h2 { class: "sheet-title call-name", "Nora" }
            p { class: "call-status", "Not connected yet" }
            p { class: "sheet-sub call-note", "Calls will run on her voice engine with today's context. This build has the screen only." }
            div { class: "call-btns",
                button {
                    class: "call-btn",
                    class: if speaker() { "on" },
                    r#type: "button",
                    aria_pressed: if speaker() { "true" } else { "false" },
                    onclick: move |_| speaker.toggle(),
                    span { class: "call-ico", Icon { name: IconName::Speaker, size: 22, stroke: 1.75 } }
                    span { "Speaker" }
                }
                button {
                    class: "call-btn",
                    class: if muted() { "on" },
                    r#type: "button",
                    aria_pressed: if muted() { "true" } else { "false" },
                    onclick: move |_| muted.toggle(),
                    span { class: "call-ico", Icon { name: if muted() { IconName::MicOff } else { IconName::Mic }, size: 22, stroke: 1.75 } }
                    span { "Mute" }
                }
                button {
                    class: "call-btn",
                    r#type: "button",
                    onclick: move |_| {
                        state.close_sheet();
                        state.nora.set(true);
                    },
                    span { class: "call-ico", Icon { name: IconName::Message, size: 22, stroke: 1.75 } }
                    span { "Chat" }
                }
                button {
                    class: "call-btn call-end",
                    r#type: "button",
                    onclick: move |_| state.close_sheet(),
                    span { class: "call-ico", Icon { name: IconName::PhoneOff, size: 22, stroke: 2.0 } }
                    span { "End" }
                }
            }
        }
    }
}

/// Ten minutes with an urge. The circle breathes: in for four, out for six.
#[component]
pub fn WaveSheet() -> Element {
    let mut state = use_context::<AppState>();
    let mut running = use_signal(|| false);
    rsx! {
        div { class: "sheet-body wave", class: if running() { "running" },
            h2 { class: "sheet-title", "Wave" }
            p { class: "sheet-sub",
                if running() {
                    "This is a wave, not an order. It rises, peaks and passes. Breathe with the circle."
                } else {
                    "Ten minutes. Notice it, breathe, let it pass. Every step can be skipped."
                }
            }
            div { class: "breath", aria_hidden: "true",
                span { class: "breath-ring" }
                span { class: "breath-core" }
                span { class: "breath-word", if running() { "Breathe" } else { "10 min" } }
            }
            div { class: "sheet-actions",
                if running() {
                    button { class: "btn btn-glass", r#type: "button", onclick: move |_| state.close_sheet(), "I'm okay" }
                    button {
                        class: "btn btn-primary btn-grow",
                        r#type: "button",
                        onclick: move |_| {
                            state.close_sheet();
                            state.open_sheet(crate::state::SheetKind::Call);
                        },
                        "Call Nora"
                    }
                } else {
                    button { class: "btn btn-primary btn-wide", r#type: "button", onclick: move |_| running.set(true), "Start" }
                }
            }
            p { class: "sheet-fine", "Stays on this phone." }
        }
    }
}

#[component]
pub fn CaptureSheet() -> Element {
    let mut state = use_context::<AppState>();
    let mut text = use_signal(String::new);
    let mut listening = use_signal(|| false);
    let (title, hint) = (state.capture_for)().unwrap_or_else(|| {
        ("Capture".to_string(), "A thought, a task, a reason. It goes to the Inbox.".to_string())
    });
    rsx! {
        div { class: "sheet-body capture",
            h2 { class: "sheet-title", "{title}" }
            p { class: "sheet-sub", "{hint}" }
            textarea {
                class: "cap-field",
                placeholder: "Type it, or hold the mic",
                aria_label: "Capture",
                rows: "3",
                value: "{text}",
                oninput: move |e| text.set(e.value()),
            }
            div { class: "cap-row",
                button {
                    class: "cap-mic",
                    class: if listening() { "on" },
                    r#type: "button",
                    aria_pressed: if listening() { "true" } else { "false" },
                    aria_label: "Voice",
                    onclick: move |_| listening.toggle(),
                    Icon { name: IconName::Mic, size: 24, stroke: 2.0 }
                    span { class: "cap-bars", aria_hidden: "true",
                        for i in 0..5 { span { key: "{i}", style: "--i: {i}" } }
                    }
                }
                span { class: "cap-hint",
                    if listening() { "Voice capture arrives with the native build" } else { "Voice" }
                }
                button {
                    class: "btn btn-primary",
                    r#type: "button",
                    disabled: text().trim().is_empty(),
                    onclick: move |_| {
                        let t = text().trim().to_string();
                        if !t.is_empty() {
                            state.captures.write().push(t);
                            state.close_sheet();
                            state.toast("Saved to Inbox");
                        }
                    },
                    "Save"
                }
            }
        }
    }
}

#[component]
pub fn PrintSheet() -> Element {
    let mut state = use_context::<AppState>();
    rsx! {
        div { class: "sheet-body print",
            h2 { class: "sheet-title", "Today's plan" }
            p { class: "sheet-sub", "One page for the desk. Prints every morning at 05:00 once the printer is linked." }
            div { class: "paper",
                div { class: "paper-head",
                    span { class: "paper-date", "{DATE_LABEL}" }
                    span { class: "paper-brand", "Agentic OS" }
                }
                p { class: "paper-h", "Today's 3" }
                for t in TASKS.iter() {
                    p { key: "{t.id}", class: "paper-line", span { class: "paper-box" } "{t.title}" }
                }
                p { class: "paper-h", "Schedule" }
                for b in BLOCKS.iter() {
                    p { key: "{b.id}", class: "paper-line paper-time", span { class: "paper-t", "{b.start}" } "{b.title}" }
                }
                p { class: "paper-h", "Core" }
                p { class: "paper-line paper-core",
                    for c in CORE.iter() {
                        span { key: "{c.id}", class: "paper-chip", span { class: "paper-box" } "{c.title}" }
                    }
                }
            }
            div { class: "sheet-actions",
                button {
                    class: "btn btn-primary btn-wide",
                    r#type: "button",
                    onclick: move |_| state.toast("Printer not linked yet"),
                    Icon { name: IconName::Printer, size: 17, stroke: 2.0 }
                    "Print"
                }
            }
        }
    }
}

#[component]
pub fn LaunchSheet() -> Element {
    let mut state = use_context::<AppState>();
    rsx! {
        div { class: "sheet-body launch",
            h2 { class: "sheet-title", "Launch work" }
            p { class: "sheet-sub", "Start a T3 session by voice, or jump back into one." }
            button {
                class: "launch-voice",
                r#type: "button",
                onclick: move |_| state.toast("T3 bridge arrives with the Work tab"),
                span { class: "launch-mic", Icon { name: IconName::Mic, size: 22, stroke: 2.0 } }
                span { class: "launch-text",
                    span { class: "launch-t", "New session" }
                    span { class: "launch-s", "Say what to build and where" }
                }
            }
            p { class: "launch-h", "Running" }
            for s in SESSIONS.iter() {
                div { key: "{s.id}", class: "launch-row",
                    span { class: "launch-ico", Icon { name: IconName::Terminal, size: 17, stroke: 2.0 } }
                    span { class: "launch-text",
                        span { class: "launch-t", "{s.title}" }
                        span { class: "launch-s", "{s.repo}, {s.since}" }
                    }
                    span { class: "launch-status {s.status.class()}", "{s.status.word()}" }
                }
            }
        }
    }
}
