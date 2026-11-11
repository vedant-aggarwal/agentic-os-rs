//! Work tab components. Page state lives in `pages::work::WorkUi` (context).

mod diff;
mod list;
mod sheets;
mod thread;

pub use diff::DiffPanel;
pub use list::ThreadList;
pub use sheets::{LaunchSheet, NewThreadSheet};
pub use thread::ThreadView;

use aos_work::types::*;
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkSheet {
    None,
    NewThread,
    Launch,
}

/// Small stroke icons used only by the Work tab (kept here so `components/icon.rs` stays as is).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WIcon {
    Plus,
    Stack,
    Back,
    Stop,
    Diff,
    Send,
    Close,
    Folder,
}

#[component]
pub fn WI(name: WIcon, #[props(default = 18)] size: u32) -> Element {
    let path = match name {
        WIcon::Plus => "M12 5v14M5 12h14",
        WIcon::Stack => "M12 3l9 5-9 5-9-5 9-5zM3 13l9 5 9-5M3 17.5l9 5 9-5",
        WIcon::Back => "M15 18l-6-6 6-6",
        WIcon::Stop => "M7 7h10v10H7z",
        WIcon::Diff => "M12 4v6M9 7h6M9 17h6M5 3h14v18H5z",
        WIcon::Send => "M5 12h13M13 6l6 6-6 6",
        WIcon::Close => "M6 6l12 12M18 6L6 18",
        WIcon::Folder => "M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z",
    };
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.8",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            path { d: path }
        }
    }
}

#[component]
pub fn StatusPill(status: ThreadStatus, #[props(default)] pending: u32) -> Element {
    let label = if pending > 0 && status != ThreadStatus::NeedsYou { "Needs you" } else { status.label() };
    let class = if pending > 0 { "needs_you" } else { status.as_str() };
    rsx! {
        span { class: "wk-pill wk-pill-{class}",
            span { class: "wk-pill-dot" }
            "{label}"
        }
    }
}

/// `now`, `4m`, `3h`, `2d`.
pub fn ago(ms: Millis) -> String {
    let now = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(ms);
    let s = (now - ms).max(0) / 1000;
    match s {
        0..=44 => "now".into(),
        45..=3599 => format!("{}m", (s + 30) / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

pub fn model_label(provider: Provider, model: &str) -> String {
    let m = provider.models().iter().find(|(id, _)| *id == model).map(|(_, l)| l.to_string());
    let m = m.unwrap_or_else(|| if model.is_empty() { "Default".into() } else { model.to_string() });
    format!("{} · {}", provider.label(), m)
}
