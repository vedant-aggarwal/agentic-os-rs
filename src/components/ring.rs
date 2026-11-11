use dioxus::prelude::*;

/// Thin progress ring. `pct` is 0..=1.
#[component]
pub fn Ring(pct: f32, #[props(default = 24)] size: u32, #[props(default = 2.5)] width: f32) -> Element {
    let half = size as f32 / 2.0;
    let r = half - width / 2.0;
    let circumference = 2.0 * std::f32::consts::PI * r;
    let offset = circumference * (1.0 - pct.clamp(0.0, 1.0));

    rsx! {
        svg {
            class: "ring",
            view_box: "0 0 {size} {size}",
            width: "{size}",
            height: "{size}",
            "aria-hidden": "true",
            circle { class: "ring-track", cx: "{half}", cy: "{half}", r: "{r}", fill: "none", stroke_width: "{width}" }
            circle {
                class: "ring-fill",
                cx: "{half}",
                cy: "{half}",
                r: "{r}",
                fill: "none",
                stroke_width: "{width}",
                stroke_linecap: "round",
                stroke_dasharray: "{circumference}",
                stroke_dashoffset: "{offset}",
            }
        }
    }
}
