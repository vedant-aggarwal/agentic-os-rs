//! Voice input: a mic on every field. Spec and notes in `design/VOICE-BUILD.md`.
//!
//! The recorder is `src/js/voice.js` (MediaRecorder, chunks kept in IndexedDB, chimes, upload).
//! This file is the Rust side:
//! - [`use_voice_provider`] installs the recorder, gives it the server URL and token, and turns
//!   its messages into signals and toasts.
//! - [`MicButton`] is the button. The tap itself is handled in JS (so the microphone and the chimes
//!   are allowed on iOS); the button only carries `data-aos-mic` and shows the state.
//! - [`RecoverRows`] lists unfinished and finished takes for You → Recover recordings.
//! - [`VoiceHost`] loads the stylesheet and, on desktop, the global hotkey.

use dioxus::prelude::*;
use serde_json::Value;

use crate::components::{Icon, IconName};
use crate::live::Live;
use crate::state::AppState;

const VOICE_JS: &str = include_str!("../js/voice.js");
const VOICE_CSS: Asset = asset!("/assets/voice.css");

/// Global hotkey on desktop: types into the last field you touched, from any app state.
/// Ctrl+Alt+M. Not taken by Windows, Codex (Win+Alt), Claude (Ctrl+Alt+Space) or PowerToys (Alt+Space).
#[allow(dead_code)]
pub const HOTKEY: &str = "ctrl+alt+m";

#[derive(Clone, Copy)]
pub struct VoiceCtx {
    /// Target of the field being recorded into.
    pub recording: Signal<Option<String>>,
    /// Targets whose audio is uploading or being transcribed.
    pub working: Signal<Vec<String>>,
    /// The latest transcript: (sequence, target, text).
    pub text: Signal<Option<(u64, String, String)>>,
    seq: Signal<u64>,
}

/// Put `add` after `base`, with one space between when needed.
pub fn append_text(base: &str, add: &str) -> String {
    let add = add.trim();
    if add.is_empty() {
        return base.to_string();
    }
    if base.is_empty() || base.ends_with(char::is_whitespace) {
        format!("{base}{add}")
    } else {
        format!("{base} {add}")
    }
}

pub fn use_voice_provider(mut state: AppState) -> VoiceCtx {
    let live = use_context::<Live>();
    let ctx = use_context_provider(|| VoiceCtx {
        recording: Signal::new(None),
        working: Signal::new(Vec::new()),
        text: Signal::new(None),
        seq: Signal::new(0),
    });
    // Install the recorder once and listen to it.
    use_effect(move || {
        let mut ctx = ctx;
        spawn(async move {
            let mut eval = document::eval(VOICE_JS);
            while let Ok(msg) = eval.recv::<Value>().await {
                match msg["type"].as_str().unwrap_or("") {
                    "state" => {
                        ctx.recording.set(msg["recording"].as_str().map(str::to_string));
                        let w: Vec<String> = msg["working"]
                            .as_array()
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                            .unwrap_or_default();
                        ctx.working.set(w);
                    }
                    "text" => {
                        let n = *ctx.seq.peek() + 1;
                        ctx.seq.set(n);
                        ctx.text.set(Some((
                            n,
                            msg["target"].as_str().unwrap_or("").to_string(),
                            msg["text"].as_str().unwrap_or("").to_string(),
                        )));
                    }
                    "error" => state.toast(msg["message"].as_str().unwrap_or("Voice failed").to_string()),
                    "orphans" => {
                        let n = msg["n"].as_u64().unwrap_or(0);
                        state.toast(format!("{n} unfinished recording{}. You → Recover recordings.", if n == 1 { "" } else { "s" }));
                    }
                    "recovered" => {
                        let n = msg["n"].as_u64().unwrap_or(0);
                        state.toast(format!("Recovered {n} recording{}. You → Recover recordings.", if n == 1 { "" } else { "s" }));
                    }
                    _ => {}
                }
            }
        });
    });
    // Tell the recorder where the server is and which token to use.
    use_effect(move || {
        let conn = live.conn.read().clone();
        if let Some(c) = conn {
            let cfg = serde_json::json!({ "url": c.url, "token": c.token });
            let _ = document::eval(&format!(
                "(function f(n) {{ var V = window.__aosVoice; if (V && V.config) {{ V.config({cfg}); }} else if (n < 60) {{ setTimeout(function () {{ f(n + 1); }}, 100); }} }})(0);"
            ));
        }
    });
    ctx
}

/// Loads the stylesheet and, on desktop, registers the global hotkey.
#[component]
pub fn VoiceHost() -> Element {
    #[cfg(feature = "desktop")]
    {
        let _ = dioxus::desktop::use_global_shortcut(HOTKEY, move |s| {
            if s == dioxus::desktop::HotKeyState::Pressed {
                let _ = document::eval("if (window.__aosVoice) window.__aosVoice.toggle('@focus');");
            }
        });
    }
    rsx! { document::Stylesheet { href: VOICE_CSS } }
}

