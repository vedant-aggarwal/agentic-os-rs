//! The call screen, the pill and the incoming ring. Both call views read [`super::CALL`]; the
//! WebRTC session itself lives in `js/call.js`, so leaving the screen never drops the call.

use dioxus::prelude::*;

use super::{CALL, Phase, end_call, set_muted, toggle_speaker};
use crate::components::{Icon, IconName};
use crate::live::{Live, NoraMsg};
use crate::pages::home::topbar::NORA_ROUND;
use crate::state::AppState;

fn now_ms() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn timer(started: i64, now: i64) -> String {
    if started <= 0 {
        return "00:00".into();
    }
    let s = ((now - started) / 1000).max(0);
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{:02}:{:02}", s / 60, s % 60) }
}

/// A clock that ticks once a second while mounted.
fn use_clock() -> Signal<i64> {
    let mut now = use_signal(now_ms);
    use_future(move || async move {
        loop {
            crate::live::net::sleep_ms(1000).await;
            now.set(now_ms());
        }
    });
    now
}

fn status_word(mood: &str) -> &'static str {
    match mood {
        "connecting" => "Connecting",
        "listening" => "Listening",
        "thinking" => "Thinking",
        "speaking" => "Speaking",
        "muted" => "Muted",
        "ended" => "Call ended",
        _ => "Listening",
    }
}

#[component]
pub fn CallScreen() -> Element {
    let open = CALL.read().open && CALL.read().phase != Phase::Idle;
    rsx! {
        if open {
            CallScreenInner {}
        }
    }
}

