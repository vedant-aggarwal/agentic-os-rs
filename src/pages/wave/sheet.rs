//! The Wave sheet: one tap starts ten minutes, then six steps (each skippable) and the outcome.
//! All state is in [`super::WAVE`], so closing the sheet and coming back from the pill lands on
//! the same step.

use dioxus::prelude::*;
use serde_json::json;

use super::{NOW, Step, WAVE, XP, advance, call_reason, clear, clock, extend, finish, start, update};
use crate::components::voice::{MicButton, append_text};
use crate::live::Live;
use crate::state::AppState;

const PLACES: [(&str, &str); 4] = [("bed", "Bed"), ("bathroom", "Bathroom"), ("desk", "Desk"), ("elsewhere", "Elsewhere")];
const FEELINGS: [&str; 8] = ["bored", "stressed", "tired", "lonely", "anxious", "restless", "low", "frustrated"];
const TRIGGERS: [&str; 6] = ["scrolling", "late night", "nothing to do", "stress", "phone in bed", "something I saw"];

#[component]
pub fn WaveSheet() -> Element {
    let state = use_context::<AppState>();
    if (state.safe)() {
        return rsx! { div { class: "sheet-body wv" } };
    }
    let w = WAVE.read().clone();
    let left = w.left_ms(*NOW.read());
    let ended = w.active() && left == 0;
    let step = w.step;

    // At Hold the timer's end moves on by itself; elsewhere a banner offers it.
    use_effect(move || {
        let w = WAVE.read();
        if w.step == Step::Hold && w.active() && w.left_ms(*NOW.read()) == 0 {
            drop(w);
            WAVE.write().step = Step::Outcome;
        }
    });

    let guided = Step::GUIDED.iter().position(|s| *s == step);
    rsx! {
        div { class: "sheet-body wv", "data-step": "{step.slug()}",
            div { class: "wv-aura", aria_hidden: "true" }
            if step != Step::Start && w.active() {
                div { class: "wv-top",
                    div { class: "wv-dots", role: "img", aria_label: "Step {guided.map(|i| i + 1).unwrap_or(6)} of 6",
                        for (i , _s) in Step::GUIDED.iter().enumerate() {
                            span { key: "{i}", class: "wv-dot", class: if guided == Some(i) { "on" }, class: if guided.is_some_and(|g| i < g) { "past" } }
                        }
                    }
                    span { class: "wv-time", if ended { "Timer done" } else { "{clock(left)} left" } }
                }
                if ended && step != Step::Outcome && step != Step::Hold {
                    button {
                        class: "wv-banner",
                        r#type: "button",
                        onclick: move |_| WAVE.write().step = Step::Outcome,
                        "The ten minutes are up. How did it go?"
                    }
                }
            }
            {match step {
                Step::Start => rsx! { StartStep {} },
                Step::Recognize => rsx! { Recognize {} },
                Step::Pause => rsx! { Pause {} },
                Step::Breathe => rsx! { Breathe {} },
                Step::Reset => rsx! { Reset {} },
                Step::Lie => rsx! { Lie {} },
                Step::Hold => rsx! { Hold {} },
                Step::Outcome => rsx! { Outcome {} },
                Step::Beaten => rsx! { Beaten {} },
                Step::Strong => rsx! { Strong {} },
                Step::Slipped => rsx! { Slipped {} },
            }}
            if w.active() && step != Step::Start && step != Step::Outcome {
                button {
                    class: "wv-early",
                    r#type: "button",
                    onclick: move |_| WAVE.write().step = Step::Outcome,
                    "Finish early"
                }
            }
            if step == Step::Start {
                p { class: "sheet-fine", "Stays on this phone and your own server." }
            }
        }
    }
}

