//! Growth card for Wave: urges beaten next to slips, by hour and trigger, and the night guard.
//! Private mode only; the caller leaves it out in Safe mode.

use dioxus::prelude::*;
use serde_json::{Value, json};

use crate::live::Live;
use crate::state::AppState;

fn urge_word(n: i64) -> &'static str {
    if n == 1 { "urge" } else { "urges" }
}

#[component]
pub fn WaveStats() -> Element {
    let mut live = use_context::<Live>();
    let state = use_context::<AppState>();
    let mut stats = use_signal(|| Value::Null);
    let mut settings = use_signal(|| json!({ "night_guard": false, "night_guard_at": "22:30" }));
    let wave_on = super::WAVE.read().active();

    use_effect(move || {
        let _ = wave_on; // reload when a wave starts or ends
        if live.conn.read().is_some() {
            spawn(async move {
                crate::live::net::sleep_ms(1500).await;
                if let Ok(v) = live.post_json("/api/tools/wave_stats", json!({ "days": 30 })).await {
                    stats.set(v);
                }
                if let Ok(v) = live.post_json("/api/tools/wave_settings", json!({})).await {
                    settings.set(v);
                }
            });
        }
    });
    if (state.safe)() {
        return rsx! {};
    }

    let s = stats();
    let n = |v: &Value| v.as_i64().unwrap_or(0);
    let urges = n(&s["urges"]);
    let max_h = s["by_hour"].as_array().map(|a| a.iter().map(|h| n(&h["urges"])).max().unwrap_or(0)).unwrap_or(0).max(1);
    let hours: Vec<(i64, i64, i64)> = s["by_hour"].as_array().map(|a| a.iter().map(|h| (n(&h["hour"]), n(&h["beaten"]), n(&h["slipped"]) + (n(&h["urges"]) - n(&h["beaten"]) - n(&h["slipped"])))).collect()).unwrap_or_default();
    let triggers: Vec<Value> = s["by_trigger"].as_array().cloned().unwrap_or_default();
    let guard_on = settings()["night_guard"].as_bool().unwrap_or(false);
    let guard_at = settings()["night_guard_at"].as_str().unwrap_or("22:30").to_string();

    rsx! {
        section { class: "wv-stats", aria_label: "Urges",
            h2 { class: "pg-h", "Urges" }
            if urges == 0 {
                p { class: "wv-empty", "Nothing logged in the last 30 days. Wave is in your shortcuts when you need it." }
            } else {
                div { class: "wv-pair",
                    div { class: "wv-num wv-num-good", span { class: "wv-n", "{n(&s[\"beaten\"])}" } span { class: "wv-l", "beaten" } }
                    div { class: "wv-num", span { class: "wv-n", "{n(&s[\"slipped\"])}" } span { class: "wv-l", "slipped" } }
                    div { class: "wv-num", span { class: "wv-n", "{n(&s[\"strong\"])}" } span { class: "wv-l", "still strong" } }
                }
                p { class: "wv-line",
                    "Last 30 days, {urges} urges."
                    if let Some(h) = s["peak_hour"].as_i64() { " Busiest hour {h:02}:00." }
                    if let Some(d) = s["days_since_slip"].as_i64() { " {d} days since the last slip." }
                }
                div { class: "wv-hours", role: "img", aria_label: "Urges by hour of day",
                    for (h , b , o) in hours {
                        div { key: "{h}", class: "wv-hr", title: "{h:02}:00, {b} beaten, {o} other",
                            span { class: "wv-bar",
                                span { class: "wv-bar-o", style: "height: {o * 100 / max_h}%" }
                                span { class: "wv-bar-b", style: "height: {b * 100 / max_h}%" }
                            }
                            if h % 6 == 0 { span { class: "wv-hl", "{h}" } }
                        }
                    }
                }
                if !triggers.is_empty() {
                    div { class: "wv-trig",
                        for t in triggers.iter().take(5) {
                            div { key: "{t[\"name\"]}", class: "wv-trow",
                                span { class: "wv-tn", "{t[\"name\"].as_str().unwrap_or(\"\")}" }
                                span { class: "wv-tc", "{n(&t[\"urges\"])} {urge_word(n(&t[\"urges\"]))}, {n(&t[\"beaten\"])} beaten, {n(&t[\"slipped\"])} slipped" }
                            }
                        }
                    }
                }
            }
            div { class: "wv-guard",
                label { class: "wv-toggle",
                    input {
                        r#type: "checkbox",
                        checked: guard_on,
                        onchange: move |e| {
                            let on = e.checked();
                            settings.write()["night_guard"] = json!(on);
                            spawn(async move {
                                let _ = live.post_json("/api/tools/wave_settings", json!({ "night_guard": on })).await;
                            });
                        },
                    }
                    span { class: "wv-toggle-t",
                        span { class: "wv-tn", "Night guard" }
                        span { class: "wv-tc", "Nora asks if your phone is docked outside the bed." }
                    }
                }
                if guard_on {
                    label { class: "wv-at",
                        span { "Ask at" }
                        input {
                            r#type: "time",
                            value: "{guard_at}",
                            aria_label: "Night guard time",
                            onchange: move |e| {
                                let v = e.value();
                                if v.len() >= 4 {
                                    settings.write()["night_guard_at"] = json!(v.clone());
                                    spawn(async move {
                                        let _ = live.post_json("/api/tools/wave_settings", json!({ "night_guard_at": v })).await;
                                    });
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}
