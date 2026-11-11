//! Photo of the marked-up morning sheet (design/MORNING-PRINT-BUILD.md).
//!
//! The picker and the upload are `src/js/photo.js` (the tap must reach a real file input on iOS).
//! [`use_photo_provider`] installs it, hands it the server URL and device token, and turns its
//! messages into a busy flag and toasts. [`PhotoButton`] is the camera button.

use dioxus::prelude::*;
use serde_json::Value;

use crate::components::{Icon, IconName};
use crate::live::Live;
use crate::state::AppState;

const PHOTO_JS: &str = include_str!("../js/photo.js");

#[derive(Clone, Copy)]
pub struct PhotoCtx {
    /// A photo is being read by Nora.
    pub busy: Signal<bool>,
}

pub fn use_photo_provider(mut state: AppState) -> PhotoCtx {
    let live = use_context::<Live>();
    let ctx = use_context_provider(|| PhotoCtx { busy: Signal::new(false) });
    use_effect(move || {
        let mut busy = ctx.busy;
        let mut live = live;
        spawn(async move {
            let mut eval = document::eval(PHOTO_JS);
            while let Ok(msg) = eval.recv::<Value>().await {
                match msg["type"].as_str().unwrap_or("") {
                    "state" => busy.set(msg["busy"].as_bool().unwrap_or(false)),
                    "done" => {
                        state.toast("Nora read your sheet. The summary is in her chat.");
                        // Tasks changed on the server: refresh the day.
                        spawn(async move { live.refresh().await });
                    }
                    "error" => state.toast(msg["message"].as_str().unwrap_or("Photo failed").to_string()),
                    _ => {}
                }
            }
        });
    });
    use_effect(move || {
        if let Some(c) = live.conn.read().clone() {
            let cfg = serde_json::json!({ "url": c.url, "token": c.token });
            let _ = document::eval(&format!(
                "(function f(n) {{ var P = window.__aosPhoto; if (P && P.config) {{ P.config({cfg}); }} else if (n < 60) {{ setTimeout(function () {{ f(n + 1); }}, 100); }} }})(0);"
            ));
        }
    });
    ctx
}

/// A camera button: tap, take or pick a picture of the marked sheet, and Nora reads it.
#[component]
pub fn PhotoButton(#[props(default = 20)] size: u32, #[props(default)] class: String) -> Element {
    let ctx = use_context::<PhotoCtx>();
    let busy = (ctx.busy)();
    rsx! {
        label {
            class: "photo-btn {class}",
            class: if busy { "is-busy" },
            title: if busy { "Nora is reading your sheet" } else { "Photo of your marked sheet" },
            aria_label: "Send a photo of your marked sheet to Nora",
            input {
                r#type: "file",
                accept: "image/*",
                capture: "environment",
                "data-aos-photo": "1",
                disabled: busy,
            }
            Icon { name: IconName::Camera, size, stroke: 2.0 }
            span { class: "photo-ring", aria_hidden: "true" }
        }
    }
}
