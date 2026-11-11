//! Third column from tablet up: Nora, then Needs you (its own module), then running work.

use dioxus::prelude::*;

use crate::components::{Icon, IconName, SectionHeader};
use crate::data::sample::{BRIEF, SESSIONS};
use crate::data::SessionStatus;
use crate::state::{Overlay, ShellState};

#[component]
pub fn NoraPanel() -> Element {
    let mut shell = use_context::<ShellState>();
    let mut draft = use_signal(String::new);
    let mut sent = use_signal(Vec::<String>::new);

    let mut send = move || {
        let value = draft.read().trim().to_string();
        if value.is_empty() {
            return;
        }
        sent.write().push(value);
        draft.set(String::new());
    };

    rsx! {
        section { class: "card nora-panel",
            div { class: "nora-head",
                span { class: "nora-av", aria_hidden: "true", "N" }
                div {
                    div { class: "nora-name", "Nora" }
                    div { class: "nora-status", "Idle" }
                }
                button {
                    class: "nora-call",
                    r#type: "button",
                    aria_label: "Call Nora",
                    onclick: move |_| shell.open(Overlay::NoraCall),
                    Icon { name: IconName::Phone, size: 16 }
                }
            }
            div { class: "msgs",
                div { class: "msg-time", "Brief, 07:40" }
                for line in BRIEF.iter() {
                    p { class: "msg", "{line}" }
                }
                for (i, line) in sent.read().iter().enumerate() {
                    p { key: "me-{i}", class: "msg me", "{line}" }
                }
            }
            div { class: "composer",
                input {
                    r#type: "text",
                    placeholder: "Message Nora",
                    aria_label: "Message Nora",
                    autocomplete: "off",
                    value: "{draft}",
                    oninput: move |e| draft.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            send();
                        }
                    },
                }
                button { class: "icon-btn", r#type: "button", aria_label: "Send", onclick: move |_| send(),
                    Icon { name: IconName::Send, size: 16 }
                }
            }
        }
    }
}

#[component]
pub fn WorkPanel() -> Element {
    let running = SESSIONS
        .iter()
        .filter(|s| s.status != SessionStatus::Done)
        .count();

    rsx! {
        section { class: "work-panel",
            SectionHeader { title: "Work", meta: "{running} running" }
            div { class: "list",
                for s in SESSIONS.iter() {
                    div { key: "{s.id}", class: "sess",
                        div { class: "sess-repo", "{s.repo}" }
                        div { class: "sess-title", "{s.title}" }
                        div { class: "sess-status {s.status.class()}",
                            "{s.status.word()}"
                            span { class: "sess-since", "{s.since}" }
                        }
                    }
                }
            }
        }
    }
}
