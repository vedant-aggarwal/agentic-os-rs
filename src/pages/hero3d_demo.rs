//! `/hero3d?level=12&private=true&focus=money`: the 3D hero alone, for screenshots and tuning.

use dioxus::prelude::*;

use crate::components::hero3d::{Goal, Hero3D};

#[component]
pub fn Hero3dDemo(level: Option<u32>, private: Option<bool>, focus: Option<String>) -> Element {
    let mut level = use_signal(|| level.unwrap_or(1).clamp(1, 30));
    let mut private_mode = use_signal(|| private.unwrap_or(false));
    let mut focused = use_signal(|| focus.as_deref().and_then(Goal::parse));
    let mut last_tap = use_signal(|| None::<Goal>);

    rsx! {
        div { style: "position:fixed;inset:0;background:#050507;color:#cfd6e4;font:13px system-ui,sans-serif;",
            Hero3D {
                level: level(),
                private_mode: private_mode(),
                focused: focused(),
                on_goal_tap: move |g| last_tap.set(Some(g)),
                on_focus_change: move |g| focused.set(g),
            }
            div { style: "position:fixed;left:12px;right:12px;bottom:12px;display:flex;flex-wrap:wrap;gap:8px;align-items:center;padding:10px 12px;border-radius:14px;background:rgba(20,22,30,.55);backdrop-filter:blur(14px);border:1px solid rgba(255,255,255,.08);",
                span { "Level {level}" }
                input {
                    r#type: "range", min: "1", max: "30", value: "{level}", style: "flex:1;min-width:120px;",
                    oninput: move |e| if let Ok(v) = e.value().parse() { level.set(v) },
                }
                button { onclick: move |_| private_mode.toggle(), if private_mode() { "Private" } else { "Safe" } }
                button { onclick: move |_| focused.set(None), "Overview" }
                for g in [Goal::Physical, Goal::Money, Goal::Mind] {
                    button { key: "{g:?}", onclick: move |_| focused.set(Some(g)), "{g.as_str()}" }
                }
                if let Some(g) = last_tap() {
                    span { style: "opacity:.7;", "tapped {g.as_str()}" }
                }
            }
        }
    }
}
