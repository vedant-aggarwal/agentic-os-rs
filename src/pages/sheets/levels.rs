//! Level roadmap: where he is, what the next level changes, the path to the top.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::data::levels::{milestone, next_milestone, xp_left, Look, TOP};
use crate::pages::home::hero::{look_face, FACE_COMPANION};
use crate::pages::home::topbar::thousands;
use crate::state::AppState;

const STEP: f32 = 92.0;
const PAD: f32 = 44.0;

/// Node centre: x as a share of the width, y in px. A slow wave so the path winds.
fn node(level: u8) -> (f32, f32) {
    let i = level as f32 - 1.0;
    let x = 50.0 + (i * 0.78).sin() * 27.0;
    (x, PAD + i * STEP)
}

fn path_between(from: u8, to: u8) -> String {
    let mut d = String::new();
    for l in from..=to {
        let (x, y) = node(l);
        if l == from {
            d.push_str(&format!("M{x:.2} {y:.1}"));
        } else {
            let (px, py) = node(l - 1);
            let mid = (py + y) / 2.0;
            d.push_str(&format!(" C{px:.2} {mid:.1} {x:.2} {mid:.1} {x:.2} {y:.1}"));
        }
    }
    d
}

#[component]
pub fn LevelsSheet() -> Element {
    let state = use_context::<AppState>();
    let safe = (state.safe)();
    let lv = crate::live::level();
    let xp = lv.total_xp.max(0) as u32;
    let xp_next = xp + lv.to_next.max(0) as u32;
    let pct = (lv.progress as f32).clamp(0.0, 1.0);
    let left = thousands(xp_left());
    let level = lv.level.min(TOP as u32 * 10) as u8;
    let next = next_milestone(level);
    let height = PAD * 2.0 + (TOP as f32 - 1.0) * STEP;
    let done_path = path_between(1, level);
    let todo_path = path_between(level, TOP);
    let xp_s = thousands(xp);
    let next_s = thousands(xp_next);

    rsx! {
        div { class: "sheet-body levels",
            header { class: "lv-head",
                h2 { class: "sheet-title", "Level {level}" }
                p { class: "lv-xp", "{xp_s} / {next_s} XP" }
                div { class: "lv-bar", style: "--p: {pct:.3}" }
                p { class: "lv-left", "{left} XP to level {level + 1}" }
            }
            if let Some(m) = next {
                div { class: "lv-next",
                    span { class: "lv-next-face",
                        img { src: look_face(m.look), alt: "" }
                        span { class: "lv-lock", Icon { name: IconName::Lock, size: 12, stroke: 2.25 } }
                    }
                    div { class: "lv-next-text",
                        span { class: "lv-next-k", "Next unlock, level {m.level}" }
                        span { class: "lv-next-t", "{m.title}: {m.detail}" }
                        if !safe {
                            if let Some(c) = m.companion {
                                span { class: "lv-next-c", "{c}" }
                            }
                        }
                    }
                }
            }
            div { class: "lv-path", style: "height: {height}px",
                svg {
                    class: "lv-svg",
                    view_box: "0 0 100 {height}",
                    "preserveAspectRatio": "none",
                    width: "100%",
                    height: "{height}",
                    "aria-hidden": "true",
                    path { class: "lv-todo", d: "{todo_path}" }
                    path { class: "lv-done", d: "{done_path}" }
                }
                for l in 1..=TOP {
                    LevelNode { key: "{l}", level: l, current: level, safe }
                }
            }
        }
    }
}

#[component]
fn LevelNode(level: u8, current: u8, safe: bool) -> Element {
    let (x, y) = node(level);
    let m = milestone(level);
    let state_class = if level < current {
        "done"
    } else if level == current {
        "here"
    } else {
        "ahead"
    };
    let side = if x < 50.0 { "right" } else { "left" };

    rsx! {
        div {
            class: "lv-node {state_class}",
            class: if m.is_some() { "milestone" },
            style: "left: {x:.2}%; top: {y:.1}px",
            "data-side": side,
            {if level == current {
                rsx! {
                    span { class: "lv-dot lv-me", img { src: look_face(Look::at(level)), alt: "" } }
                }
            } else if let Some(m) = m {
                rsx! {
                    span { class: "lv-dot lv-face", img { src: look_face(m.look), alt: "" } }
                }
            } else {
                rsx! {
                    span { class: "lv-dot",
                        if level < current {
                            Icon { name: IconName::Check, size: 14, stroke: 2.75 }
                        } else {
                            "{level}"
                        }
                    }
                }
            }}
            if level == current || m.is_some() {
                span { class: "lv-label",
                    span { class: "lv-label-k",
                        if level == current { "You are here, level {level}" } else { "Level {level}" }
                    }
                    if let Some(m) = m {
                        span { class: "lv-label-t", "{m.title}" }
                        if !safe && level > current {
                            if let Some(c) = m.companion {
                                span { class: "lv-label-c",
                                    img { src: FACE_COMPANION, alt: "" }
                                    "{c}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
