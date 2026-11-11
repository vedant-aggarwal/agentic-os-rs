//! Unlock: a Face ID style prompt over the dark stage, and Safe mode under it.
//! Face ID opens private mode (companion shown); Safe mode opens with private things
//! hidden. Real Face ID needs the native layer; this is the screen and the flow.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::state::{AppState, Lock};

#[component]
pub fn UnlockLayer() -> Element {
    let mut state = use_context::<AppState>();
    let lock = (state.lock)();
    let name = crate::live::profile_name();
    let welcome = if name == "You" { "Welcome back".to_string() } else { format!("Welcome back, {name}") };

    let face_id = move |_: MouseEvent| {
        if *state.lock.peek() != Lock::Locked {
            return;
        }
        state.set_safe(false);
        state.lock.set(Lock::Opening);
        // The glyph draws its tick, then the stage lights up.
        spawn(async move {
            let _ = document::eval(
                "const fast = matchMedia('(prefers-reduced-motion: reduce)').matches; await new Promise(r => setTimeout(r, fast ? 80 : 720)); return 1;",
            )
            .await;
            state.lock.set(Lock::Open);
        });
    };

    rsx! {
        div {
            class: "unlock",
            "data-lock": lock.slug(),
            "inert": if lock == Lock::Open { "true" },
            div { class: "ul-center",
                button {
                    class: "faceid",
                    r#type: "button",
                    "data-motion-permission": "1",
                    aria_label: "Unlock with Face ID",
                    onclick: face_id,
                    FaceIdGlyph {}
                }
                p { class: "ul-title", if lock == Lock::Locked { "Face ID" } else { "{welcome}" } }
                p { class: "ul-sub", "Tap to unlock" }
            }
            div { class: "ul-foot",
                button {
                    class: "ul-safe",
                    r#type: "button",
                    "data-motion-permission": "1",
                    onclick: move |_| {
                        if *state.lock.peek() == Lock::Locked {
                            state.set_safe(true);
                            state.lock.set(Lock::Open);
                        }
                    },
                    Icon { name: IconName::EyeOff, size: 18, stroke: 1.75 }
                    span { "Safe mode" }
                }
                p { class: "ul-note", "Someone next to you? Safe mode opens without your companion or anything private." }
            }
        }
    }
}

#[component]
fn FaceIdGlyph() -> Element {
    rsx! {
        svg {
            class: "fi",
            view_box: "0 0 64 64",
            width: "64",
            height: "64",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "3",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            g { class: "fi-frame",
                path { d: "M4 19V12a8 8 0 0 1 8-8h7" }
                path { d: "M45 4h7a8 8 0 0 1 8 8v7" }
                path { d: "M60 45v7a8 8 0 0 1-8 8h-7" }
                path { d: "M19 60h-7a8 8 0 0 1-8-8v-7" }
            }
            g { class: "fi-face",
                path { d: "M22 23v5" }
                path { d: "M42 23v5" }
                path { d: "M32 23v12h-3" }
                path { d: "M23 43c2.6 3 5.6 4.5 9 4.5s6.4-1.5 9-4.5" }
            }
            path { class: "fi-tick", d: "M20 33.5l8.5 8.5 16-17", "pathLength": "1" }
        }
    }
}
