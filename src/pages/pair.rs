//! First run (after unlock): connect this device to the Agentic OS server with a 6-digit code.
//! On the PC: `aos pair new` prints the code. The token is kept on the device and sent on every
//! request; only "Unpair" in You removes it.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};
use crate::live::{net, Live};
use crate::state::{AppState, Lock};

#[component]
pub fn PairLayer() -> Element {
    let state = use_context::<AppState>();
    let mut live = use_context::<Live>();
    let mut url = use_signal(String::new);
    let mut code = use_signal(String::new);
    let mut name = use_signal(|| match net::platform() {
        "ios" => "iPhone".to_string(),
        "android" => "Android phone".to_string(),
        "web" => "Web".to_string(),
        _ => "Desktop".to_string(),
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // Web: the server that served this page is the default.
    use_effect(move || {
        spawn(async move {
            let origin = match live.conn.peek().as_ref() {
                Some(c) => c.url.clone(),
                None => net::page_origin().await,
            };
            if url.peek().is_empty() {
                url.set(origin);
            }
        });
    });

    let show = (state.lock)() == Lock::Open
        && (live.ready)()
        && (((live.conn)().is_none() && !(live.pair_skipped)()) || (live.pair_open)());
    if !show {
        return rsx! {};
    }

    let submit = move |_| {
        let u = url.peek().trim().to_string();
        let c: String = code.peek().chars().filter(|c| c.is_ascii_digit()).collect();
        let n = name.peek().trim().to_string();
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            error.set(Some("The server URL starts with http:// or https://".into()));
            return;
        }
        if c.len() != 6 {
            error.set(Some("The code has 6 digits.".into()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            match live.pair(&u, &c, if n.is_empty() { "Device" } else { &n }).await {
                Ok(()) => {}
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    rsx! {
        div { class: "pair", role: "dialog", aria_label: "Connect to your server",
            div { class: "pair-card",
                span { class: "pair-ico", Icon { name: IconName::Shield, size: 22, stroke: 1.75 } }
                h2 { class: "pair-title", "Connect to your server" }
                p { class: "pair-lead", "On your PC, run " code { "aos pair new" } " and type the 6-digit code here. You stay signed in until you unpair." }
                label { class: "pair-label", "Server URL"
                    input {
                        class: "lt-input",
                        r#type: "url",
                        inputmode: "url",
                        autocapitalize: "off",
                        spellcheck: "false",
                        placeholder: "https://your-server.example",
                        value: "{url}",
                        oninput: move |e| url.set(e.value()),
                    }
                }
                label { class: "pair-label", "Code"
                    input {
                        class: "lt-input pair-code",
                        inputmode: "numeric",
                        autocomplete: "one-time-code",
                        maxlength: "7",
                        placeholder: "123456",
                        value: "{code}",
                        oninput: move |e| code.set(e.value()),
                    }
                }
                label { class: "pair-label", "This device"
                    input { class: "lt-input", value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                if let Some(e) = error() {
                    p { class: "pair-err", role: "alert", "{e}" }
                }
                button { class: "btn btn-primary pair-go", r#type: "button", disabled: busy(), onclick: submit,
                    if busy() { "Connecting…" } else { "Connect" }
                }
                button {
                    class: "btn btn-ghost pair-skip",
                    r#type: "button",
                    onclick: move |_| {
                        live.pair_skipped.set(true);
                        live.pair_open.set(false);
                    },
                    if (live.conn)().is_some() { "Cancel" } else { "Not now (works offline, syncs after pairing)" }
                }
            }
        }
    }
}
