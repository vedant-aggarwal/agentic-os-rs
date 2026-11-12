//! Mission tab: Visualization (boards), Mind (the library), Version I want to be, New Me, Growth.
//! Design: research/mission-control.md (private), design/specs/boards.md, design/MISSION-BUILD.md.
//!
//! Deep links for screenshots and reloads: `/mission#viz`, `#mind`, `#version`, `#newme`, `#growth`,
//! `#board=<id>`, `#board=<id>&canvas`.

mod data;
pub mod mind;
mod newme;
mod version;
mod viz;

use dioxus::prelude::*;
use serde_json::{Value, json};

use crate::live::Live;
use crate::pages::placeholder::MissionGrowth;
use crate::state::AppState;
pub use data::{Mc, Seg};

const BOARD_JS: &str = include_str!("../../js/board.js");

#[component]
pub fn Mission() -> Element {
    let live = use_context::<Live>();
    let mut mc = use_context_provider(|| Mc::new(live));
    let state = use_context::<AppState>();
    let mind = use_context_provider(|| mind::Mind::new(live));
    use_board_bridge(mc, live, state);
    mind::use_mind_bridge(mind, mc, live, state);
    use_hash(mc);
    // Opened from the capture sheet's "Open Mind".
    use_effect(move || {
        if mind::OPEN_MIND() {
            *mind::OPEN_MIND.write() = false;
            mc.seg.set(Seg::Mind);
            set_hash("mind");
        }
    });

    use_effect(move || {
        if live.conn.read().is_some() {
            spawn(async move {
                mc.load_overview().await;
                mc.load_boards().await;
            });
        }
    });

    let seg = (mc.seg)();
    let o = (mc.overview)();
    let kept = o["rules_kept_rate"].as_f64().map(|r| format!("{:.0}%", r * 100.0)).unwrap_or_else(|| "–".into());
    let asks = o["proposals_pending"].as_i64().unwrap_or(0);

    rsx! {
        main { class: "page mc-page", "data-seg": seg.slug(),
            div { class: "amb amb-quiet", aria_hidden: "true" }
            div { class: "mc-aura", aria_hidden: "true" }
            header { class: "page-head mc-head",
                h1 { class: "page-title", "Mission" }
                div { class: "mc-pulse", aria_label: "This week",
                    span { b { "{o[\"vote_days_7\"].as_i64().unwrap_or(0)}" } "/7 days voted" }
                    span { b { "{o[\"steps_done_7\"].as_i64().unwrap_or(0)}" } if o["steps_done_7"] == 1 { " step done" } else { " steps done" } }
                    span { b { "{kept}" } " rules kept" }
                }
                if asks > 0 {
                    p { class: "mc-asks", "Nora will raise {asks} change", if asks > 1 { "s" } " with you tonight." }
                }
            }
            nav { class: "mc-seg", role: "tablist",
                for (s, label) in [(Seg::Viz, "Visualization"), (Seg::Mind, "Mind"), (Seg::Version, "Version"), (Seg::NewMe, "New Me"), (Seg::Growth, "Growth")] {
                    button {
                        key: "{s.slug()}",
                        r#type: "button",
                        role: "tab",
                        class: if seg == s { "on" },
                        aria_selected: if seg == s { "true" } else { "false" },
                        onclick: move |_| {
                            mc.seg.set(s);
                            set_hash(s.slug());
                        },
                        "{label}"
                    }
                }
            }
            match seg {
                Seg::Viz => rsx! { viz::VizHome {} },
                Seg::Mind => rsx! { mind::MindHome {} },
                Seg::Version => rsx! { version::VersionView {} },
                Seg::NewMe => rsx! { newme::NewMeView {} },
                Seg::Growth => rsx! { MissionGrowth {} },
            }
        }
        if (mc.open_board)().is_some() { viz::BoardLayer {} }
        if (mc.card)().is_some() { viz::CardSheet {} }
        if (mc.adding)() { viz::AddSheet {} }
        if (mc.playing)() { viz::PlayLayer {} }
        if (mind.open)().is_some() { mind::MindDetail {} }
    }
}

pub fn set_hash(h: &str) {
    let _ = document::eval(&format!("try {{ history.replaceState(null, '', '#{h}'); }} catch (e) {{}}"));
}

/// Read the hash once on open.
fn use_hash(mut mc: Mc) {
    use_effect(move || {
        let read = document::eval("return location.hash || '';");
        spawn(async move {
            let Ok(h) = read.join::<String>().await else { return };
            let h = h.trim_start_matches('#');
            if let Some(rest) = h.strip_prefix("board=") {
                let mut parts = rest.split('&');
                let id = parts.next().unwrap_or_default().to_string();
                if parts.any(|p| p == "canvas") {
                    mc.mode.set("canvas");
                }
                mc.seg.set(Seg::Viz);
                mc.open_board.set(Some(id));
                mc.load_board().await;
            } else if let Some(s) = Seg::from_slug(h) {
                mc.seg.set(s);
            }
        });
    });
}

