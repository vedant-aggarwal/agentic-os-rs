use dioxus::prelude::*;

use crate::components::icon::{Icon, IconName};
use crate::state::{Overlay, ShellState};

/// Type or speak a thought. Enter saves it to the (local, for now) inbox.
#[component]
pub fn CaptureBar() -> Element {
    let mut shell = use_context::<ShellState>();
    let mut text = use_signal(String::new);
    let seq = (shell.capture_seq)();

    let mut submit = move || {
        let value = text.read().trim().to_string();
        if value.is_empty() {
            return;
        }
        shell.capture(value);
        text.set(String::new());
    };

    rsx! {
        div { class: "capture",
            input {
                class: "capture-input",
                r#type: "text",
                placeholder: "Capture a thought",
                aria_label: "Capture a thought",
                autocomplete: "off",
                value: "{text}",
                oninput: move |e| text.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        submit();
                    }
                },
            }
            button {
                class: "icon-btn",
                r#type: "button",
                aria_label: "Speak",
                onclick: move |_| shell.open(Overlay::Placeholder("Voice capture")),
                Icon { name: IconName::Mic, size: 18 }
            }
            if seq > 0 {
                span { key: "toast-{seq}", class: "capture-toast", "Saved to inbox" }
            }
        }
    }
}
