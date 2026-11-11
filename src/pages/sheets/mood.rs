//! Mood and energy: a glowing circle that takes the colour of the mood and the
//! brightness of the energy, two sliders, then a short "why" step whose chips follow
//! the side of the scale he picked.

use dioxus::prelude::*;

use crate::components::voice::{MicButton, append_text};
use crate::components::{Icon, IconName};
use crate::data::mood::{reasons, Checkin, ENERGY, MOODS, MOOD_HUES};
use crate::data::Time;
use crate::live::Live;
use crate::state::AppState;

#[component]
pub fn MoodSheet() -> Element {
    let mut state = use_context::<AppState>();
    let mut live = use_context::<Live>();
    let last = state.checkin.peek().clone();
    let mut mood = use_signal(|| last.as_ref().map_or(3, |c| c.mood));
    let mut energy = use_signal(|| last.as_ref().map_or(2, |c| c.energy));
    let mut step = use_signal(|| 1u8);
    let mut picked = use_signal(Vec::<&'static str>::new);
    let mut note = use_signal(String::new);

    let m = mood();
    let e = energy();
    let hue = MOOD_HUES[m as usize];
    let mood_p = m as f32 / 6.0;
    let energy_p = e as f32 / 4.0;
    let mood_word = MOODS[m as usize];
    let energy_word = ENERGY[e as usize];
    let energy_lower = energy_word.to_lowercase();

    rsx! {
        div { class: "sheet-body mood", "data-step": "{step}", style: "--mood: {hue}; --mood-p: {mood_p:.3}; --energy: {energy_p:.3}",
            if step() == 1 {
                h2 { class: "sheet-title", "How are you right now?" }
                div { class: "orb-wrap", aria_hidden: "true",
                    span { class: "orb-halo" }
                    span { class: "orb" }
                }
                p { class: "mood-word", "{mood_word}" }
                div { class: "scale",
                    div { class: "scale-head",
                        span { class: "scale-name", "Mood" }
                    }
                    input {
                        class: "range range-mood",
                        r#type: "range",
                        min: "0",
                        max: "6",
                        step: "1",
                        value: "{m}",
                        aria_label: "Mood",
                        "aria-valuetext": "{mood_word}",
                        oninput: move |ev| mood.set(ev.value().parse().unwrap_or(3)),
                    }
                    div { class: "scale-ends", span { "Very low" } span { "Great" } }
                }
                div { class: "scale",
                    div { class: "scale-head",
                        span { class: "scale-name", "Energy" }
                        span { class: "scale-val", "{energy_word}" }
                    }
                    input {
                        class: "range range-energy",
                        r#type: "range",
                        min: "0",
                        max: "4",
                        step: "1",
                        value: "{e}",
                        aria_label: "Energy",
                        "aria-valuetext": "{energy_word}",
                        oninput: move |ev| energy.set(ev.value().parse().unwrap_or(2)),
                    }
                    div { class: "scale-ends", span { "Drained" } span { "Charged" } }
                }
                div { class: "sheet-actions",
                    button {
                        class: "btn btn-primary btn-wide",
                        r#type: "button",
                        onclick: move |_| {
                            picked.set(Vec::new());
                            step.set(2);
                        },
                        "Next"
                    }
                }
            } else {
                div { class: "why-head",
                    span { class: "why-orb", aria_hidden: "true" }
                    div {
                        h2 { class: "sheet-title", "What's behind it?" }
                        p { class: "sheet-sub", "{mood_word}, energy {energy_lower}. Pick any." }
                    }
                }
                div { class: "chips",
                    for r in reasons(m, e) {
                        button {
                            key: "{r}",
                            class: "chip",
                            class: if picked().contains(&r) { "on" },
                            r#type: "button",
                            aria_pressed: if picked().contains(&r) { "true" } else { "false" },
                            onclick: move |_| {
                                let mut p = picked.write();
                                if let Some(i) = p.iter().position(|x| *x == r) {
                                    p.remove(i);
                                } else {
                                    p.push(r);
                                }
                            },
                            "{r}"
                        }
                    }
                }
                div { class: "mood-why",
                    textarea {
                        rows: "2",
                        placeholder: "Or say it in your words",
                        aria_label: "Why, in your words",
                        value: "{note}",
                        oninput: move |ev| note.set(ev.value()),
                    }
                    MicButton { target: "mood-why", on_text: move |t: String| { let v = append_text(&note.peek(), &t); note.set(v) } }
                }
                div { class: "sheet-actions",
                    button {
                        class: "btn btn-glass",
                        r#type: "button",
                        onclick: move |_| step.set(1),
                        Icon { name: IconName::ChevronLeft, size: 16, stroke: 2.0 }
                        "Back"
                    }
                    button {
                        class: "btn btn-primary btn-grow",
                        r#type: "button",
                        onclick: move |_| {
                            let now = chrono::Local::now();
                            let at = Time::new(chrono::Timelike::hour(&now) as u8, chrono::Timelike::minute(&now) as u8);
                            let mut reasons: Vec<String> = picked().iter().map(|r| r.to_string()).collect();
                            let own = note.peek().trim().to_string();
                            if !own.is_empty() {
                                reasons.push(own);
                            }
                            live.log_mood((m as f64 - 3.0) / 3.0, e as i64 + 1, reasons);
                            state.checkin.set(Some(Checkin { mood: m, energy: e, reasons: picked(), at }));
                            state.close_sheet();
                            state.toast("Checked in");
                        },
                        "Save"
                    }
                }
            }
        }
    }
}
