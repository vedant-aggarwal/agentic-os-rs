//! Wave (urge help) on the client: the ten-minute timer, its steps, the pill and the stats card.
//! Spec: SYSTEM.md section 7, design/WAVE-BUILD.md. Private mode only: Safe mode shows nothing.
//!
//! The running wave lives in [`WAVE`] and in localStorage, so the timer survives a restart. Every
//! change is also sent to the server (`wave_*` tools, through the offline outbox), which keeps the
//! log. Time left is computed from the end time, never counted, so a sleeping phone loses nothing.

mod sheet;
mod stats;

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::live::Live;
use crate::state::{AppState, SheetKind};
pub use sheet::WaveSheet;
pub use stats::WaveStats;

pub const MINUTES: i64 = 10;
/// Willpower XP for a beaten urge (the server awards it; this is only what the screen says).
pub const XP: i64 = 15;
const STORE_KEY: &str = "aos.wave";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum Step {
    #[default]
    Start,
    Recognize,
    Pause,
    Breathe,
    Reset,
    Lie,
    Hold,
    Outcome,
    Beaten,
    Strong,
    Slipped,
}

impl Step {
    /// The six guided steps, in order.
    pub const GUIDED: [Step; 6] = [Step::Recognize, Step::Pause, Step::Breathe, Step::Reset, Step::Lie, Step::Hold];

    pub fn next(self) -> Step {
        match self {
            Step::Recognize => Step::Pause,
            Step::Pause => Step::Breathe,
            Step::Breathe => Step::Reset,
            Step::Reset => Step::Lie,
            Step::Lie => Step::Hold,
            _ => Step::Outcome,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Step::Start => "start",
            Step::Recognize => "recognize",
            Step::Pause => "pause",
            Step::Breathe => "breathe",
            Step::Reset => "reset",
            Step::Lie => "lie",
            Step::Hold => "hold",
            Step::Outcome => "outcome",
            Step::Beaten => "beaten",
            Step::Strong => "strong",
            Step::Slipped => "slipped",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize, Default)]
pub struct WaveUi {
    pub id: Option<String>,
    pub started_ms: i64,
    pub ends_ms: i64,
    pub step: Step,
    pub intensity: u8,
    pub place: Option<String>,
    pub alone: Option<bool>,
    pub feelings: Vec<String>,
    pub trigger: Option<String>,
    pub action: Option<String>,
    pub lie: String,
    pub truth: String,
    /// Steps he went through (not skipped).
    pub steps: Vec<String>,
    /// Set once it is finished: beaten, strong or slipped.
    pub outcome: Option<String>,
}

impl WaveUi {
    /// Running, or waiting for its outcome.
    pub fn active(&self) -> bool {
        self.id.is_some() && self.outcome.is_none()
    }

    pub fn left_ms(&self, now: i64) -> i64 {
        (self.ends_ms - now).max(0)
    }
}

pub static WAVE: GlobalSignal<WaveUi> = Signal::global(WaveUi::default);
/// Wall clock in ms, ticked once a second while a wave runs.
pub static NOW: GlobalSignal<i64> = Signal::global(now_ms);
static LOADED: GlobalSignal<bool> = Signal::global(|| false);

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339()).unwrap_or_default()
}

/// `mm:ss`.
pub fn clock(ms: i64) -> String {
    let s = (ms.max(0) + 999) / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// A 16-character id made on the device (the server's alphabet).
fn new_id() -> String {
    const A: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let t = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) as u64;
    let mut x = (t ^ 0x9E37_79B9_7F4A_7C15) | 1;
    (0..16)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            A[(x % 32) as usize] as char
        })
        .collect()
}

// -------------------------------------------------------------------------------------------
// Actions

/// One tap: the timer starts now and the log row is created.
pub fn start(live: &mut Live) {
    let now = now_ms();
    let id = new_id();
    *WAVE.write() = WaveUi { id: Some(id.clone()), started_ms: now, ends_ms: now + MINUTES * 60_000, step: Step::Recognize, intensity: 5, ..Default::default() };
    *NOW.write() = now;
    live.write("wave_start", json!({ "id": id, "started_at": iso(now) }));
}

/// Save fields on the running wave.
pub fn update(live: &mut Live, fields: Value) {
    let Some(id) = WAVE.peek().id.clone() else { return };
    let mut args = fields;
    args["id"] = json!(id);
    live.write("wave_update", args);
}

/// Leave a step: log that he did it (not when skipped) and move on.
pub fn advance(live: &mut Live, done: bool) {
    let (step, steps) = {
        let w = WAVE.peek();
        let mut steps = w.steps.clone();
        if done && !steps.iter().any(|s| s == w.step.slug()) {
            steps.push(w.step.slug().to_string());
        }
        (w.step, steps)
    };
    update(live, json!({ "steps": steps }));
    let mut w = WAVE.write();
    w.steps = steps;
    w.step = step.next();
}

pub fn finish(live: &mut Live, outcome: &str, note: Option<String>) {
    let Some(id) = WAVE.peek().id.clone() else { return };
    let mut args = json!({ "id": id, "outcome": outcome });
    if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
        args["note"] = json!(n);
    }
    live.write("wave_finish", args);
    let mut w = WAVE.write();
    w.outcome = Some(outcome.to_string());
    w.step = match outcome {
        "beaten" => Step::Beaten,
        "strong" => Step::Strong,
        _ => Step::Slipped,
    };
}

/// Another ten minutes on the same urge.
pub fn extend(live: &mut Live) {
    let now = now_ms();
    let end = {
        let mut w = WAVE.write();
        w.ends_ms = now + MINUTES * 60_000;
        w.outcome = None;
        w.step = Step::Hold;
        w.ends_ms
    };
    *NOW.write() = now;
    update(live, json!({ "ends_at": iso(end) }));
}

