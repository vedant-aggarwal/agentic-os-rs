//! Nora's calls in the app (design/specs/orb-and-call.md).
//!
//! The call lives in a root-level session, not in the call screen: [`CallRoot`] opens one eval
//! with `js/call.js` (WebRTC, mic, audio element, analysers, the WebGL orb) for as long as the
//! app runs. [`CALL`] holds what both views read: the full [`screen::CallScreen`] and the
//! [`screen::CallPill`] left behind when he leaves the screen. Server events (`call_state`,
//! `call_transcript`, `call_handoff`, `orb_state`, rings) arrive through the Nora SSE stream
//! ([`on_server_event`]).

pub mod screen;

use dioxus::prelude::*;
use serde_json::Value;

use crate::live::Live;

const CALL_JS: &str = include_str!("../js/call.js");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Connecting,
    Live,
    /// Hung up; the wrap card shows until he closes it.
    Ended,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Orb {
    pub state: String,
    pub color: String,
    pub label: String,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ring {
    pub id: i64,
    pub reason: String,
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallUi {
    pub phase: Phase,
    pub call_id: Option<String>,
    pub kind: String,
    /// Milliseconds since 1970, for the timer.
    pub started_at: i64,
    pub muted: bool,
    pub speaker: bool,
    /// `none`, `you` or `nora` (from the audio levels).
    pub who: String,
    /// Hand-offs running (the orb turns into the triangle while above zero).
    pub thinking: usize,
    /// "Checking your alarms", "muted"...
    pub status: String,
    /// What she is saying now (the transcript runs a little behind the audio).
    pub caption: String,
    /// His last words.
    pub heard: String,
    /// The caption / heard line is a finished utterance: the next words start a new one.
    pub caption_done: bool,
    pub heard_done: bool,
    pub orb: Orb,
    /// The full call screen is showing (else the pill, while a call is on).
    pub open: bool,
    pub ring: Option<Ring>,
    pub summary: String,
    pub actions: Vec<String>,
    /// `none`, `running`, `done`, `failed`.
    pub writeup: String,
    pub error: Option<String>,
    /// Chat slid up over the call screen.
    pub chat: bool,
    pub reasons_open: bool,
}

impl CallUi {
    pub fn on_call(&self) -> bool {
        matches!(self.phase, Phase::Connecting | Phase::Live)
    }

    /// The orb's state word, which also drives the ring on her portrait.
    pub fn mood(&self) -> &'static str {
        match self.phase {
            Phase::Connecting => "connecting",
            Phase::Ended | Phase::Failed => "ended",
            Phase::Idle => "idle",
            Phase::Live if self.muted => "muted",
            Phase::Live if self.thinking > 0 => "thinking",
            Phase::Live => match self.who.as_str() {
                "nora" => "speaking",
                "you" => "listening",
                _ => "idle",
            },
        }
    }
}

pub static CALL: GlobalSignal<CallUi> = Signal::global(|| CallUi { orb: Orb { color: "#6DAEFF".into(), label: "Calm".into(), ..Default::default() }, speaker: true, ..Default::default() });

/// A call start asked for by a button anywhere in the app: `(reason, ring id)`. [`CallRoot`] runs it.
static START: GlobalSignal<Option<(Option<String>, Option<i64>)>> = Signal::global(|| None);

pub fn request_start(reason: Option<String>) {
    *START.write() = Some((reason, None));
}

pub fn answer_ring(id: i64) {
    *START.write() = Some((None, Some(id)));
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

pub fn end_call() {
    let _ = document::eval("window.aosCall && window.aosCall.end('hung up');");
}

pub fn set_muted(m: bool) {
    let _ = document::eval(&format!("window.aosCall && window.aosCall.setMuted({m});"));
}

pub fn toggle_speaker() {
    let _ = document::eval("window.aosCall && window.aosCall.toggleSpeaker();");
}

/// The session, the orb colour bridge and the ring watcher. Mount once, in the shell.
#[component]
pub fn CallRoot() -> Element {
    let live = use_context::<Live>();

    // Events from the JS session.
    use_hook(move || {
        spawn(async move {
            let mut eval = document::eval(CALL_JS);
            while let Ok(raw) = eval.recv::<String>().await {
                let Ok(ev) = serde_json::from_str::<Value>(&raw) else { continue };
                on_js_event(&ev);
            }
        });
    });

    // Start requests from buttons.
    use_effect(move || {
        let Some((reason, ring)) = START.read().clone() else { return };
        *START.write() = None;
        let conn = live.conn.peek().clone();
        let Some(conn) = conn else {
            let mut c = CALL.write();
            c.phase = Phase::Failed;
            c.open = true;
            c.error = Some("Not connected to your server. Pair in You first.".into());
            return;
        };
        {
            let mut c = CALL.write();
            if c.on_call() {
                c.open = true;
                return;
            }
            let orb = c.orb.clone();
            *c = CallUi { phase: Phase::Connecting, open: true, orb, speaker: true, who: "none".into(), writeup: "none".into(), status: "Connecting".into(), ..Default::default() };
        }
        let opts = format!(
            "{{base: {}, token: {}, reason: {}, ringId: {}}}",
            js_str(&conn.url),
            js_str(&conn.token),
            reason.as_deref().map(js_str).unwrap_or_else(|| "null".into()),
            ring.map(|r| r.to_string()).unwrap_or_else(|| "null".into())
        );
        let _ = document::eval(&format!("window.aosCall && window.aosCall.start({opts});"));
        // His state for the orb colour, fresh at the start of a call.
        spawn(async move {
            if let Ok(v) = crate::live::net::get(&conn, "/api/nora/orb").await {
                set_orb(&v);
            }
        });
    });

    // Colour, thinking and mute go to the orb.
    use_effect(move || {
        let c = CALL.read();
        let o = format!(
            "{{color: {}, thinking: {}, muted: {}, stress: {}}}",
            js_str(&c.orb.color),
            c.thinking > 0 && c.phase == Phase::Live,
            c.muted,
            c.orb.state == "stressed"
        );
        let _ = document::eval(&format!("window.aosCall && window.aosCall.setOrb({o});"));
    });

    // What is going on at the server right now (a ring that started before the app opened).
    use_effect(move || {
        let conn = live.conn.read().clone();
        if let Some(conn) = conn {
            spawn(async move {
                if let Ok(v) = crate::live::net::get(&conn, "/api/nora/call").await {
                    if let Some(r) = v.get("ringing").filter(|r| !r.is_null()) {
                        set_ring(r);
                    }
                    if let Some(o) = v.get("orb") {
                        set_orb(o);
                    }
                }
            });
        }
    });

    rsx! {
        screen::RingCard {}
        screen::CallScreen {}
        screen::CallPill {}
    }
}

fn set_orb(v: &Value) {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    if s("color").is_empty() {
        return;
    }
    let reasons = v["reasons"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    CALL.write().orb = Orb { state: s("state"), color: s("color"), label: s("label"), reasons };
}

fn set_ring(v: &Value) {
    let Some(id) = v["id"].as_i64() else { return };
    if CALL.peek().on_call() {
        return;
    }
    CALL.write().ring = Some(Ring { id, reason: v["reason"].as_str().unwrap_or("").into(), source: v["source"].as_str().unwrap_or("").into() });
}

fn on_js_event(ev: &Value) {
    let mut c = CALL.write();
    match ev["t"].as_str().unwrap_or("") {
        "hello" => {
            // A re-mounted root: pick up a call that is still running.
            if ev["phase"] == "live" {
                c.phase = Phase::Live;
                c.call_id = ev["callId"].as_str().map(String::from);
                c.started_at = ev["startedAt"].as_i64().unwrap_or(c.started_at);
            }
        }
        "phase" => match ev["phase"].as_str().unwrap_or("") {
            "connecting" => c.phase = Phase::Connecting,
            "live" => {
                c.phase = Phase::Live;
                c.call_id = ev["callId"].as_str().map(String::from);
                c.started_at = ev["startedAt"].as_i64().unwrap_or(0);
                c.kind = ev["kind"].as_str().unwrap_or("quick").into();
                c.status = String::new();
            }
            "ended" => {
                c.phase = Phase::Ended;
                c.thinking = 0;
                c.who = "none".into();
                c.status = "Call ended".into();
                c.open = true;
                c.chat = false;
            }
            "failed" => {
                c.phase = Phase::Failed;
                c.open = true;
                c.error = Some(ev["error"].as_str().unwrap_or("The call didn't connect.").to_string());
            }
            _ => {}
        },
        "who" => c.who = ev["who"].as_str().unwrap_or("none").into(),
        "muted" => c.muted = ev["muted"].as_bool().unwrap_or(false),
        "speaker" => {
            c.speaker = ev["speaker"].as_bool().unwrap_or(true);
            if !ev["routed"].as_bool().unwrap_or(false) {
                c.status = "This device has one speaker".into();
            }
        }
        _ => {}
    }
}

/// Call events from `/api/nora/events` (called by the Nora stream in `live`).
pub fn on_server_event(ev: &Value) {
    let t = ev["type"].as_str().unwrap_or("");
    match t {
        "call_requested" => set_ring(&ev["call"]),
        "call_missed" => {
            let id = ev["call"]["id"].as_i64();
            let mut c = CALL.write();
            if c.ring.as_ref().map(|r| Some(r.id)) == Some(id) {
                c.ring = None;
            }
        }
        "orb_state" => set_orb(&ev["orb"]),
        _ => {}
    }
    let mut c = CALL.write();
    let mine = |id: &str| c.call_id.as_deref() == Some(id);
    match t {
        "call_state" => {
            let v = &ev["call"];
            if !mine(v["id"].as_str().unwrap_or("")) {
                return;
            }
            c.thinking = v["thinking"].as_u64().unwrap_or(0) as usize;
            c.writeup = v["writeup"].as_str().unwrap_or("none").into();
            c.summary = v["summary"].as_str().unwrap_or("").into();
            c.actions = v["actions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            if v["status"] == "ended" && c.phase == Phase::Live {
                // The server hung up (quiet line, voice engine stopped).
                drop(c);
                end_call();
                return;
            }
            if c.thinking == 0 && c.phase == Phase::Live {
                c.status = String::new();
            }
        }
        "call_transcript" => {
            if !mine(ev["call_id"].as_str().unwrap_or("")) {
                return;
            }
            let text = ev["text"].as_str().unwrap_or("");
            let done = ev["done"].as_bool().unwrap_or(false);
            let c = &mut *c;
            let (field, finished) = if ev["who"] == "you" { (&mut c.heard, &mut c.heard_done) } else { (&mut c.caption, &mut c.caption_done) };
            if done {
                *field = text.to_string();
                *finished = true;
            } else {
                // A new utterance after a finished one starts fresh.
                if *finished {
                    field.clear();
                    *finished = false;
                }
                field.push_str(text);
                let n = field.chars().count();
                if n > 240 {
                    *field = field.chars().skip(n - 240).collect();
                }
            }
        }
        "call_handoff" => {
            if !mine(ev["call_id"].as_str().unwrap_or("")) {
                return;
            }
            c.status = match ev["status"].as_str().unwrap_or("") {
                "running" => "Working on it".into(),
                "tool" => tool_status(ev["text"].as_str().unwrap_or("")),
                "failed" => "That didn't work".into(),
                _ => String::new(),
            };
        }
        _ => {}
    }
}

/// "mcp__aos__alarm_set" or "alarm_set" → "Setting an alarm".
fn tool_status(title: &str) -> String {
    let name = title.rsplit("__").next().unwrap_or(title).trim();
    let name = name.split_whitespace().next().unwrap_or(name);
    let known = [
        ("alarm", "Setting an alarm"),
        ("task_add", "Adding a task"),
        ("task_done", "Ticking a task"),
        ("task", "Checking your tasks"),
        ("habit", "Checking your habits"),
        ("journal", "Writing your journal"),
        ("mood", "Logging your mood"),
        ("remember", "Saving that"),
        ("recall", "Checking her memory"),
        ("today", "Looking at today"),
        ("inbox", "Checking your inbox"),
        ("calendar", "Checking your calendar"),
    ];
    known
        .iter()
        .find(|(k, _)| name.contains(k))
        .map(|(_, s)| s.to_string())
        .unwrap_or_else(|| if name.is_empty() { "Working on it".into() } else { format!("Running {}", name.replace('_', " ")) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_status_reads_well() {
        assert_eq!(tool_status("mcp__aos__alarm_set"), "Setting an alarm");
        assert_eq!(tool_status("task_list"), "Checking your tasks");
        assert_eq!(tool_status("weird_tool"), "Running weird tool");
    }

    #[test]
    fn mood_follows_phase() {
        let mut c = CallUi { phase: Phase::Live, who: "nora".into(), ..Default::default() };
        assert_eq!(c.mood(), "speaking");
        c.thinking = 1;
        assert_eq!(c.mood(), "thinking");
        c.muted = true;
        assert_eq!(c.mood(), "muted");
    }
}
