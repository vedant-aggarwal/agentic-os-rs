use dioxus::prelude::*;

use crate::components::{Icon, IconName, Ring};
use crate::live;
use crate::state::{AppState, SheetKind};

pub const NORA_ROUND: Asset = asset!("/assets/img/nora-round.webp");

/// "1,760" for XP counts.
pub fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[component]
pub fn TopBar() -> Element {
    let mut state = use_context::<AppState>();
    let safe = (state.safe)();
    let lv = live::level();
    let xp = lv.total_xp.max(0) as u32;
    let pct = lv.progress as f32;
    let xp_label = thousands(xp);
    let name = live::profile_name();
    let lvl = lv.level;

    rsx! {
        header { class: "topbar",
            div { class: "tb-left",
                div { class: "tb-home",
                    h1 { class: "tb-name", "{name}" }
                    button {
                        class: "xp-pill",
                        r#type: "button",
                        aria_label: "Level {lvl}, {xp_label} XP. Open the level roadmap",
                        onclick: move |_| state.open_sheet(SheetKind::Levels),
                        Ring { pct, size: 18, width: 2.25 }
                        span { class: "xp-lvl", "Lv {lvl}" }
                        span { class: "xp-num", "{xp_label} XP" }
                    }
                }
                button {
                    class: "tb-back",
                    r#type: "button",
                    aria_label: "Back to Today",
                    onclick: move |_| state.back(),
                    Icon { name: IconName::ChevronLeft, size: 22, stroke: 2.0 }
                    span { "Today" }
                }
            }
            div { class: "tb-right",
                NoraButton {}
                button {
                    class: "tb-profile",
                    r#type: "button",
                    aria_label: if safe { "You. Safe mode is on" } else { "You" },
                    onclick: move |_| state.open_sheet(SheetKind::Profile),
                    Icon { name: IconName::User, size: 20, stroke: 1.75 }
                    if safe {
                        span { class: "tb-safe", aria_hidden: "true",
                            Icon { name: IconName::Shield, size: 10, stroke: 2.75 }
                        }
                    }
                }
            }
        }
    }
}

/// Nora's round avatar. The ring shows her state; a dot means she has something for him.
#[component]
pub fn NoraButton() -> Element {
    let mut state = use_context::<AppState>();
    rsx! {
        button {
            class: "nora-btn",
            r#type: "button",
            "data-state": "ready",
            aria_label: "Nora. She has your brief",
            onclick: move |_| state.nora.set(true),
            span { class: "nb-ring", aria_hidden: "true" }
            img { class: "nb-face", src: NORA_ROUND, alt: "" }
            span { class: "nb-dot", aria_hidden: "true" }
        }
    }
}
