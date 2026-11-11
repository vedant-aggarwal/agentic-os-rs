//! One host for every sheet. It stays mounted; `open` slides it in and out, and the
//! last sheet's content stays rendered while it leaves. Drag the handle down to close.

use dioxus::prelude::*;

use crate::components::icon::{Icon, IconName};
use crate::pages::sheets::{
    CallSheet, CaptureSheet, LaunchSheet, LevelsSheet, MoodSheet, PrintSheet, ProfileSheet, WaveSheet,
};
use crate::state::{AppState, SheetKind};

#[component]
pub fn SheetHost() -> Element {
    let mut state = use_context::<AppState>();
    let open = (state.sheet)().is_some();
    let kind = (state.sheet_shown)();
    let seq = (state.sheet_seq)();

    rsx! {
        div {
            class: "sheet-layer",
            class: if open { "open" },
            "inert": if !open { "true" },
            div { class: "scrim", onclick: move |_| state.close_sheet() }
            div {
                class: "sheet sheet-{kind.slug()}",
                role: "dialog",
                aria_modal: "true",
                aria_label: "{kind.title()}",
                "data-panel": "sheet",
                div { class: "sheet-handle", "data-handle": "1",
                    span { class: "sheet-grab" }
                }
                button {
                    class: "sheet-close",
                    r#type: "button",
                    aria_label: "Close",
                    onclick: move |_| state.close_sheet(),
                    Icon { name: IconName::X, size: 18, stroke: 2.0 }
                }
                {match kind {
                    SheetKind::Levels => rsx! { LevelsSheet { key: "{seq}" } },
                    SheetKind::Mood => rsx! { MoodSheet { key: "{seq}" } },
                    SheetKind::Call => rsx! { CallSheet { key: "{seq}" } },
                    SheetKind::Wave => rsx! { WaveSheet { key: "{seq}" } },
                    SheetKind::Capture => rsx! { CaptureSheet { key: "{seq}" } },
                    SheetKind::Print => rsx! { PrintSheet { key: "{seq}" } },
                    SheetKind::Launch => rsx! { LaunchSheet { key: "{seq}" } },
                    SheetKind::Profile => rsx! { ProfileSheet { key: "{seq}" } },
                }}
            }
        }
    }
}

/// Toast for small confirmations. Keyed by id so its CSS animation replays.
#[component]
pub fn Toast() -> Element {
    let state = use_context::<AppState>();
    let toast = (state.toast)();
    rsx! {
        div { class: "toast-host", role: "status", aria_live: "polite",
            if let Some((id, text)) = toast {
                div { key: "{id}", class: "toast",
                    Icon { name: IconName::Check, size: 16, stroke: 2.25 }
                    span { "{text}" }
                }
            }
        }
    }
}