/// Install the canvas and upload module, hand it the server and token, and act on its messages.
fn use_board_bridge(mut mc: Mc, live: Live, mut state: AppState) {
    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(BOARD_JS);
            while let Ok(msg) = eval.recv::<Value>().await {
                match msg["type"].as_str().unwrap_or("") {
                    "open" => {
                        if let Some(id) = msg["id"].as_str() {
                            mc.card.set(Some(id.to_string()));
                        }
                    }
                    "moves" => {
                        let moves = msg["moves"].clone();
                        for m in moves.as_array().into_iter().flatten() {
                            let id = m["id"].as_str().unwrap_or_default().to_string();
                            mc.patch_item(&id, |it| {
                                for k in ["x", "y", "w", "h", "z"] {
                                    if !m[k].is_null() {
                                        it[k] = m[k].clone();
                                    }
                                }
                            });
                        }
                        let mut l = live;
                        l.write("mission_board_item_move", json!({ "moves": moves }));
                    }
                    "uploaded" => {
                        let board = msg["board"].as_str().unwrap_or_default().to_string();
                        let asset = msg["asset"]["id"].as_str().unwrap_or_default().to_string();
                        let (w, h) = (msg["w"].as_f64().unwrap_or(0.0), msg["h"].as_f64().unwrap_or(0.0));
                        let height = if w > 0.0 { (260.0 * h / w).clamp(160.0, 460.0) } else { 340.0 };
                        let id = add_card(mc, json!({ "board_id": board, "kind": "image", "asset_id": asset, "w": 260.0, "h": height }));
                        // Ask once what it is for.
                        mc.adding.set(false);
                        mc.card.set(Some(id));
                    }
                    "paste-url" => {
                        let url = msg["url"].as_str().unwrap_or_default().to_string();
                        add_from_link(mc, state, url);
                    }
                    "paste-text" => {
                        if let Some(b) = mc.open_board.peek().clone() {
                            add_card(mc, json!({ "board_id": b, "kind": "text", "caption": msg["text"] }));
                        }
                    }
                    "busy" => mc.busy.set(msg["on"].as_bool().unwrap_or(false)),
                    "error" => state.toast(msg["message"].as_str().unwrap_or("Something went wrong").to_string()),
                    _ => {}
                }
            }
        });
    });
    use_effect(move || {
        if let Some(c) = live.conn.read().clone() {
            let cfg = json!({ "url": c.url, "token": c.token });
            let _ = document::eval(&format!(
                "(function f(n) {{ var B = window.__aosBoard; if (B && B.config) {{ B.config({cfg}); }} else if (n < 60) {{ setTimeout(function () {{ f(n + 1); }}, 100); }} }})(0);"
            ));
        }
    });
}

/// Add a card with a device-made id: shown at once, saved through the outbox. Returns the id.
pub fn add_card(mut mc: Mc, mut args: Value) -> String {
    let id = mc.new_id();
    args["id"] = json!(id);
    let mut guard = mc.board.write();
    let items = guard.as_mut().filter(|b| b["board"]["id"] == args["board_id"]).and_then(|b| b["items"].as_array_mut());
    if let Some(items) = items {
        let max_x = items.iter().map(|i| i["x"].as_f64().unwrap_or(0.0) + i["w"].as_f64().unwrap_or(0.0)).fold(f64::MIN, f64::max);
        let z = items.iter().filter_map(|i| i["z"].as_i64()).max().unwrap_or(0) + 1;
        let kind = args["kind"].as_str().unwrap_or("text");
        items.push(json!({
            "id": id, "kind": kind, "caption": args["caption"], "context": args["context"], "url": args["url"],
            "asset_id": args["asset_id"], "node_kind": if kind == "board" { "page" } else { kind },
            "x": if items.is_empty() { 0.0 } else { max_x + 32.0 }, "y": 0.0,
            "w": args["w"].as_f64().unwrap_or(if kind == "image" { 260.0 } else { 240.0 }),
            "h": args["h"].as_f64().unwrap_or(if kind == "image" { 340.0 } else { 160.0 }),
            "z": z, "stage": "seed", "saving": true,
        }));
    }
    drop(guard);
    mc.write("mission_board_item_add", args);
    id
}

/// A pasted or typed link: an image link becomes an image card (the server downloads it),
/// anything else a link card.
pub fn add_from_link(mut mc: Mc, mut state: AppState, url: String) {
    let Some(board) = mc.open_board.peek().clone() else { return };
    let lower = url.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    let is_image = [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"].iter().any(|e| path.ends_with(e));
    if !is_image {
        let host = url.split("//").nth(1).and_then(|r| r.split('/').next()).unwrap_or("").trim_start_matches("www.").to_string();
        add_card(mc, json!({ "board_id": board, "kind": "link", "url": url, "caption": host, "h": 150.0 }));
        return;
    }
    dioxus::core::spawn_forever(async move {
        mc.busy.set(true);
        let mut live = mc.live();
        match live.post_json("/api/media/fetch", json!({ "url": url })).await {
            Ok(a) => {
                let id = add_card(mc, json!({ "board_id": board, "kind": "image", "asset_id": a["id"] }));
                mc.adding.set(false);
                mc.card.set(Some(id));
            }
            Err(e) => {
                state.toast(format!("Couldn't save that image: {e}"));
                add_card(mc, json!({ "board_id": board, "kind": "image", "url": url }));
            }
        }
        mc.busy.set(false);
    });
}