#[component]
fn CallScreenInner() -> Element {
    let state = use_context::<AppState>();
    let live = use_context::<Live>();
    let now = use_clock();
    let mut pressing = use_signal(|| false);
    use_drop(|| {
        let _ = document::eval("window.aosCall && window.aosCall.unmountOrb();");
    });

    let c = CALL.read().clone();
    let mood = c.mood();
    let ended = matches!(c.phase, Phase::Ended | Phase::Failed);
    let status = if !c.status.is_empty() && c.phase == Phase::Live && c.thinking > 0 {
        c.status.clone()
    } else if !c.status.is_empty() && !matches!(mood, "listening" | "speaking") {
        c.status.clone()
    } else {
        status_word(mood).to_string()
    };
    let clock = if c.phase == Phase::Live { timer(c.started_at, now()) } else if c.phase == Phase::Connecting { "Calling…".into() } else { status_word(mood).into() };
    let close_day = c.kind == "close_day";
    let safe = (state.safe)();
    let caption = if c.caption.is_empty() && c.phase == Phase::Live { "…".to_string() } else { c.caption.clone() };
    let msgs: Vec<NoraMsg> = live.nora.read().iter().rev().filter(|m| m.kind != "ack" && m.kind != "progress").take(30).cloned().collect::<Vec<_>>().into_iter().rev().collect();

    let press_start = move |_| {
        pressing.set(true);
        spawn(async move {
            crate::live::net::sleep_ms(500).await;
            if pressing() {
                let mut w = CALL.write();
                w.reasons_open = !w.reasons_open;
            }
        });
    };

    rsx! {
        section {
            class: "call-screen",
            "data-mood": "{mood}",
            style: "--orb: {c.orb.color};",
            role: "dialog",
            aria_label: "Call with Nora",
            div { class: "cs-glow", aria_hidden: "true" }
            header { class: "cs-top",
                button {
                    class: "icon-btn cs-min",
                    r#type: "button",
                    aria_label: if ended { "Close" } else { "Leave the call screen. The call stays on" },
                    onclick: move |_| {
                        let mut w = CALL.write();
                        w.open = false;
                        if matches!(w.phase, Phase::Ended | Phase::Failed) {
                            w.phase = Phase::Idle;
                        }
                    },
                    Icon { name: IconName::ChevronLeft, size: 22, stroke: 2.0 }
                }
                div { class: "cs-who",
                    span { class: "cs-name", if close_day { "Close the day" } else { "Nora" } }
                    span { class: "cs-timer", "{clock}" }
                }
                div { class: "cs-face", "data-mood": "{mood}",
                    span { class: "cs-ring", aria_hidden: "true" }
                    img { src: NORA_ROUND, alt: "Nora" }
                }
            }
            div { class: "cs-panel",
                if !ended {
                    p { class: "cs-caption", aria_live: "polite", "{caption}" }
                    if !c.heard.is_empty() {
                        p { class: "cs-heard", "You: {c.heard}" }
                    }
                }
                div {
                    class: "cs-orb",
                    onpointerdown: press_start,
                    onpointerup: move |_| pressing.set(false),
                    onpointerleave: move |_| pressing.set(false),
                    canvas {
                        id: "call-orb",
                        aria_label: "Nora's orb. {c.orb.label}",
                        onmounted: move |_| {
                            let _ = document::eval("requestAnimationFrame(() => { const el = document.getElementById('call-orb'); if (el && window.aosCall) window.aosCall.mountOrb(el); });");
                        },
                    }
                    span { class: "cs-orb-fallback", aria_hidden: "true" }
                }
                p { class: "cs-status", "{status}" }
                if c.reasons_open && !safe {
                    div { class: "cs-reasons",
                        strong { "{c.orb.label}" }
                        for r in c.orb.reasons.iter() {
                            span { key: "{r}", "{r}" }
                        }
                    }
                }
            }
            if ended {
                WrapCard {}
            } else {
                div { class: "cs-ctrls",
                    button {
                        class: "cs-btn",
                        class: if c.muted { "on" },
                        r#type: "button",
                        aria_pressed: if c.muted { "true" } else { "false" },
                        aria_label: if c.muted { "Unmute" } else { "Mute" },
                        onclick: move |_| set_muted(!CALL.peek().muted),
                        Icon { name: if c.muted { IconName::MicOff } else { IconName::Mic }, size: 22, stroke: 1.75 }
                    }
                    button {
                        class: "cs-btn",
                        class: if !c.speaker { "on" },
                        r#type: "button",
                        aria_label: "Speaker",
                        onclick: move |_| toggle_speaker(),
                        Icon { name: IconName::Speaker, size: 22, stroke: 1.75 }
                    }
                    button {
                        class: "cs-btn",
                        class: if c.chat { "on" },
                        r#type: "button",
                        aria_label: "Chat",
                        onclick: move |_| {
                            let mut w = CALL.write();
                            w.chat = !w.chat;
                        },
                        Icon { name: IconName::Message, size: 22, stroke: 1.75 }
                    }
                    button {
                        class: "cs-btn cs-end",
                        r#type: "button",
                        aria_label: "End call",
                        onclick: move |_| end_call(),
                        Icon { name: IconName::PhoneOff, size: 24, stroke: 2.0 }
                    }
                }
            }
            div { class: "cs-chat", class: if c.chat { "open" }, "inert": if !c.chat { "true" },
                button {
                    class: "cs-chat-close",
                    r#type: "button",
                    onclick: move |_| CALL.write().chat = false,
                    "Close chat"
                }
                div { class: "cs-chat-list",
                    for m in msgs {
                        div {
                            key: "{m.id}",
                            class: "msg",
                            class: if m.engine == "user" { "msg-me" } else { "msg-nora" },
                            class: if m.engine == "system" { "msg-quiet" },
                            if m.engine != "user" && m.engine != "system" {
                                span { class: "cs-mini-orb", aria_hidden: "true" }
                            }
                            "{m.text}"
                        }
                    }
                }
            }
        }
    }
}

