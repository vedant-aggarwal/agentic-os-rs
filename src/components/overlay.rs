//! Sheets the dock and rail can open on any page: Nora, a call, Wave, placeholders.

use dioxus::prelude::*;

use crate::components::icon::{Icon, IconName};
use crate::components::ring::Ring;
use crate::components::sheet::Sheet;
use crate::data::sample::BRIEF;
use crate::state::{Overlay, ShellState};

#[component]
pub fn OverlayHost() -> Element {
    let mut shell = use_context::<ShellState>();
    let Some(overlay) = (shell.overlay)() else {
        return rsx! {};
    };
    let close = move |_: ()| shell.close();

    match overlay {
        Overlay::Nora => rsx! {
            Sheet { title: "Nora", onclose: close, NoraBody {} }
        },
        Overlay::NoraCall => rsx! {
            Sheet { title: "Calling Nora", onclose: close,
                p { class: "sheet-sub", "Calls run on Codex voice with today's context. This build stops at the screen." }
                div { class: "sheet-actions",
                    button { class: "btn btn-secondary", r#type: "button", onclick: move |_| shell.close(), "End" }
                }
            }
        },
        Overlay::Wave => rsx! {
            Sheet { title: "Wave", onclose: close, WaveBody {} }
        },
        Overlay::Placeholder(title) => rsx! {
            Sheet { title: "{title}", onclose: close,
                p { class: "sheet-sub", "Coming in the next build." }
            }
        },
    }
}

#[component]
fn NoraBody() -> Element {
    let mut shell = use_context::<ShellState>();
    rsx! {
        div { class: "nora-head",
            span { class: "nora-av", aria_hidden: "true", "N" }
            div {
                div { class: "nora-name", "Nora" }
                div { class: "nora-status", "Idle" }
            }
        }
        div { class: "msgs",
            div { class: "msg-time", "Brief, 07:40" }
            for line in BRIEF.iter() {
                p { class: "msg", "{line}" }
            }
        }
        div { class: "sheet-actions",
            button { class: "btn btn-secondary", r#type: "button", onclick: move |_| shell.close(), "Open chat" }
            button { class: "btn btn-primary", r#type: "button", onclick: move |_| shell.open(Overlay::NoraCall),
                Icon { name: IconName::Phone, size: 16 }
                "Call"
            }
        }
    }
}

/// Ten-minute urge flow, stubbed: the ring fills over ten minutes once started.
#[component]
fn WaveBody() -> Element {
    let mut shell = use_context::<ShellState>();
    let mut started = use_signal(|| false);
    let running = started();

    rsx! {
        div { class: "wave-ring", class: if running { "running" },
            Ring { pct: if running { 1.0 } else { 0.0 }, size: 132, width: 4.0 }
            span { class: "wave-time", "10 min" }
        }
        p { class: "sheet-sub",
            if running {
                "This is an urge, not a command. It peaks and passes."
            } else {
                "Recognize, pause, breathe, reset, spot the lie. Every step can be skipped."
            }
        }
        div { class: "sheet-actions",
            if running {
                button { class: "btn btn-secondary", r#type: "button", onclick: move |_| shell.close(), "I'm okay" }
                button { class: "btn btn-primary", r#type: "button", onclick: move |_| shell.open(Overlay::NoraCall), "Call Nora" }
            } else {
                button { class: "btn btn-primary", r#type: "button", onclick: move |_| started.set(true), "Start" }
            }
        }
        p { class: "meta", style: "margin-top:12px;text-align:center", "Stays on this device." }
    }
}
