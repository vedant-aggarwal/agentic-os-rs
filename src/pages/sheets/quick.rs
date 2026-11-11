//! The shortcut sheets: Call Nora, Wave, Capture, Print plan, Launch work.
//! Each is the real screen with the back end stubbed; nothing claims to have happened.

use dioxus::prelude::*;

use crate::components::voice::{append_text, MicButton};
use crate::components::{Icon, IconName};
use crate::data::sample::SESSIONS;
use crate::live::Live;
use crate::pages::home::topbar::NORA_ROUND;
use crate::state::AppState;

#[component]
pub fn CallSheet() -> Element {
    let mut state = use_context::<AppState>();
    let mut muted = use_signal(|| false);
    let mut speaker = use_signal(|| false);
    rsx! {
        div { class: "sheet-body call",
            div { class: "call-orb", aria_hidden: "true",
                span { class: "call-core",
                    img { src: NORA_ROUND, alt: "" }
                }
            }
            h2 { class: "sheet-title call-name", "Nora" }
            p { class: "call-status", "Not connected yet" }
            p { class: "sheet-sub call-note", "Calls will run on her voice engine with today's context. This build has the screen only." }
            div { class: "call-btns",
                button {
                    class: "call-btn",
                    class: if speaker() { "on" },
                    r#type: "button",
                    aria_pressed: if speaker() { "true" } else { "false" },
                    onclick: move |_| speaker.toggle(),
                    span { class: "call-ico", Icon { name: IconName::Speaker, size: 22, stroke: 1.75 } }
                    span { "Speaker" }
                }
                button {
                    class: "call-btn",
                    class: if muted() { "on" },
                    r#type: "button",
                    aria_pressed: if muted() { "true" } else { "false" },
                    onclick: move |_| muted.toggle(),
                    span { class: "call-ico", Icon { name: if muted() { IconName::MicOff } else { IconName::Mic }, size: 22, stroke: 1.75 } }
                    span { "Mute" }
                }
                button {
                    class: "call-btn",
                    r#type: "button",
                    onclick: move |_| {
                        state.close_sheet();
                        state.nora.set(true);
                    },
                    span { class: "call-ico", Icon { name: IconName::Message, size: 22, stroke: 1.75 } }
                    span { "Chat" }
                }
                button {
                    class: "call-btn call-end",
                    r#type: "button",
                    onclick: move |_| state.close_sheet(),
                    span { class: "call-ico", Icon { name: IconName::PhoneOff, size: 22, stroke: 2.0 } }
                    span { "End" }
                }
            }
        }
    }
}