/// A mic button for one field. `target` is a unique name ("capture", "nora", "task-add"); when the
/// transcript for it arrives, `on_text` gets the text. The button never submits anything itself.
#[component]
pub fn MicButton(
    target: String,
    on_text: EventHandler<String>,
    #[props(default = 20)] size: u32,
    #[props(default)] class: String,
) -> Element {
    let ctx = use_context::<VoiceCtx>();
    let mut seen = use_signal(|| 0u64);
    let mine = target.clone();
    use_effect(move || {
        if let Some((n, t, text)) = ctx.text.read().clone() {
            if t == mine && n != *seen.peek() {
                seen.set(n);
                on_text.call(text);
            }
        }
    });
    let rec = ctx.recording.read().as_deref() == Some(target.as_str());
    let busy = ctx.working.read().iter().any(|w| *w == target);
    rsx! {
        button {
            class: "mic {class}",
            class: if rec { "is-rec" },
            class: if busy { "is-busy" },
            r#type: "button",
            "data-aos-mic": "{target}",
            aria_pressed: if rec { "true" } else { "false" },
            aria_label: if rec { "Stop recording" } else { "Record voice" },
            title: if rec { "Tap to stop" } else { "Tap to dictate" },
            Icon { name: IconName::Mic, size, stroke: 2.0 }
            span { class: "mic-ring", aria_hidden: "true" }
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct TakeRow {
    id: String,
    at: f64,
    state: String,
    text: String,
    seconds: u32,
    #[serde(default)]
    unseen: bool,
}

/// You → Recover recordings. Unfinished takes stay on the device until they are transcribed.
#[component]
pub fn RecoverRows() -> Element {
    let mut state = use_context::<AppState>();
    let mut rows = use_signal(Vec::<TakeRow>::new);
    let mut login = use_signal(String::new);
    let mut confirm = use_signal(|| None::<String>);
    let mut busy = use_signal(|| None::<String>);
    let reload = move || {
        spawn(async move {
            let v = document::eval("return JSON.stringify(window.__aosVoice && window.__aosVoice.list ? await window.__aosVoice.list() : []);")
                .join::<String>()
                .await
                .unwrap_or_default();
            rows.set(serde_json::from_str(&v).unwrap_or_default());
        });
    };
    use_effect(move || {
        reload();
        spawn(async move {
            let v = document::eval("return JSON.stringify(window.__aosVoice && window.__aosVoice.status ? await window.__aosVoice.status() : {});")
                .join::<String>()
                .await
                .unwrap_or_default();
            let j: Value = serde_json::from_str(&v).unwrap_or_default();
            login.set(match j["login"].as_str() {
                Some("ok") => "Codex login ok".to_string(),
                Some(_) => j["message"].as_str().unwrap_or("Codex login needs attention").to_string(),
                None => String::new(),
            });
        });
    });
    let list = rows.read().clone();
    rsx! {
        div { class: "rc",
            if !login().is_empty() { p { class: "rc-login", "{login}" } }
            if list.is_empty() {
                p { class: "rc-empty", "No recordings on this device yet." }
            }
            for r in list {
                div { key: "{r.id}", class: "rc-row",
                    div { class: "rc-head",
                        span { class: "rc-when", "{when(r.at)}" }
                        span { class: "rc-state", "{label(&r)}" }
                    }
                    if !r.text.is_empty() { p { class: "rc-text", "{r.text}" } }
                    div { class: "rc-acts",
                        if r.state != "done" {
                            button {
                                class: "rc-btn", r#type: "button", disabled: busy().is_some(),
                                onclick: {
                                    let id = r.id.clone();
                                    move |_| {
                                        let id = id.clone();
                                        busy.set(Some(id.clone()));
                                        spawn(async move {
                                            let v = document::eval(&format!("return JSON.stringify(await window.__aosVoice.recover('{id}'));")).join::<String>().await.unwrap_or_default();
                                            let j: Value = serde_json::from_str(&v).unwrap_or_default();
                                            match j["error"].as_str() {
                                                Some(e) => state.toast(e.to_string()),
                                                None => state.toast("Transcribed. Copy it from the list."),
                                            }
                                            busy.set(None);
                                            reload();
                                        });
                                    }
                                },
                                if busy().as_deref() == Some(r.id.as_str()) { "Working…" } else { "Recover and transcribe" }
                            }
                        }
                        if !r.text.is_empty() {
                            button {
                                class: "rc-btn", r#type: "button",
                                onclick: {
                                    let id = r.id.clone();
                                    move |_| {
                                        let id = id.clone();
                                        spawn(async move {
                                            let _ = document::eval(&format!("return await window.__aosVoice.copy('{id}');")).join::<bool>().await;
                                            state.toast("Copied");
                                        });
                                    }
                                },
                                "Copy"
                            }
                        }
                        if r.state != "done" {
                            button {
                                class: "rc-btn", r#type: "button",
                                onclick: {
                                    let id = r.id.clone();
                                    move |_| {
                                        let id = id.clone();
                                        spawn(async move {
                                            let _ = document::eval(&format!("return await window.__aosVoice.download('{id}');")).join::<bool>().await;
                                        });
                                    }
                                },
                                "Save audio"
                            }
                        }
                        button {
                            class: "rc-btn rc-del", r#type: "button",
                            onclick: {
                                let id = r.id.clone();
                                move |_| {
                                    if confirm().as_deref() == Some(id.as_str()) {
                                        let id = id.clone();
                                        confirm.set(None);
                                        spawn(async move {
                                            let _ = document::eval(&format!("return await window.__aosVoice.discard('{id}');")).join::<bool>().await;
                                            reload();
                                        });
                                    } else {
                                        confirm.set(Some(id.clone()));
                                    }
                                }
                            },
                            if confirm().as_deref() == Some(r.id.as_str()) { "Tap again to delete" } else { "Delete" }
                        }
                    }
                }
            }
        }
    }
}

fn label(r: &TakeRow) -> String {
    let len = if r.seconds > 0 { format!(" · {}:{:02}", r.seconds / 60, r.seconds % 60) } else { String::new() };
    match r.state.as_str() {
        "done" if r.unseen => format!("recovered{len}"),
        "done" => format!("done{len}"),
        "stopped" => format!("not transcribed yet{len}"),
        _ => format!("interrupted{len}"),
    }
}

fn when(ms: f64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|d| d.format("%d %b %H:%M").to_string())
        .unwrap_or_default()
}
