//! Nora's page: her portrait in the state ring, then the merged thread (her Claude replies,
//! Grok Bot messages, system notes) streamed live from `/api/nora/events`. Messages go through
//! the outbox (`nora_send`), so one typed offline is sent when the server is back.
//! The engine switch (Auto / Claude / Grok) is icons only. Call opens the call sheet stub.

use dioxus::prelude::*;

use crate::components::voice::{append_text, MicButton};
use crate::components::{Icon, IconName};
use crate::live::{Live, NoraMsg};
use crate::state::{AppState, SheetKind};

const NORA_BUST: Asset = asset!("/assets/img/nora.webp");

const ENGINES: [(&str, &str, IconName); 3] = [
    ("auto", "Auto: Claude, may hand over to Grok", IconName::Sparkle),
    ("claude", "Claude", IconName::Message),
    ("grok", "Grok Bot", IconName::Rocket),
];

fn clock(at: i64) -> String {
    // Seconds or milliseconds since 1970, whichever the server sent.
    let secs = if at > 10_000_000_000 { at / 1000 } else { at };
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

#[component]
pub fn NoraPage() -> Element {
    let mut state = use_context::<AppState>();
    let mut live = use_context::<Live>();
    let open = (state.nora)();
    let mut draft = use_signal(String::new);

    // Fresh thread each time the page opens.
    use_effect(move || {
        if (state.nora)() {
            spawn(async move { live.load_nora().await });
            let _ = document::eval("setTimeout(() => { const s = document.querySelector('.np-scroll'); if (s) s.scrollTop = s.scrollHeight; }, 120);");
        }
    });

    let mut send = move || {
        let text = draft.peek().trim().to_string();
        if !text.is_empty() {
            live.send_nora(&text);
            draft.set(String::new());
            let _ = document::eval("setTimeout(() => { const s = document.querySelector('.np-scroll'); if (s) s.scrollTop = s.scrollHeight; }, 60);");
        }
    };

    let msgs: Vec<NoraMsg> = live.nora.read().iter().filter(|m| m.kind != "progress" || m.engine == "user").cloned().collect();
    let pending = live.nora_pending();
    let streaming = live.nora_live.read().clone();
    let engine = (live.nora_engine)();
    let paired = live.paired();
    let status = if streaming.is_some() {
        "Writing…"
    } else if !paired {
        "Not connected. Pair in You."
    } else if (live.online)() {
        "Here, free to talk"
    } else {
        "Offline. Messages wait here."
    };

    rsx! {
        section {
            class: "nora-page",
            class: if open { "open" },
            "data-panel": "nora",
            "data-edge": "1",
            "inert": if !open { "true" },
            aria_label: "Nora",
            div { class: "np-glow", aria_hidden: "true" }
            header { class: "np-top",
                button {
                    class: "icon-btn",
                    r#type: "button",
                    aria_label: "Back",
                    onclick: move |_| state.nora.set(false),
                    Icon { name: IconName::ChevronLeft, size: 22, stroke: 2.0 }
                }
                span { class: "np-top-title", "Nora" }
                button {
                    class: "icon-btn",
                    r#type: "button",
                    aria_label: "Call Nora",
                    onclick: move |_| state.open_sheet(SheetKind::Call),
                    Icon { name: IconName::Phone, size: 19, stroke: 1.75 }
                }
            }
            div { class: "np-scroll",
                div { class: "np-hero np-hero-compact",
                    div { class: "np-portrait", "data-state": if streaming.is_some() { "busy" } else { "ready" },
                        span { class: "np-ring", aria_hidden: "true" }
                        img { src: NORA_BUST, alt: "Nora" }
                    }
                    h1 { class: "np-name", "Nora" }
                    p { class: "np-state",
                        span { class: "np-dot", aria_hidden: "true" }
                        "{status}"
                    }
                }
                div { class: "np-thread",
                    if msgs.is_empty() && pending.is_empty() {
                        p { class: "np-day", "No messages yet. Say hi, or ask her to plan your day." }
                    }
                    for m in msgs {
                        div {
                            key: "{m.id}",
                            class: "msg",
                            class: if m.engine == "user" { "msg-me" } else { "msg-nora" },
                            class: if m.engine == "system" { "msg-quiet" },
                            "data-engine": "{m.engine}",
                            if m.engine == "grok" || m.source == "grok" {
                                span { class: "msg-tag", "Grok" }
                            }
                            "{m.text}"
                            span { class: "msg-at", "{clock(m.at)}" }
                        }
                    }
                    for (i, p) in pending.iter().enumerate() {
                        div { key: "p{i}", class: "msg msg-me msg-pending", "{p}"
                            span { class: "msg-at", "Waiting to send" }
                        }
                    }
                    if let Some(text) = streaming.clone() {
                        div { class: "msg msg-nora msg-live",
                            if text.is_empty() { span { class: "typing", span {} span {} span {} } } else { "{text}" }
                        }
                    }
                    if let Some(err) = (live.nora_error)() {
                        p { class: "np-day", "{err}" }
                    }
                }
            }
            div { class: "np-input",
                div { class: "np-engines", role: "radiogroup", aria_label: "Who answers",
                    for (key, label, icon) in ENGINES {
                        button {
                            key: "{key}",
                            class: "np-engine",
                            class: if engine == key { "on" },
                            r#type: "button",
                            role: "radio",
                            aria_checked: if engine == key { "true" } else { "false" },
                            aria_label: "{label}",
                            title: "{label}",
                            onclick: move |_| live.nora_engine.set(key),
                            Icon { name: icon, size: 15, stroke: 2.0 }
                        }
                    }
                }
                input {
                    class: "np-field",
                    r#type: "text",
                    placeholder: "Message Nora",
                    aria_label: "Message Nora",
                    value: "{draft}",
                    oninput: move |e| draft.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            send();
                        }
                    },
                }
                MicButton {
                    target: "nora",
                    size: 20,
                    on_text: move |t: String| { let v = append_text(&draft.peek(), &t); draft.set(v) },
                }
                crate::components::photo::PhotoButton { size: 20 }
                button {
                    class: "np-send",
                    r#type: "button",
                    aria_label: "Send",
                    onclick: move |_| send(),
                    Icon { name: IconName::Send, size: 18, stroke: 2.0 }
                }
            }
        }
    }
}
