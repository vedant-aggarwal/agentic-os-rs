//! The other three tabs, until they are built.

use dioxus::prelude::*;

use crate::state::ShellState;

#[component]
pub fn Work() -> Element {
    rsx! {
        PlaceholderPage { title: "Work", body: "T3 sessions, approvals and voice-to-job. Next build." }
    }
}

#[component]
pub fn Nora() -> Element {
    rsx! {
        PlaceholderPage { title: "Nora", body: "Chat, calls, bots and her day. Next build." }
    }
}

#[component]
pub fn Brain() -> Element {
    rsx! {
        PlaceholderPage { title: "Brain", body: "Ask, notes, journal, books, people. Next build." }
    }
}

#[component]
fn PlaceholderPage(title: &'static str, body: &'static str) -> Element {
    let shell = use_context::<ShellState>();
    rsx! {
        main { class: "page page-empty",
            div { class: "empty",
                h1 { class: "hdr-date", "{title}" }
                p { class: "card-sub", "{body}" }
                Link { to: shell.today_route(), class: "btn btn-secondary", "Back to Today" }
            }
        }
    }
}