#[component]
fn Chip(label: String, on: bool, onclick: EventHandler<()>) -> Element {
    rsx! {
        button {
            class: "chip",
            class: if on { "on" },
            r#type: "button",
            aria_pressed: if on { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            "{label}"
        }
    }
}

/// Skip and Next under a step.
#[component]
fn Nav(next: &'static str, on_next: EventHandler<()>, on_skip: EventHandler<()>) -> Element {
    rsx! {
        div { class: "sheet-actions",
            button { class: "btn btn-glass", r#type: "button", onclick: move |_| on_skip.call(()), "Skip" }
            button { class: "btn btn-primary btn-grow", r#type: "button", onclick: move |_| on_next.call(()), "{next}" }
        }
    }
}

#[component]
fn StartStep() -> Element {
    let mut live = use_context::<Live>();
    rsx! {
        div { class: "wv-start",
            h2 { class: "sheet-title", "Wave" }
            p { class: "sheet-sub", "An urge peaks and passes. Ten minutes is enough to see it go. Every step can be skipped." }
            button {
                class: "wv-orb",
                r#type: "button",
                aria_label: "Start the ten-minute timer",
                onclick: move |_| start(&mut live),
                span { class: "wv-orb-ring", aria_hidden: "true" }
                span { class: "wv-orb-core", aria_hidden: "true" }
                span { class: "wv-orb-word", "Start" }
                span { class: "wv-orb-sub", "10 minutes" }
            }
        }
    }
}

#[component]
fn Recognize() -> Element {
    let mut live = use_context::<Live>();
    let w = WAVE.read().clone();
    let v = w.intensity;
    rsx! {
        div { class: "wv-step",
            h2 { class: "sheet-title", "Notice it" }
            p { class: "sheet-sub wv-praise", "Good job recognizing this." }
            div { class: "scale",
                div { class: "scale-head",
                    span { class: "scale-name", "How strong is it" }
                    span { class: "scale-val", "{v} / 10" }
                }
                input {
                    class: "range wv-range",
                    r#type: "range",
                    min: "0",
                    max: "10",
                    step: "1",
                    value: "{v}",
                    aria_label: "Urge intensity, 0 to 10",
                    oninput: move |e| WAVE.write().intensity = e.value().parse().unwrap_or(5),
                }
                div { class: "scale-ends", span { "Faint" } span { "Very strong" } }
            }
            h3 { class: "wv-h", "Where are you" }
            div { class: "chips wv-chips",
                for (k , label) in PLACES {
                    Chip { key: "{k}", label: label.to_string(), on: w.place.as_deref() == Some(k), onclick: move |_| WAVE.write().place = Some(k.to_string()) }
                }
            }
            h3 { class: "wv-h", "Alone" }
            div { class: "chips wv-chips",
                Chip { label: "Yes", on: w.alone == Some(true), onclick: move |_| WAVE.write().alone = Some(true) }
                Chip { label: "No", on: w.alone == Some(false), onclick: move |_| WAVE.write().alone = Some(false) }
            }
            h3 { class: "wv-h", "What are you feeling" }
            div { class: "chips wv-chips",
                for f in FEELINGS {
                    Chip {
                        key: "{f}",
                        label: f.to_string(),
                        on: w.feelings.iter().any(|x| x == f),
                        onclick: move |_| {
                            let mut g = WAVE.write();
                            if let Some(i) = g.feelings.iter().position(|x| x == f) {
                                g.feelings.remove(i);
                            } else {
                                g.feelings.push(f.to_string());
                            }
                        },
                    }
                }
            }
            h3 { class: "wv-h", "What set it off" }
            div { class: "chips wv-chips",
                for t in TRIGGERS {
                    Chip {
                        key: "{t}",
                        label: t.to_string(),
                        on: w.trigger.as_deref() == Some(t),
                        onclick: move |_| {
                            let mut g = WAVE.write();
                            g.trigger = if g.trigger.as_deref() == Some(t) { None } else { Some(t.to_string()) };
                        },
                    }
                }
            }
            Nav {
                next: "Next",
                on_next: move |_| {
                    let w = WAVE.peek().clone();
                    let mut f = json!({ "intensity": w.intensity, "feelings": w.feelings });
                    if let Some(p) = &w.place {
                        f["place"] = json!(p);
                    }
                    if let Some(a) = w.alone {
                        f["alone"] = json!(a);
                    }
                    if let Some(t) = &w.trigger {
                        f["trigger_kind"] = json!(t);
                    }
                    update(&mut live, f);
                    advance(&mut live, true);
                },
                on_skip: move |_| advance(&mut live, false),
            }
        }
    }
}

/// The wave itself: a curve that rises, peaks and falls, with a dot for where the ten minutes are.
fn curve(frac: f64) -> (String, f64, f64) {
    let y = |x: f64| 74.0 - 60.0 * (-((x - 0.38) / 0.2).powi(2)).exp();
    let pts: Vec<String> = (0..=48).map(|i| i as f64 / 48.0).map(|x| format!("{}{:.1} {:.1}", if x == 0.0 { "M" } else { "L" }, x * 280.0 + 10.0, y(x))).collect();
    let f = frac.clamp(0.0, 1.0);
    (pts.join(" "), f * 280.0 + 10.0, y(f))
}

#[component]
fn Pause() -> Element {
    let mut live = use_context::<Live>();
    let w = WAVE.read().clone();
    let total = (w.ends_ms - w.started_ms).max(1) as f64;
    let frac = (*NOW.read() - w.started_ms) as f64 / total;
    let (d, cx, cy) = curve(frac);
    rsx! {
        div { class: "wv-step wv-pause",
            h2 { class: "sheet-title", "Pause" }
            p { class: "wv-big", "This is an urge, not a command." }
            p { class: "wv-big wv-soft", "It peaks and passes." }
            svg { class: "wv-curve", view_box: "0 0 300 90", role: "img", "aria-label": "An urge rises, peaks and falls",
                path { class: "wv-curve-line", d: "{d}" }
                circle { class: "wv-curve-dot", cx: "{cx:.1}", cy: "{cy:.1}", r: "6" }
            }
            p { class: "wv-note", "You are the dot. You only have to stay on the line." }
            Nav { next: "Next", on_next: move |_| advance(&mut live, true), on_skip: move |_| advance(&mut live, false) }
        }
    }
}

const PHASES: [&str; 4] = ["Breathe in", "Hold", "Breathe out", "Hold"];
const CYCLES: u32 = 4;

#[component]
fn Breathe() -> Element {
    let mut live = use_context::<Live>();
    // Seconds since Begin; None before. Four cycles of four sides of four seconds.
    let mut t = use_signal(|| None::<u32>);
    use_future(move || async move {
        loop {
            crate::live::net::sleep_ms(1000).await;
            let cur = *t.peek();
            if let Some(n) = cur {
                if n < CYCLES * 16 {
                    t.set(Some(n + 1));
                }
            }
        }
    });
    let cur = t();
    let done = cur.is_some_and(|n| n >= CYCLES * 16);
    let (phase, word, count, cycle) = match cur {
        None => ("idle".to_string(), "Ready".to_string(), String::new(), 0),
        Some(_) if done => ("idle".to_string(), "Done".to_string(), String::new(), CYCLES),
        Some(n) => ((n % 16 / 4).to_string(), PHASES[(n % 16 / 4) as usize].to_string(), (4 - n % 4).to_string(), n / 16 + 1),
    };
    rsx! {
        div { class: "wv-step wv-breathe",
            h2 { class: "sheet-title", "Breathe" }
            p { class: "sheet-sub", "Box breathing: in 4, hold 4, out 4, hold 4. Four rounds." }
            div { class: "bx", "data-p": "{phase}", role: "img", aria_label: "Breathing guide",
                span { class: "bx-ring", aria_hidden: "true" }
                span { class: "bx-core", aria_hidden: "true" }
                span { class: "bx-word", "{word}" }
                span { class: "bx-count", "{count}" }
            }
            p { class: "wv-note", aria_live: "polite",
                if cur.is_none() { "Tap Begin when you are ready." } else if done { "Four rounds done." } else { "Round {cycle} of {CYCLES}" }
            }
            div { class: "sheet-actions",
                button { class: "btn btn-glass", r#type: "button", onclick: move |_| advance(&mut live, false), "Skip" }
                if cur.is_none() {
                    button { class: "btn btn-primary btn-grow", r#type: "button", onclick: move |_| t.set(Some(0)), "Begin" }
                } else {
                    button { class: "btn btn-primary btn-grow", r#type: "button", onclick: move |_| advance(&mut live, done), "Next" }
                }
            }
        }
    }
}

const ACTIONS: [(&str, &str, &str); 4] = [
    ("cold water on face", "Cold water on your face", "Thirty seconds at the tap. It resets your body."),
    ("20 push-ups", "20 push-ups", "Slow and clean. Feel your arms, not your head."),
    ("2-minute walk", "A 2-minute walk", "Leave the room and go outside your door and back."),
    ("call Nora", "Call Nora", "She talks you through it, knowing about this wave."),
];

#[component]
fn Reset() -> Element {
    let mut live = use_context::<Live>();
    let mut state = use_context::<AppState>();
    let w = WAVE.read().clone();
    rsx! {
        div { class: "wv-step",
            h2 { class: "sheet-title", "Reset" }
            p { class: "sheet-sub", "Pick one. Change what your body is doing." }
            div { class: "wv-acts",
                for (k , title , hint) in ACTIONS {
                    button {
                        key: "{k}",
                        class: "wv-act",
                        class: if w.action.as_deref() == Some(k) { "on" },
                        r#type: "button",
                        aria_pressed: if w.action.as_deref() == Some(k) { "true" } else { "false" },
                        onclick: move |_| {
                            WAVE.write().action = Some(k.to_string());
                            if k == "call Nora" {
                                update(&mut live, json!({ "action": k }));
                                crate::call::request_start(Some(call_reason()));
                                state.close_sheet();
                            }
                        },
                        span { class: "wv-act-t", "{title}" }
                        span { class: "wv-act-h", "{hint}" }
                    }
                }
            }
            Nav {
                next: "Done, next",
                on_next: move |_| {
                    let action = WAVE.peek().action.clone();
                    if let Some(a) = &action {
                        update(&mut live, json!({ "action": a }));
                    }
                    advance(&mut live, action.is_some());
                },
                on_skip: move |_| advance(&mut live, false),
            }
        }
    }
}

#[component]
fn Lie() -> Element {
    let mut live = use_context::<Live>();
    let w = WAVE.read().clone();
    rsx! {
        div { class: "wv-step",
            h2 { class: "sheet-title", "Spot the lie" }
            p { class: "sheet-sub", "Write the reason your brain is giving you. Then write what is true." }
            label { class: "wv-label", "What your brain says" }
            div { class: "wv-field",
                textarea {
                    rows: "2",
                    placeholder: "Type it, or tap the mic",
                    aria_label: "The reason your brain gives",
                    value: "{w.lie}",
                    oninput: move |e| WAVE.write().lie = e.value(),
                }
                MicButton { target: "wave-lie", on_text: move |t: String| { let v = append_text(&WAVE.peek().lie, &t); WAVE.write().lie = v } }
            }
            label { class: "wv-label", "What is true" }
            div { class: "wv-field",
                textarea {
                    rows: "2",
                    placeholder: "It peaks and passes. I will feel better if I wait.",
                    aria_label: "The truth",
                    value: "{w.truth}",
                    oninput: move |e| WAVE.write().truth = e.value(),
                }
                MicButton { target: "wave-truth", on_text: move |t: String| { let v = append_text(&WAVE.peek().truth, &t); WAVE.write().truth = v } }
            }
            Nav {
                next: "Next",
                on_next: move |_| {
                    let w = WAVE.peek().clone();
                    let any = !w.lie.trim().is_empty() || !w.truth.trim().is_empty();
                    if any {
                        update(&mut live, json!({ "lie": w.lie.trim(), "truth": w.truth.trim() }));
                    }
                    advance(&mut live, any);
                },
                on_skip: move |_| advance(&mut live, false),
            }
        }
    }
}

#[component]
fn Hold() -> Element {
    let mut app = use_context::<AppState>();
    let w = WAVE.read().clone();
    let left = w.left_ms(*NOW.read());
    rsx! {
        div { class: "wv-step wv-hold",
            h2 { class: "sheet-title", "Let it pass" }
            div { class: "bx bx-slow", "data-p": "idle", role: "img", aria_label: "Slow breathing circle",
                span { class: "bx-ring", aria_hidden: "true" }
                span { class: "bx-core", aria_hidden: "true" }
                span { class: "bx-clock", "{clock(left)}" }
            }
            p { class: "wv-big wv-soft", "It peaks and passes." }
            div { class: "sheet-actions",
                button {
                    class: "btn btn-glass",
                    r#type: "button",
                    onclick: move |_| {
                        crate::call::request_start(Some(call_reason()));
                        app.close_sheet();
                    },
                    "Call Nora"
                }
                button { class: "btn btn-primary btn-grow", r#type: "button", onclick: move |_| app.close_sheet(), "Keep going" }
            }
            p { class: "sheet-fine", "The timer keeps running when you close this." }
        }
    }
}

#[component]
fn Outcome() -> Element {
    let mut live = use_context::<Live>();
    rsx! {
        div { class: "wv-step",
            h2 { class: "sheet-title", "How did it go?" }
            p { class: "sheet-sub", "Any answer is useful. It goes in the log as it is." }
            div { class: "wv-outs",
                button { class: "wv-out wv-out-good", r#type: "button", onclick: move |_| finish(&mut live, "beaten", None),
                    span { class: "wv-act-t", "I beat it" }
                    span { class: "wv-act-h", "It passed and I did not act on it. +{XP} Willpower XP." }
                }
                button { class: "wv-out", r#type: "button", onclick: move |_| finish(&mut live, "strong", None),
                    span { class: "wv-act-t", "Still strong" }
                    span { class: "wv-act-h", "It is still here. That is fine." }
                }
                button { class: "wv-out", r#type: "button", onclick: move |_| finish(&mut live, "slipped", None),
                    span { class: "wv-act-t", "I slipped" }
                    span { class: "wv-act-h", "It happened. We write down the facts." }
                }
            }
        }
    }
}

#[component]
fn Beaten() -> Element {
    let mut state = use_context::<AppState>();
    let w = WAVE.read().clone();
    let did = w.action.clone().map(|a| format!(" You used {a}.")).unwrap_or_default();
    rsx! {
        div { class: "wv-step wv-end",
            div { class: "wv-seal", aria_hidden: "true" }
            h2 { class: "sheet-title", "You beat it" }
            p { class: "wv-big", "+{XP} Willpower XP" }
            p { class: "sheet-sub", "You noticed it, paused, and let it pass.{did}" }
            div { class: "sheet-actions",
                button { class: "btn btn-primary btn-wide", r#type: "button", onclick: move |_| { state.close_sheet(); }, "Done" }
            }
        }
    }
}

#[component]
fn Strong() -> Element {
    let mut live = use_context::<Live>();
    let mut state = use_context::<AppState>();
    rsx! {
        div { class: "wv-step wv-end",
            h2 { class: "sheet-title", "Still strong" }
            p { class: "sheet-sub", "That happens. The urge has not peaked out yet. You do not have to carry it alone." }
            div { class: "wv-outs",
                button {
                    class: "wv-out wv-out-good",
                    r#type: "button",
                    onclick: move |_| {
                        crate::call::request_start(Some(call_reason()));
                        state.close_sheet();
                    },
                    span { class: "wv-act-t", "Call Nora" }
                    span { class: "wv-act-h", "She helps you through the next ten minutes." }
                }
                button { class: "wv-out", r#type: "button", onclick: move |_| extend(&mut live),
                    span { class: "wv-act-t", "Another 10 minutes" }
                    span { class: "wv-act-h", "Same urge, new timer." }
                }
                button { class: "wv-out", r#type: "button", onclick: move |_| finish(&mut live, "beaten", None),
                    span { class: "wv-act-t", "It passed" }
                    span { class: "wv-act-h", "Log it as beaten." }
                }
            }
        }
    }
}

#[component]
fn Slipped() -> Element {
    let mut live = use_context::<Live>();
    let mut state = use_context::<AppState>();
    let mut note = use_signal(String::new);
    let w = WAVE.read().clone();
    // This week's numbers, after the slip reached the server.
    let week = use_resource(move || async move {
        crate::live::net::sleep_ms(1200).await;
        live.flush().await;
        live.post_json("/api/tools/wave_stats", json!({ "days": 7 })).await.ok()
    });
    let at = chrono::Local::now().format("%H:%M").to_string();
    let place = w.place.as_deref().map(|p| format!(", at the {p}")).unwrap_or_default();
    let alone = match w.alone {
        Some(true) => ", alone",
        Some(false) => ", with people around",
        None => "",
    };
    let s1 = format!("You slipped at {at}{place}{alone}, with the urge at {} out of 10.", w.intensity);
    let s2 = match week.read().as_ref().and_then(|v| v.as_ref()) {
        Some(v) => {
            let (b, s) = (v["beaten"].as_i64().unwrap_or(0), v["slipped"].as_i64().unwrap_or(0));
            format!("In the last 7 days you beat {b} and slipped {s}; the ones you beat still count.")
        }
        None => "The urges you beat before this one still count.".to_string(),
    };
    let s3 = "Nothing resets: the day is still yours, and you can start a new ten minutes right now.";
    rsx! {
        div { class: "wv-step wv-end",
            h2 { class: "sheet-title", "Logged" }
            ol { class: "wv-facts",
                li { "{s1}" }
                li { "{s2}" }
                li { "{s3}" }
            }
            label { class: "wv-label", "What was happening? (optional)" }
            div { class: "wv-field",
                textarea {
                    rows: "2",
                    placeholder: "A few words, or the mic",
                    aria_label: "What was happening",
                    value: "{note}",
                    oninput: move |e| note.set(e.value()),
                }
                MicButton { target: "wave-slip", on_text: move |t: String| { let v = append_text(&note.peek(), &t); note.set(v) } }
            }
            div { class: "sheet-actions",
                button {
                    class: "btn btn-glass",
                    r#type: "button",
                    onclick: move |_| {
                        let n = note.peek().trim().to_string();
                        if !n.is_empty() {
                            update(&mut live, json!({ "note": n }));
                        }
                        state.close_sheet();
                    },
                    "Done"
                }
                button {
                    class: "btn btn-primary btn-grow",
                    r#type: "button",
                    onclick: move |_| {
                        let n = note.peek().trim().to_string();
                        if !n.is_empty() {
                            update(&mut live, json!({ "note": n }));
                        }
                        clear();
                        start(&mut live);
                    },
                    "Restart now"
                }
            }
        }
    }
}
