//! Navigation chrome. `Rail` shows from tablet up, `Dock` (capture row + tab bar) on phones.
//! Both are always rendered; CSS picks one by width.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::capture_bar::CaptureBar;
use crate::components::icon::{Icon, IconName};
use crate::components::press::use_press;
use crate::state::{Overlay, ShellState};

fn nav_class(base: &str, active: bool) -> String {
    if active {
        format!("{base} active")
    } else {
        base.to_string()
    }
}

#[component]
pub fn Rail() -> Element {
    let route = use_route::<Route>();
    let mut shell = use_context::<ShellState>();
    let is_today = matches!(route, Route::Today { .. });

    rsx! {
        nav { class: "rail", aria_label: "Sections",
            div { class: "rail-mark", aria_hidden: "true",
                Icon { name: IconName::Sunrise, size: 18, stroke: 2.0 }
            }
            div { class: "rail-nav",
                Link { to: shell.today_route(), class: nav_class("rail-item", is_today), title: "Today", aria_label: "Today",
                    Icon { name: IconName::Home, size: 20 }
                }
                Link { to: Route::Work {}, class: nav_class("rail-item", route == Route::Work {}), title: "Work", aria_label: "Work",
                    Icon { name: IconName::Briefcase, size: 20 }
                }
                Link { to: Route::Nora {}, class: nav_class("rail-item", route == Route::Nora {}), title: "Nora", aria_label: "Nora",
                    Icon { name: IconName::Message, size: 20 }
                }
                Link { to: Route::Brain {}, class: nav_class("rail-item", route == Route::Brain {}), title: "Brain", aria_label: "Brain",
                    Icon { name: IconName::Brain, size: 20 }
                }
            }
            div { class: "rail-spacer" }
            button {
                class: "rail-item",
                r#type: "button",
                title: "Wave",
                aria_label: "Wave",
                onclick: move |_| shell.open(Overlay::Wave),
                Icon { name: IconName::Waves, size: 20 }
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

#[component]
pub fn Dock() -> Element {
    let route = use_route::<Route>();
    let mut shell = use_context::<ShellState>();
    let mut press = use_press();
    let is_today = matches!(route, Route::Today { .. });

    rsx! {
        div { class: "dock",
            div { class: "dock-row",
                CaptureBar {}
                // Tap: chat. Hold: call.
                button {
                    class: "nora-btn",
                    r#type: "button",
                    aria_label: "Nora. Tap to chat, hold to call",
                    onpointerdown: move |_| press.down(),
                    onpointercancel: move |_| press.cancel(),
                    onpointerleave: move |_| press.cancel(),
                    onpointerup: move |_| match press.up() {
                        Some(true) => shell.open(Overlay::NoraCall),
                        Some(false) => shell.open(Overlay::Nora),
                        None => {}
                    },
                    oncontextmenu: move |e| e.prevent_default(),
                    "N"
                }
                button {
                    class: "wave-btn",
                    r#type: "button",
                    aria_label: "Wave",
                    onclick: move |_| shell.open(Overlay::Wave),
                    Icon { name: IconName::Waves, size: 22 }
                }
            }
            nav { class: "tabbar", aria_label: "Tabs",
                Link { to: shell.today_route(), class: nav_class("tab", is_today),
                    Icon { name: IconName::Home, size: 22 }
                    span { "Today" }
                }
                Link { to: Route::Work {}, class: nav_class("tab", route == Route::Work {}),
                    Icon { name: IconName::Briefcase, size: 22 }
                    span { "Work" }
                }
                Link { to: Route::Nora {}, class: nav_class("tab", route == Route::Nora {}),
                    Icon { name: IconName::Message, size: 22 }
                    span { "Nora" }
                }
                Link { to: Route::Brain {}, class: nav_class("tab", route == Route::Brain {}),
                    Icon { name: IconName::Brain, size: 22 }
                    span { "Brain" }
                }
            }
        }
    }
}
