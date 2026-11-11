//! "You": safe mode, lock, theme.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::data::levels::Look;
use crate::live;
use crate::pages::home::hero::look_face;
use crate::state::{AppState, Lock};

#[component]
pub fn ProfileSheet() -> Element {
    let mut state = use_context::<AppState>();
    let safe = (state.safe)();
    let level = live::level().level;
    let mut lv = use_context::<live::Live>();
    let conn = (lv.conn)();
    let online = (lv.online)();
    let pending = lv.outbox.read().len();
    let server_line = match &conn {
        Some(c) if (lv.auth_bad)() => format!("{} · token refused, re-pair", c.url),
        Some(c) if online => format!("{} · connected", c.url),
        Some(c) => format!("{} · offline", c.url),
        None => "Not paired".to_string(),
    };
    let device_line = match &conn {
        Some(c) => format!("{} · {} waiting to send", c.device_name, pending),
        None => format!("{pending} waiting to send"),
    };
    let name = live::profile_name();
    rsx! {
        div { class: "sheet-body profile",
            div { class: "pf-head",
                img { class: "pf-face", src: look_face(Look::at(level.min(255) as u8)), alt: "" }
                div {
                    h2 { class: "sheet-title", "{name}" }
                    p { class: "sheet-sub", "Level {level}" }
                }
            }
            div { class: "pf-list",
                button {
                    class: "pf-row",
                    r#type: "button",
                    role: "switch",
                    aria_checked: if safe { "true" } else { "false" },
                    onclick: move |_| {
                        let next = !safe;
                        state.set_safe(next);
                        state.toast(if next { "Safe mode on" } else { "Safe mode off" });
                    },
                    span { class: "pf-ico", Icon { name: IconName::EyeOff, size: 18, stroke: 1.75 } }
                    span { class: "pf-text",
                        span { class: "pf-t", "Safe mode" }
                        span { class: "pf-s", "Hides your companion and anything private" }
                    }
                    span { class: "switch", class: if safe { "on" }, aria_hidden: "true", span { class: "switch-knob" } }
                }
                button {
                    class: "pf-row",
                    r#type: "button",
                    onclick: move |_| {
                        state.close_sheet();
                        state.goal.set(None);
                        state.pillar.set(None);
                        state.nora.set(false);
                        state.lock.set(Lock::Locked);
                    },
                    span { class: "pf-ico", Icon { name: IconName::Lock, size: 18, stroke: 1.75 } }
                    span { class: "pf-text",
                        span { class: "pf-t", "Lock now" }
                        span { class: "pf-s", "Back to Face ID" }
                    }
                }
                div { class: "pf-row pf-static",
                    span { class: "pf-ico", Icon { name: IconName::Terminal, size: 18, stroke: 1.75 } }
                    span { class: "pf-text",
                        span { class: "pf-t", "Server" }
                        span { class: "pf-s", "{server_line}" }
                        span { class: "pf-s", "{device_line}" }
                    }
                }
                button {
                    class: "pf-row",
                    r#type: "button",
                    onclick: move |_| {
                        state.close_sheet();
                        lv.pair_open.set(true);
                    },
                    span { class: "pf-ico", Icon { name: IconName::Shield, size: 18, stroke: 1.75 } }
                    span { class: "pf-text",
                        span { class: "pf-t", if conn.is_some() { "Re-pair" } else { "Pair this device" } }
                        span { class: "pf-s", "Server URL and a 6-digit code from aos pair new" }
                    }
                }
                if conn.is_some() {
                    button {
                        class: "pf-row",
                        r#type: "button",
                        onclick: move |_| {
                            spawn(async move { lv.unpair().await });
                            state.toast("Unpaired. This device's token is revoked.");
                        },
                        span { class: "pf-ico", Icon { name: IconName::X, size: 18, stroke: 1.75 } }
                        span { class: "pf-text",
                            span { class: "pf-t", "Unpair this device" }
                            span { class: "pf-s", "Revokes its token on the server" }
                        }
                    }
                }
                div { class: "pf-row pf-static",
                    span { class: "pf-ico", Icon { name: IconName::Sparkle, size: 18, stroke: 1.75 } }
                    span { class: "pf-text",
                        span { class: "pf-t", "Theme" }
                        span { class: "pf-s", "Dark. Light comes later." }
                    }
                }
            }
        }
    }
}
