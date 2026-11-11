//! Needs you: at most three cards, hidden when empty. Marigold is used only here.

use dioxus::prelude::*;

use crate::components::SectionHeader;
use crate::data::sample::ASKS;
use crate::data::Ask;
use crate::state::TodayState;

const MAX_CARDS: usize = 3;

#[component]
pub fn NeedsYou() -> Element {
    let today = use_context::<TodayState>();
    let dismissed = (today.dismissed)();
    let asks: Vec<&Ask> = ASKS
        .iter()
        .filter(|a| !dismissed.contains(&a.id))
        .take(MAX_CARDS)
        .collect();

    if asks.is_empty() {
        return rsx! {};
    }

    rsx! {
        section { class: "needs",
            SectionHeader { title: "Needs you", meta: "{asks.len()}" }
            div { class: "stack",
                for ask in asks {
                    AskCard { key: "{ask.id}", ask: ask.clone() }
                }
            }
        }
    }
}

#[component]
fn AskCard(ask: Ask) -> Element {
    let mut today = use_context::<TodayState>();
    let id = ask.id;

    rsx! {
        article { class: "card ask",
            div { class: "ask-from",
                span { "{ask.from}" }
                span { class: "ask-kind", "{ask.kind}" }
            }
            p { class: "ask-text",
                "{ask.lead}"
                if let Some(code) = ask.code {
                    " "
                    code { "{code}" }
                }
            }
            div { class: "ask-actions",
                for (i, option) in ask.options.iter().enumerate() {
                    button {
                        key: "{option}",
                        class: if i == 0 { "btn btn-sm btn-soft" } else { "btn btn-sm btn-secondary" },
                        r#type: "button",
                        onclick: move |_| today.dismiss(id),
                        "{option}"
                    }
                }
            }
        }
    }
}
