use dioxus::prelude::*;

use crate::components::icon::{Icon, IconName};
use crate::state::ItemStatus;

/// The circle at the left of a task or core row. Purely visual; the row handles input.
#[component]
pub fn Check(status: ItemStatus, #[props(default = false)] small: bool) -> Element {
    let state_class = match status {
        ItemStatus::Open => "",
        ItemStatus::Done => "done",
        ItemStatus::Floor => "floor",
        ItemStatus::Skipped(_) => "skipped",
    };
    let check_size = if small { 13 } else { 15 };

    rsx! {
        span { class: "check {state_class}", class: if small { "sm" }, aria_hidden: "true",
            {match status {
                ItemStatus::Done => rsx! { Icon { name: IconName::Check, size: check_size, stroke: 2.75 } },
                ItemStatus::Floor => rsx! { span { class: "check-dot" } },
                ItemStatus::Skipped(_) => rsx! { Icon { name: IconName::Minus, size: 13, stroke: 2.0 } },
                ItemStatus::Open => rsx! {},
            }}
        }
    }
}
