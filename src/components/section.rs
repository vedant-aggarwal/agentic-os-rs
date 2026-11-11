use dioxus::prelude::*;

/// Section title with an optional count on the right ("2 of 3").
#[component]
pub fn SectionHeader(title: String, meta: Option<String>) -> Element {
    rsx! {
        div { class: "sec-h",
            h2 { class: "sec-title", "{title}" }
            if let Some(meta) = meta {
                span { class: "sec-meta", "{meta}" }
            }
        }
    }
}
