//! Navigation: a floating tab bar on phones, a slim sidebar from 1100 px.
//! Both are always rendered; CSS picks one by width.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::icon::{Icon, IconName};
use crate::state::{AppState, SheetKind};

pub const FACE_L1: Asset = asset!("/assets/img/me-l1-face.webp");

#[derive(Clone, Copy, PartialEq)]
struct Tab {
    label: &'static str,
    icon: IconName,
}

const TABS: [Tab; 5] = [
    Tab { label: "Today", icon: IconName::Sun },
    Tab { label: "Inbox", icon: IconName::Inbox },
    Tab { label: "Life", icon: IconName::Sprout },
    Tab { label: "Work", icon: IconName::Terminal },
    Tab { label: "Mission", icon: IconName::Mountain },
];

fn tab_route(i: usize) -> Route {
    match i {
        1 => Route::Inbox {},
        2 => Route::Life {},
        3 => Route::Work {},
        4 => Route::Mission {},
        _ => Route::Today { lock: None, mode: None },
    }
}

fn tab_index(route: &Route) -> usize {
    match route {
        Route::Today { .. } => 0,
        Route::Inbox {} => 1,
        Route::Life {} => 2,
        Route::Work {} => 3,
        Route::Mission {} => 4,
        Route::Hero3dDemo { .. } => 0,
    }
}

#[component]
pub fn TabBar() -> Element {
    let route = use_route::<Route>();
    let state = use_context::<AppState>();
    let active = tab_index(&route);
    // Out of the way while a goal or Nora fills the screen.
    let hidden = (state.goal)().is_some() || (state.nora)();

    rsx! {
        nav {
            class: "tabbar",
            class: if hidden { "is-hidden" },
            aria_label: "Sections",
            style: "--tab: {active}",
            span { class: "tab-pill", aria_hidden: "true" }
            for (i, tab) in TABS.iter().enumerate() {
                Link {
                    key: "{tab.label}",
                    to: tab_route(i),
                    class: if i == active { "tab active" } else { "tab" },
                    aria_current: if i == active { "page" },
                    Icon { name: tab.icon, size: 22, stroke: if i == active { 2.0 } else { 1.6 } }
                    span { class: "tab-label", "{tab.label}" }
                }
            }
        }
    }
}

#[component]
pub fn Sidebar() -> Element {
    let route = use_route::<Route>();
    let mut state = use_context::<AppState>();
    let active = tab_index(&route);

    rsx! {
        nav { class: "sidebar", aria_label: "Sections",
            button {
                class: "sb-btn sb-capture",
                r#type: "button",
                title: "Capture",
                aria_label: "Capture",
                onclick: move |_| state.open_sheet(SheetKind::Capture),
                Icon { name: IconName::Plus, size: 20, stroke: 2.0 }
            }
            div { class: "sb-nav",
                for (i, tab) in TABS.iter().enumerate() {
                    Link {
                        key: "{tab.label}",
                        to: tab_route(i),
                        class: if i == active { "sb-btn active" } else { "sb-btn" },
                        title: "{tab.label}",
                        aria_label: "{tab.label}",
                        aria_current: if i == active { "page" },
                        Icon { name: tab.icon, size: 20, stroke: if i == active { 2.0 } else { 1.6 } }
                    }
                }
            }
            div { class: "sb-spacer" }
            button {
                class: "sb-avatar",
                r#type: "button",
                aria_label: "You",
                onclick: move |_| state.open_sheet(SheetKind::Profile),
                img { src: FACE_L1, alt: "" }
            }
        }
    }
}