/// After the call: what she will write up, then the summary and action points.
#[component]
fn WrapCard() -> Element {
    let c = CALL.read().clone();
    let close = move |_| {
        let mut w = CALL.write();
        w.open = false;
        w.phase = Phase::Idle;
    };
    rsx! {
        div { class: "cs-wrap",
            if let Some(err) = c.error.clone().filter(|_| c.phase == Phase::Failed) {
                h2 { "Couldn't connect" }
                p { class: "cs-wrap-sub", "{err}" }
            } else {
                h2 { if c.kind == "close_day" { "Day closed" } else { "Call ended" } }
                match c.writeup.as_str() {
                    "running" => rsx! { p { class: "cs-wrap-sub", "Writing up what you covered…" } },
                    "done" | "failed" => rsx! {
                        p { class: "cs-wrap-sum", "{c.summary}" }
                        if !c.actions.is_empty() {
                            ul { class: "cs-wrap-actions",
                                for a in c.actions.iter() {
                                    li { key: "{a}", "{a}" }
                                }
                            }
                        }
                        p { class: "cs-wrap-sub", "Saved to Nora's thread and your journal." }
                    },
                    _ => rsx! { p { class: "cs-wrap-sub", "Nothing to write up." } },
                }
            }
            button { class: "btn btn-primary btn-wide", r#type: "button", onclick: close, "Done" }
        }
    }
}

/// The capsule left at the top when he leaves the call screen during a call.
#[component]
pub fn CallPill() -> Element {
    let show = {
        let c = CALL.read();
        c.on_call() && !c.open
    };
    rsx! {
        if show {
            CallPillInner {}
        }
    }
}

#[component]
fn CallPillInner() -> Element {
    let now = use_clock();
    let c = CALL.read().clone();
    let mood = c.mood();
    let clock = if c.phase == Phase::Live { timer(c.started_at, now()) } else { "Calling…".into() };
    rsx! {
        div { class: "call-pill", "data-mood": "{mood}", style: "--orb: {c.orb.color};",
            button {
                class: "cp-open",
                r#type: "button",
                aria_label: "Back to the call",
                onclick: move |_| CALL.write().open = true,
                span { class: "cp-orb", aria_hidden: "true" }
                span { class: "cp-name", "Nora" }
                span { class: "cp-timer", "{clock}" }
            }
            button {
                class: "cp-btn",
                class: if c.muted { "on" },
                r#type: "button",
                aria_label: if c.muted { "Unmute" } else { "Mute" },
                onclick: move |_| set_muted(!CALL.peek().muted),
                Icon { name: if c.muted { IconName::MicOff } else { IconName::Mic }, size: 16, stroke: 2.0 }
            }
            button {
                class: "cp-btn cp-end",
                r#type: "button",
                aria_label: "End call",
                onclick: move |_| end_call(),
                Icon { name: IconName::PhoneOff, size: 16, stroke: 2.0 }
            }
        }
    }
}

/// Nora ringing in the app (the 21:00 close-the-day call, a heartbeat call, a test ring).
#[component]
pub fn RingCard() -> Element {
    let live = use_context::<Live>();
    let c = CALL.read();
    let Some(ring) = c.ring.clone().filter(|_| !c.on_call()) else { return rsx! {} };
    drop(c);
    let id = ring.id;
    let title = if ring.source == "close_day" { "Time to close the day" } else { "Nora is calling" };
    rsx! {
        div { class: "ring-card", role: "alertdialog", aria_label: "{title}",
            div { class: "rc-face",
                span { class: "cs-ring", aria_hidden: "true" }
                img { src: NORA_ROUND, alt: "" }
            }
            div { class: "rc-text",
                strong { "{title}" }
                span { "{ring.reason}" }
            }
            button {
                class: "rc-btn rc-no",
                r#type: "button",
                aria_label: "Decline",
                onclick: move |_| {
                    CALL.write().ring = None;
                    if let Some(conn) = live.conn.peek().clone() {
                        spawn(async move {
                            let _ = crate::live::net::post(&conn, "/api/nora/call/decline", &serde_json::json!({ "ring_id": id })).await;
                        });
                    }
                },
                Icon { name: IconName::PhoneOff, size: 20, stroke: 2.0 }
            }
            button {
                class: "rc-btn rc-yes",
                r#type: "button",
                aria_label: "Answer",
                onclick: move |_| {
                    CALL.write().ring = None;
                    super::answer_ring(id);
                },
                Icon { name: IconName::Phone, size: 20, stroke: 2.0 }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_formats() {
        assert_eq!(timer(1000, 66_000), "01:05");
        assert_eq!(timer(0, 5000), "00:00");
        assert_eq!(timer(1, 3_726_001), "1:02:06");
    }
}
