use dioxus::prelude::*;

use crate::components::icon::{Icon, IconName};

/// Bottom sheet on phones, centred dialog from tablet up. Tap outside to close.
#[component]
pub fn Sheet(title: String, onclose: EventHandler<()>, children: Element) -> Element {
    rsx! {
        div { class: "scrim", onclick: move |_| onclose.call(()),
            div {
                class: "sheet",
                role: "dialog",
                aria_modal: "true",
                aria_label: "{title}",
                onclick: move |e| e.stop_propagation(),
                div { class: "sheet-grab" }
                div { class: "sheet-head",
                    h2 { class: "sheet-title", "{title}" }
                    button {
                        class: "icon-btn sheet-close",
                        r#type: "button",
                        aria_label: "Close",
                        onclick: move |_| onclose.call(()),
                        Icon { name: IconName::X, size: 18 }
                    }
                }
                {children}
            }
        }
    }
}
