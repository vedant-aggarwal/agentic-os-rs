use dioxus::prelude::*;

use crate::components::{Icon, IconName, Ring};
use crate::data::sample::{DATE_LABEL, LEVELS};
use crate::data::Mode;
use crate::state::{Overlay, ShellState, TodaySheet, TodayState};

#[component]
pub fn Header() -> Element {
    let mut today = use_context::<TodayState>();
    let mut shell = use_context::<ShellState>();
    let mode = (today.mode)();
    let xp = LEVELS.xp + (today.xp_today)();
    let pct = xp as f32 / LEVELS.xp_next as f32;

    rsx! {
        header { class: "hdr",
            div { class: "hdr-left",
                h1 { class: "hdr-date", "{DATE_LABEL}" }
                button {
                    class: "chip",
                    class: if mode != Mode::Normal { "on" },
                    r#type: "button",
                    aria_label: "Mode: {mode.label()}. Change",
                    onclick: move |_| today.open_sheet(TodaySheet::Mode),
                    "{mode.chip_label()}"
                    Icon { name: IconName::ChevronDown, size: 14, stroke: 2.0 }
                }
            }
            div { class: "hdr-right",
                button {
                    class: "lvl",
                    r#type: "button",
                    aria_label: "Level {LEVELS.level}. Open levels",
                    onclick: move |_| today.open_sheet(TodaySheet::Levels),
                    Ring { pct, size: 24, width: 2.5 }
                    span { "Lv {LEVELS.level}" }
                }
                button {
                    class: "avatar",
                    r#type: "button",
                    aria_label: "Settings",
                    onclick: move |_| shell.open(Overlay::Placeholder("Settings")),
                    "V"
                }
            }
        }
    }
}