/// Ten minutes with an urge. The circle breathes: in for four, out for six.
#[component]
pub fn WaveSheet() -> Element {
    let mut state = use_context::<AppState>();
    let mut running = use_signal(|| false);
    rsx! {
        div { class: "sheet-body wave", class: if running() { "running" },
            h2 { class: "sheet-title", "Wave" }
            p { class: "sheet-sub",
                if running() {
                    "This is a wave, not an order. It rises, peaks and passes. Breathe with the circle."
                } else {
                    "Ten minutes. Notice it, breathe, let it pass. Every step can be skipped."
                }
            }
            div { class: "breath", aria_hidden: "true",
                span { class: "breath-ring" }
                span { class: "breath-core" }
                span { class: "breath-word", if running() { "Breathe" } else { "10 min" } }
            }
            div { class: "sheet-actions",
                if running() {
                    button { class: "btn btn-glass", r#type: "button", onclick: move |_| state.close_sheet(), "I'm okay" }
                    button {
                        class: "btn btn-primary btn-grow",
                        r#type: "button",
                        onclick: move |_| {
                            state.close_sheet();
                            state.open_sheet(crate::state::SheetKind::Call);
                        },
                        "Call Nora"
                    }
                } else {
                    button { class: "btn btn-primary btn-wide", r#type: "button", onclick: move |_| running.set(true), "Start" }
                }
            }
            p { class: "sheet-fine", "Stays on this phone." }
        }
    }
}

#[component]
pub fn CaptureSheet() -> Element {
    let mut state = use_context::<AppState>();
    let mut text = use_signal(String::new);
    let (title, hint) = (state.capture_for)().unwrap_or_else(|| {
        ("Capture".to_string(), "A thought, a task, a reason. It goes to the Inbox.".to_string())
    });
    rsx! {
        div { class: "sheet-body capture",
            h2 { class: "sheet-title", "{title}" }
            p { class: "sheet-sub", "{hint}" }
            textarea {
                class: "cap-field",
                placeholder: "Type it, or tap the mic",
                aria_label: "Capture",
                rows: "3",
                value: "{text}",
                oninput: move |e| text.set(e.value()),
            }
            div { class: "cap-row",
                MicButton {
                    target: "capture",
                    class: "mic-lg",
                    size: 24,
                    on_text: move |t: String| { let v = append_text(&text.peek(), &t); text.set(v) },
                }
                crate::components::photo::PhotoButton { class: "photo-lg", size: 24 }
                span { class: "cap-hint", "Mic to speak, camera for a marked sheet" }
                button {
                    class: "btn btn-primary",
                    r#type: "button",
                    disabled: text().trim().is_empty(),
                    onclick: move |_| {
                        let t = text().trim().to_string();
                        if !t.is_empty() {
                            state.captures.write().push(t);
                            state.close_sheet();
                            state.toast("Saved to Inbox");
                        }
                    },
                    "Save"
                }
            }
        }
    }
}

/// One line per area with open tasks, for the paper preview.
fn plan_line(area: &crate::live::AreaTasks) -> Option<String> {
    let open = area.tasks.iter().filter(|t| !t.task.done()).count();
    (open > 0).then(|| format!("{} ({open})", area.area.clone().unwrap_or_else(|| "No area".into())))
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
struct MorningCfg {
    enabled: bool,
    time: String,
    printer: String,
    analysis: bool,
    news: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
struct PrinterRow {
    name: String,
    offline: bool,
    physical: bool,
}

/// The morning print: today's real list, the daily switch and time, the printer, and Print now,
/// which starts the server's `print_morning` job.
#[component]
pub fn PrintSheet() -> Element {
    let mut state = use_context::<AppState>();
    let mut live = use_context::<Live>();
    let mut cfg = use_signal(MorningCfg::default);
    let mut loaded = use_signal(|| false);
    let mut printers = use_signal(Vec::<PrinterRow>::new);
    let mut chosen = use_signal(String::new);
    let mut busy = use_signal(|| false);

    use_effect(move || {
        spawn(async move {
            if let Ok(v) = live.get_json("/api/nora/morning-settings").await {
                if let Ok(c) = serde_json::from_value::<MorningCfg>(v) {
                    cfg.set(c);
                    loaded.set(true);
                }
            }
            if let Ok(v) = live.get_json("/api/nora/printers").await {
                let rows: Vec<PrinterRow> = serde_json::from_value(v["printers"].clone()).unwrap_or_default();
                printers.set(rows.into_iter().filter(|p| p.physical).collect());
                chosen.set(v["chosen"].as_str().unwrap_or("").to_string());
            }
        });
    });

    let mut save = move |c: MorningCfg| {
        cfg.set(c.clone());
        spawn(async move {
            let body = serde_json::json!({ "enabled": c.enabled, "time": c.time, "printer": c.printer, "analysis": c.analysis, "news": c.news });
            if let Err(e) = live.post_json("/api/nora/morning-settings", body).await {
                state.toast(e);
            }
        });
    };

    let today = live.today.read().clone();
    let c = cfg();
    let paired = live.paired();
    let date_label = today
        .as_ref()
        .and_then(|t| chrono::NaiveDate::parse_from_str(&t.date, "%Y-%m-%d").ok())
        .map(|d| d.format("%A %-d %B").to_string())
        .unwrap_or_else(|| "Today".to_string());
    let lines: Vec<String> = today.as_ref().map(|t| t.areas.iter().filter_map(plan_line).collect()).unwrap_or_default();
    let habits = today.as_ref().map(|t| t.habits.iter().filter(|h| h.due_today).count()).unwrap_or(0);
    let task_count: usize = today.as_ref().map(|t| t.areas.iter().map(|a| a.tasks.iter().filter(|x| !x.task.done()).count()).sum()).unwrap_or(0);

    rsx! {
        div { class: "sheet-body print",
            h2 { class: "sheet-title", "Print plan" }
            p { class: "sheet-sub",
                if c.enabled { "Prints at {c.time} every morning: schedule, tasks, habits, messages, Nora's note on yesterday, news." } else { "The morning print is off. Print now still works." }
            }
            div { class: "paper",
                div { class: "paper-head",
                    span { class: "paper-date", "{date_label}" }
                    span { class: "paper-brand", "Agentic OS" }
                }
                p { class: "paper-h", "Tasks ({task_count})" }
                if lines.is_empty() {
                    p { class: "paper-line", "Nothing on today's list yet." }
                }
                for l in lines.iter() {
                    p { key: "{l}", class: "paper-line", span { class: "paper-box" } "{l}" }
                }
                p { class: "paper-h", "Habits" }
                p { class: "paper-line", span { class: "paper-box" } "{habits} due today" }
            }
            div { class: "pr-rows",
                button {
                    class: "pr-row",
                    r#type: "button",
                    disabled: !loaded(),
                    onclick: move |_| {
                        let mut n = cfg();
                        n.enabled = !n.enabled;
                        save(n);
                    },
                    span { class: "pf-text",
                        span { class: "pf-t", "Print every morning" }
                        span { class: "pf-s", "Off switch for the daily job" }
                    }
                    span { class: "switch", class: if c.enabled { "on" }, aria_hidden: "true", span { class: "switch-knob" } }
                }
                label { class: "pr-row",
                    span { class: "pf-text",
                        span { class: "pf-t", "Time" }
                        span { class: "pf-s", "24-hour, this PC's clock" }
                    }
                    input {
                        class: "pr-time",
                        r#type: "time",
                        value: "{c.time}",
                        disabled: !loaded(),
                        onchange: move |e| {
                            let v = e.value();
                            if v.len() >= 4 {
                                let mut n = cfg();
                                n.time = v;
                                save(n);
                            }
                        },
                    }
                }
            }
            if !printers().is_empty() {
                p { class: "paper-h pr-h", "Printer" }
                div { class: "pr-printers",
                    for p in printers() {
                        button {
                            key: "{p.name}",
                            class: "pr-chip",
                            class: if chosen() == p.name { "on" },
                            class: if p.offline { "off" },
                            r#type: "button",
                            title: if p.offline { "Looks offline right now" } else { "" },
                            onclick: {
                                let name = p.name.clone();
                                move |_| {
                                    chosen.set(name.clone());
                                    let mut n = cfg();
                                    n.printer = name.clone();
                                    save(n);
                                }
                            },
                            "{p.name}"
                        }
                    }
                }
            }
            div { class: "sheet-actions",
                button {
                    class: "btn btn-primary btn-wide",
                    r#type: "button",
                    disabled: busy() || !paired,
                    onclick: move |_| {
                        busy.set(true);
                        spawn(async move {
                            match live.post_json("/api/nora/print-morning", serde_json::json!({})).await {
                                Ok(_) => {
                                    state.close_sheet();
                                    state.toast("Making your sheet. Nora will say when it is printed.");
                                }
                                Err(e) => state.toast(e),
                            }
                            busy.set(false);
                        });
                    },
                    Icon { name: IconName::Printer, size: 17, stroke: 2.0 }
                    if busy() { "Starting…" } else { "Print now" }
                }
            }
            p { class: "pr-note", "Mark the page with a pen, photograph it, and tap the camera in Nora's chat." }
        }
    }
}

#[component]
pub fn LaunchSheet() -> Element {
    let mut state = use_context::<AppState>();
    rsx! {
        div { class: "sheet-body launch",
            h2 { class: "sheet-title", "Launch work" }
            p { class: "sheet-sub", "Start a T3 session by voice, or jump back into one." }
            button {
                class: "launch-voice",
                r#type: "button",
                onclick: move |_| state.toast("T3 bridge arrives with the Work tab"),
                span { class: "launch-mic", Icon { name: IconName::Mic, size: 22, stroke: 2.0 } }
                span { class: "launch-text",
                    span { class: "launch-t", "New session" }
                    span { class: "launch-s", "Say what to build and where" }
                }
            }
            p { class: "launch-h", "Running" }
            for s in SESSIONS.iter() {
                div { key: "{s.id}", class: "launch-row",
                    span { class: "launch-ico", Icon { name: IconName::Terminal, size: 17, stroke: 2.0 } }
                    span { class: "launch-text",
                        span { class: "launch-t", "{s.title}" }
                        span { class: "launch-s", "{s.repo}, {s.since}" }
                    }
                    span { class: "launch-status {s.status.class()}", "{s.status.word()}" }
                }
            }
        }
    }
}