pub fn clear() {
    *WAVE.write() = WaveUi::default();
}

/// What Nora is told when she is called from a Wave.
pub fn call_reason() -> String {
    let w = WAVE.peek();
    let mut s = format!("Wave: he is riding out an urge, {} left on the ten-minute timer.", clock(w.left_ms(now_ms())));
    if w.intensity > 0 || w.place.is_some() {
        s.push_str(&format!(" Intensity {}/10", w.intensity));
        if let Some(p) = &w.place {
            s.push_str(&format!(", at the {p}"));
        }
        match w.alone {
            Some(true) => s.push_str(", alone"),
            Some(false) => s.push_str(", not alone"),
            None => {}
        }
        s.push('.');
    }
    if !w.feelings.is_empty() {
        s.push_str(&format!(" Feeling {}.", w.feelings.join(", ")));
    }
    if let Some(t) = &w.trigger {
        s.push_str(&format!(" Set off by: {t}."));
    }
    if !w.lie.trim().is_empty() {
        s.push_str(&format!(" His brain is saying: \"{}\".", w.lie.trim()));
    }
    s
}

// -------------------------------------------------------------------------------------------
// Provider and pill

/// Restores a running wave after a restart, saves changes, ticks the clock and adopts a wave the
/// server knows about. Mount once, in the shell.
pub fn use_wave_provider(live: Live, state: AppState) {
    use_hook(move || {
        spawn(async move {
            let read = document::eval(&format!("try {{ return localStorage.getItem('{STORE_KEY}') || ''; }} catch (e) {{ return ''; }}"));
            if let Ok(v) = read.join::<String>().await {
                if let Ok(w) = serde_json::from_str::<WaveUi>(&v) {
                    if w.id.is_some() && WAVE.peek().id.is_none() {
                        *WAVE.write() = w;
                    }
                }
            }
            *LOADED.write() = true;
        });
    });
    // Save after every change, once the stored copy was read.
    use_effect(move || {
        let w = WAVE.read().clone();
        if !*LOADED.read() {
            return;
        }
        let js = if w.id.is_some() {
            format!("try {{ localStorage.setItem('{STORE_KEY}', {}); }} catch (e) {{}}", serde_json::to_string(&serde_json::to_string(&w).unwrap_or_default()).unwrap_or_default())
        } else {
            format!("try {{ localStorage.removeItem('{STORE_KEY}'); }} catch (e) {{}}")
        };
        let _ = document::eval(&js);
    });
    // A finished wave's end screen is cleared once its sheet has slid away.
    use_effect(move || {
        if (state.sheet)() == Some(SheetKind::Wave) {
            return;
        }
        if WAVE.peek().outcome.is_some() {
            spawn(async move {
                crate::live::net::sleep_ms(700).await;
                if (state.sheet.peek()).is_none_or(|k| k != SheetKind::Wave) && WAVE.peek().outcome.is_some() {
                    clear();
                }
            });
        }
    });
    // The clock.
    use_future(move || async move {
        loop {
            crate::live::net::sleep_ms(1000).await;
            if WAVE.peek().active() {
                *NOW.write() = now_ms();
            }
        }
    });
    // A wave the server has and this device doesn't (started elsewhere, or the storage was cleared).
    use_effect(move || {
        if live.conn.read().is_none() || !*LOADED.read() {
            return;
        }
        let mut live = live;
        spawn(async move {
            if WAVE.peek().id.is_some() {
                return;
            }
            let Ok(v) = live.post_json("/api/tools/wave_active", json!({})).await else { return };
            let u = &v["wave"];
            let (Some(id), Some(s), Some(e)) = (u["id"].as_str(), u["started_at"].as_str(), u["ends_at"].as_str()) else { return };
            let ms = |t: &str| chrono::DateTime::parse_from_rfc3339(t).map(|d| d.timestamp_millis()).unwrap_or(0);
            if WAVE.peek().id.is_some() {
                return;
            }
            let strs = |k: &str| -> Vec<String> { u[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
            *WAVE.write() = WaveUi {
                id: Some(id.to_string()),
                started_ms: ms(s),
                ends_ms: ms(e),
                step: if ms(e) <= now_ms() { Step::Outcome } else { Step::Hold },
                intensity: u["intensity"].as_u64().unwrap_or(5) as u8,
                place: u["place"].as_str().map(String::from),
                alone: u["alone"].as_bool(),
                feelings: strs("feelings"),
                trigger: u["trigger_kind"].as_str().map(String::from),
                action: u["action"].as_str().map(String::from),
                lie: u["lie"].as_str().unwrap_or("").to_string(),
                truth: u["truth"].as_str().unwrap_or("").to_string(),
                steps: strs("steps"),
                outcome: None,
            };
        });
    });
}

/// The small timer capsule shown on every page while a wave is active and its sheet is closed.
#[component]
pub fn WavePill() -> Element {
    let mut state = use_context::<AppState>();
    let w = WAVE.read();
    let sheet_open = (state.sheet)() == Some(SheetKind::Wave);
    if !w.active() || (state.safe)() || sheet_open {
        return rsx! {};
    }
    let left = w.left_ms(*NOW.read());
    let done = left == 0;
    rsx! {
        button {
            class: "wave-pill",
            class: if done { "is-done" },
            r#type: "button",
            aria_label: if done { "Wave timer finished. Open to say how it went".to_string() } else { format!("Wave timer, {} left. Open", clock(left)) },
            onclick: move |_| state.open_sheet(SheetKind::Wave),
            span { class: "wp-dot", aria_hidden: "true" }
            span { class: "wp-time", if done { "Done?" } else { "{clock(left)}" } }
        }
    }
}
