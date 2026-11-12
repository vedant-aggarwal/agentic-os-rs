//! Visualization: the gallery of boards, a board (gallery or infinite canvas), the card sheet
//! with the dream path, the add sheet (photos, files, paste, link, text, subpage, goal) and Play.

use dioxus::prelude::*;
use serde_json::{Value, json};

use super::data::{Mc, image_src, short_day, txt};
use super::{add_card, add_from_link, set_hash};
use crate::components::{Icon, IconName};
use crate::live::Live;
use crate::state::AppState;

const PATH: [(&str, &str, &str); 5] = [
    ("why", "Why", "Why this matters to you"),
    ("outcome", "Outcome", "What it looks like when it's real"),
    ("obstacle", "Obstacle", "What inside you gets in the way"),
    ("if_then", "If-then plan", "If ___, then I ___"),
    ("first_step", "First step", "The smallest next action"),
];

fn path_count(v: &Value) -> usize {
    ["why", "obstacle", "if_then", "first_step"].iter().filter(|k| v[**k].as_str().is_some_and(|s| !s.trim().is_empty())).count()
}

fn open_board(mut mc: Mc, id: String) {
    mc.board.set(None);
    mc.open_board.set(Some(id.clone()));
    set_hash(&format!("board={id}{}", if *mc.mode.peek() == "canvas" { "&canvas" } else { "" }));
    dioxus::core::spawn_forever(async move { mc.load_board().await });
}

// ---------------------------------------------------------------------------------------------
// Gallery of boards

#[component]
pub fn VizHome() -> Element {
    let mut mc = use_context::<Mc>();
    let live = use_context::<Live>();
    let state = use_context::<AppState>();
    let safe = (state.safe)();
    let mut naming = use_signal(|| false);
    let mut name = use_signal(String::new);
    let boards = (mc.boards)();

    let mut create = move |title: String| {
        let title = if title.trim().is_empty() { "Visualization".to_string() } else { title.trim().to_string() };
        let id = mc.new_id();
        if let Some(list) = mc.boards.write().as_mut() {
            list.push(json!({ "id": id, "title": title, "item_count": 0, "child_count": 0 }));
        }
        mc.write("mission_board_add", json!({ "id": id, "title": title }));
        naming.set(false);
        name.set(String::new());
        open_board(mc, id);
    };

    rsx! {
        section { class: "mc-viz",
            div { class: "mc-viz-head",
                div {
                    h2 { class: "mc-h", "Visualization" }
                    p { class: "mc-sub", "What you want, and the path to each. Open a board to walk in." }
                }
                button { class: "mc-ghost", r#type: "button", onclick: move |_| naming.set(true), Icon { name: IconName::Plus, size: 16 } "Board" }
            }
            if naming() {
                form { class: "mc-inline",
                    onsubmit: move |e| { e.prevent_default(); create(name.peek().clone()) },
                    input { r#type: "text", placeholder: "Board name", value: "{name}", autofocus: true, oninput: move |e| name.set(e.value()) }
                    button { class: "btn btn-primary", r#type: "submit", "Create" }
                }
            }
            match boards {
                None => rsx! { p { class: "mc-empty", if live.paired() { "Loading your boards…" } else { "Pair this device to open your boards." } } },
                Some(list) if list.is_empty() => rsx! {
                    div { class: "mc-hero-empty",
                        div { class: "mc-orbit", aria_hidden: "true", span {} span {} span {} }
                        h3 { "Start your board" }
                        p { "Add the pictures of what you want. Each one gets a why, the obstacle, an if-then plan and a first step." }
                        button { class: "btn btn-primary", r#type: "button", disabled: !live.paired(), onclick: move |_| create(String::new()), "Create my Visualization board" }
                    }
                },
                Some(list) => rsx! {
                    div { class: "mc-gallery",
                        for b in list.into_iter().filter(|b| !(safe && b["sensitive"] == 1)) {
                            BoardCover { key: "{b[\"id\"].as_str().unwrap_or_default()}", b: b.clone() }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn BoardCover(b: Value) -> Element {
    let mc = use_context::<Mc>();
    let id = txt(&b, "id");
    let src = image_src(&b);
    let n = b["item_count"].as_i64().unwrap_or(0);
    let kids = b["child_count"].as_i64().unwrap_or(0);
    let hue = b["accent"].as_str().unwrap_or("mind").to_string();
    rsx! {
        button { class: "mc-cover", r#type: "button", "data-hue": "{hue}", onclick: move |_| open_board(mc, id.clone()),
            if let Some(s) = src.clone() {
                img { class: "mc-cover-glow", src: "{s}", alt: "", aria_hidden: "true" }
                img { class: "mc-cover-img", src: "{s}", alt: "" }
            } else {
                div { class: "mc-cover-blank", aria_hidden: "true" }
            }
            div { class: "mc-cover-text",
                span { class: "mc-cover-title", "{txt(&b, \"title\")}" }
                span { class: "mc-cover-meta", "{n} card", if n != 1 { "s" } if kids > 0 { " · {kids} board" } if kids > 1 { "s" } }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// One board

#[component]
pub fn BoardLayer() -> Element {
    let mut mc = use_context::<Mc>();
    let state = use_context::<AppState>();
    let safe = (state.safe)();
    let mode = (mc.mode)();
    let view = (mc.board)();
    let open_id = (mc.open_board)().unwrap_or_default();
    let mut renaming = use_signal(|| None::<String>);

    // Hand the canvas to the JS module once it is on screen.
    let canvas_id = open_id.clone();
    use_effect(move || {
        if (mc.mode)() == "canvas" && (mc.board)().is_some() {
            let _ = document::eval(&format!("window.__aosBoard && window.__aosBoard.mount({:?});", canvas_id));
        }
    });

    let Some(view) = view else {
        return rsx! { div { class: "mc-layer", div { class: "mc-bar", BackButton {} } p { class: "mc-empty", "Opening…" } } };
    };
    let board = view["board"].clone();
    let title = txt(&board, "title");
    let crumbs = view["breadcrumb"].as_array().cloned().unwrap_or_default();
    let items: Vec<Value> = view["items"].as_array().cloned().unwrap_or_default().into_iter().filter(|i| !(safe && (i["sensitive"] == 1 || i["node_sensitive"] == 1))).collect();
    let path = view["path"].clone();
    let hue = board["accent"].as_str().unwrap_or("mind").to_string();
    let bid = open_id.clone();
    let bid2 = open_id.clone();
    let has_images = items.iter().any(|i| image_src(i).is_some());

    rsx! {
        div { class: "mc-layer", "data-hue": "{hue}", "data-mode": "{mode}",
            div { class: "mc-layer-aura", aria_hidden: "true" }
            header { class: "mc-bar",
                BackButton {}
                div { class: "mc-bar-title",
                    if !crumbs.is_empty() {
                        span { class: "mc-crumbs",
                            for c in crumbs.iter() {
                                button { key: "{txt(c, \"id\")}", r#type: "button", onclick: { let id = txt(c, "id"); move |_| open_board(mc, id.clone()) }, "{txt(c, \"title\")}" }
                                span { aria_hidden: "true", " / " }
                            }
                        }
                    }
                    if let Some(r) = renaming() {
                        form { class: "mc-rename",
                            onsubmit: move |e| {
                                e.prevent_default();
                                let t = renaming.peek().clone().unwrap_or_default();
                                if !t.trim().is_empty() {
                                    if let Some(b) = mc.board.write().as_mut() { b["board"]["title"] = json!(t.trim()); }
                                    mc.write("mission_board_update", json!({ "id": bid2.clone(), "title": t.trim() }));
                                }
                                renaming.set(None);
                            },
                            input { r#type: "text", value: "{r}", autofocus: true, oninput: move |e| renaming.set(Some(e.value())) }
                        }
                    } else {
                        h2 { class: "mc-board-title", onclick: { let t = title.clone(); move |_| renaming.set(Some(t.clone())) }, "{title}" }
                    }
                }
                div { class: "mc-bar-tools",
                    div { class: "mc-toggle", role: "group", aria_label: "View",
                        button { r#type: "button", class: if mode == "gallery" { "on" }, onclick: { let b = bid.clone(); move |_| { mc.mode.set("gallery"); set_hash(&format!("board={b}")); } }, "Gallery" }
                        button { r#type: "button", class: if mode == "canvas" { "on" }, onclick: { let b = bid.clone(); move |_| { mc.mode.set("canvas"); set_hash(&format!("board={b}&canvas")); } }, "Canvas" }
                    }
                    if has_images {
                        button { class: "mc-icon-btn", r#type: "button", aria_label: "Play", title: "Play", onclick: move |_| mc.playing.set(true), Icon { name: IconName::Sparkle, size: 18 } }
                    }
                    button { class: "mc-add-btn", r#type: "button", aria_label: "Add", onclick: move |_| mc.adding.set(true),
                        if (mc.busy)() { span { class: "mc-spin" } } else { Icon { name: IconName::Plus, size: 18, stroke: 2.2 } }
                        span { "Add" }
                    }
                }
            }
            if !path.is_null() && !crumbs.is_empty() {
                PathStrip { path: path.clone() }
            }
            if items.is_empty() {
                div { class: "mc-board-empty",
                    p { "Empty board. Add a photo, paste an image or link, or write what you want." }
                    button { class: "btn btn-primary", r#type: "button", onclick: move |_| mc.adding.set(true), "Add the first card" }
                }
            } else if mode == "canvas" {
                div { class: "mc-viewport", "data-board": "{open_id}",
                    div { class: "mc-world",
                        for it in items.iter() {
                            CanvasItem { key: "{txt(it, \"id\")}", it: it.clone() }
                        }
                    }
                    div { class: "mc-canvas-tools",
                        button { r#type: "button", aria_label: "Zoom out", onclick: move |_| { let _ = document::eval("window.__aosBoard.zoom(1/1.25)"); }, "−" }
                        button { r#type: "button", aria_label: "Fit all", onclick: move |_| { let _ = document::eval("window.__aosBoard.fit()"); }, "Fit" }
                        button { r#type: "button", aria_label: "Zoom in", onclick: move |_| { let _ = document::eval("window.__aosBoard.zoom(1.25)"); }, "+" }
                    }
                    p { class: "mc-canvas-hint", "Drag to move · pinch or Ctrl+scroll to zoom · paste to add" }
                }
            } else {
                div { class: "mc-masonry",
                    for it in items.iter() {
                        CardTile { key: "{txt(it, \"id\")}", it: it.clone() }
                    }
                }
            }
        }
    }
}

#[component]
fn BackButton() -> Element {
    let mut mc = use_context::<Mc>();
    rsx! {
        button { class: "mc-icon-btn mc-back", r#type: "button", aria_label: "Back",
            onclick: move |_| {
                let parent = mc.board.peek().as_ref().and_then(|b| b["board"]["parent_board_id"].as_str().map(String::from));
                match parent {
                    Some(p) => open_board(mc, p),
                    None => {
                        let _ = document::eval("window.__aosBoard && window.__aosBoard.unmount()");
                        mc.open_board.set(None);
                        mc.board.set(None);
                        set_hash("viz");
                        dioxus::core::spawn_forever(async move { mc.load_boards().await });
                    }
                }
            },
            Icon { name: IconName::ChevronLeft, size: 20, stroke: 2.0 }
        }
    }
}

#[component]
fn PathStrip(path: Value) -> Element {
    let n = path_count(&path);
    rsx! {
        div { class: "mc-path-strip", "data-stage": "{txt(&path, \"stage\")}",
            span { class: "mc-stage", "{txt(&path, \"stage\")}" }
            for (k, label, _) in PATH.iter().filter(|(k, _, _)| *k != "outcome") {
                if let Some(v) = path[*k].as_str().filter(|s| !s.trim().is_empty()) {
                    span { key: "{k}", class: "mc-path-bit", b { "{label}" } " {v}" }
                }
            }
            if n < 4 { span { class: "mc-path-bit mc-muted", "Path {n}/4" } }
        }
    }
}

/// The face of a card, shared by the gallery and the canvas.
#[component]
fn CardFace(it: Value) -> Element {
    let kind = txt(&it, "kind");
    let caption = txt(&it, "caption");
    let context = txt(&it, "context");
    let stage = it["stage"].as_str().unwrap_or("").to_string();
    let n = path_count(&it);
    let src = image_src(&it);
    rsx! {
        match kind.as_str() {
            "image" => rsx! {
                if let Some(s) = src {
                    img { class: "mc-card-img", src: "{s}", alt: "{caption}", draggable: "false", loading: "lazy" }
                } else {
                    div { class: "mc-card-img mc-card-img-missing" }
                }
                if !caption.is_empty() || !context.is_empty() {
                    div { class: "mc-card-cap",
                        if !caption.is_empty() { span { class: "mc-cap", "{caption}" } }
                        if !context.is_empty() { span { class: "mc-ctx", "{context}" } }
                    }
                }
            },
            "board" => rsx! {
                div { class: "mc-sub-stack", aria_hidden: "true",
                    if let Some(s) = src { img { src: "{s}", alt: "", draggable: "false" } }
                }
                div { class: "mc-card-cap",
                    span { class: "mc-cap", "{caption}" }
                    span { class: "mc-ctx", "{it[\"child_items\"].as_i64().unwrap_or(0)} cards · open" }
                }
            },
            "link" => rsx! {
                div { class: "mc-link",
                    span { class: "mc-link-host", "{caption}" }
                    span { class: "mc-link-url", "{txt(&it, \"url\")}" }
                    if !context.is_empty() { span { class: "mc-ctx", "{context}" } }
                }
            },
            "goal" => rsx! {
                div { class: "mc-goal-chip", "data-hue": "{super::data::hue_of(it[\"goal_section\"].as_str().unwrap_or(\"\"))}",
                    span { class: "mc-goal-h", "{txt(&it, \"goal_horizon\")} term goal" }
                    span { class: "mc-cap", "{caption}" }
                    span { class: "mc-ctx", "{txt(&it, \"goal_status\")}" }
                }
            },
            _ => rsx! {
                div { class: "mc-note",
                    p { "{caption}" }
                    if !context.is_empty() { span { class: "mc-ctx", "{context}" } }
                }
            },
        }
        if kind != "goal" && !stage.is_empty() {
            span { class: "mc-stage-dot", "data-stage": "{stage}", title: "{stage}",
                if stage == "seed" { "seed · path {n}/4" } else { "{stage}" }
            }
        }
    }
}

#[component]
fn CardTile(it: Value) -> Element {
    let mut mc = use_context::<Mc>();
    let id = txt(&it, "id");
    let kind = txt(&it, "kind");
    let seed = it["stage"] == "seed";
    let child = txt(&it, "ref_id");
    rsx! {
        button {
            class: "mc-tile mc-k-{kind}",
            class: if seed { "is-seed" },
            class: if it["saving"] == true { "is-saving" },
            r#type: "button",
            onclick: move |_| {
                if kind == "board" && !child.is_empty() { open_board(mc, child.clone()) } else { mc.card.set(Some(id.clone())) }
            },
            CardFace { it: it.clone() }
        }
    }
}

#[component]
fn CanvasItem(it: Value) -> Element {
    let (x, y, w, h) = (it["x"].as_f64().unwrap_or(0.0), it["y"].as_f64().unwrap_or(0.0), it["w"].as_f64().unwrap_or(240.0), it["h"].as_f64().unwrap_or(200.0));
    let z = it["z"].as_i64().unwrap_or(0);
    let rot = it["rot"].as_f64().unwrap_or(0.0);
    let kind = txt(&it, "kind");
    let seed = it["stage"] == "seed";
    rsx! {
        div {
            class: "mc-item mc-k-{kind}",
            class: if seed { "is-seed" },
            "data-id": "{txt(&it, \"id\")}",
            "data-kind": "{kind}",
            "data-x": "{x}", "data-y": "{y}", "data-w": "{w}", "data-h": "{h}", "data-z": "{z}",
            style: "transform: translate({x}px, {y}px) rotate({rot}deg); width: {w}px; height: {h}px; z-index: {z};",
            CardFace { it: it.clone() }
            span { class: "mc-rz", aria_hidden: "true" }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Card sheet: caption, context, the dream path

#[component]
pub fn CardSheet() -> Element {
    let mut mc = use_context::<Mc>();
    let Some(id) = (mc.card)() else { return rsx! {} };
    let Some(it) = mc.item(&id) else {
        return rsx! { div { class: "mc-sheet-layer", onclick: move |_| mc.card.set(None) } };
    };
    let kind = txt(&it, "kind");
    let stage = txt(&it, "stage");
    let src = image_src(&it);
    let child = txt(&it, "ref_id");
    let verified = it["verified_at"].as_str().map(short_day);

    let mut save = move |field: &'static str, value: String| {
        let id = mc.card.peek().clone().unwrap_or_default();
        let v = if value.trim().is_empty() { Value::Null } else { json!(value.trim()) };
        mc.patch_item(&id, |x| x[field] = v.clone());
        mc.write("mission_board_item_update", json!({ "id": id, field: v }));
    };

    rsx! {
        div { class: "mc-sheet-layer",
            div { class: "scrim", onclick: move |_| mc.card.set(None) }
            div { class: "mc-sheet", role: "dialog", aria_label: "Card",
                div { class: "mc-sheet-top",
                    span { class: "mc-sheet-kind", "{kind} card" }
                    button { class: "mc-icon-btn", r#type: "button", aria_label: "Close", onclick: move |_| mc.card.set(None), Icon { name: IconName::X, size: 18 } }
                }
                if let Some(s) = src { img { class: "mc-sheet-img", src: "{s}", alt: "" } }
                if let Some(mid) = it["mind_item_id"].as_str().map(String::from) {
                    // A card that shows a Mind library item: its words live in the library.
                    p { class: "mc-path-note", b { "{txt(&it, \"caption\")}" } }
                    if !txt(&it, "context").is_empty() { p { class: "mc-path-note", "{txt(&it, \"context\")}" } }
                    div { class: "mc-sheet-actions",
                        button { class: "btn btn-primary", r#type: "button",
                            onclick: move |_| {
                                mc.card.set(None);
                                mc.open_board.set(None);
                                mc.seg.set(super::Seg::Mind);
                                super::set_hash("mind");
                                let mut m = consume_context::<super::mind::Mind>();
                                m.open_item(mid.clone());
                            },
                            "Open in Mind"
                        }
                        button { class: "mc-ghost mc-danger", r#type: "button",
                            onclick: move |_| {
                                let id = mc.card.peek().clone().unwrap_or_default();
                                if let Some(b) = mc.board.write().as_mut().and_then(|b| b["items"].as_array_mut()) { b.retain(|x| x["id"] != id.as_str()); }
                                mc.card.set(None);
                                mc.write("mission_board_item_remove", json!({ "id": id }));
                            },
                            "Take off board"
                        }
                    }
                } else {
                label { class: "mc-field",
                    span { if kind == "board" { "Title" } else { "Caption" } }
                    input { r#type: "text", value: "{txt(&it, \"caption\")}", placeholder: "Say what it is", onchange: move |e| save("caption", e.value()) }
                }
                label { class: "mc-field",
                    span { "Context" }
                    textarea { rows: "2", value: "{txt(&it, \"context\")}", placeholder: "What is this for?", onchange: move |e| save("context", e.value()) }
                }
                if kind == "link" || (kind == "image" && it["asset_id"].is_null()) {
                    label { class: "mc-field",
                        span { "Link" }
                        input { r#type: "url", value: "{txt(&it, \"url\")}", onchange: move |e| save("url", e.value()) }
                    }
                }
                if kind != "goal" {
                    div { class: "mc-path",
                        div { class: "mc-path-head",
                            h3 { "The path" }
                            span { class: "mc-path-n", "{path_count(&it)}/4" }
                        }
                        p { class: "mc-path-note", "A dream on its own can feel like done. The path makes it a plan." }
                        for (k, label, hint) in PATH {
                            label { key: "{k}", class: "mc-field mc-field-path",
                                span { "{label}" }
                                textarea { rows: "1", value: "{txt(&it, k)}", placeholder: "{hint}", onchange: move |e| save(k, e.value()) }
                            }
                        }
                        div { class: "mc-stages", role: "group", aria_label: "Stage",
                            for s in ["seed", "rooted", "moving", "reached", "dropped"] {
                                button {
                                    key: "{s}", r#type: "button", class: if stage == s { "on" }, "data-stage": "{s}",
                                    onclick: move |_| {
                                        let id = mc.card.peek().clone().unwrap_or_default();
                                        mc.patch_item(&id, |x| x["stage"] = json!(s));
                                        mc.write("mission_board_item_update", json!({ "id": id, "stage": s }));
                                    },
                                    "{s}"
                                }
                            }
                        }
                    }
                }
                div { class: "mc-sheet-actions",
                    if kind == "board" && !child.is_empty() {
                        button { class: "btn btn-primary", r#type: "button", onclick: move |_| { mc.card.set(None); open_board(mc, child.clone()); }, "Open board" }
                    }
                    if let Some(node) = it["node_id"].as_str().map(String::from) {
                        button { class: "mc-ghost", r#type: "button",
                            onclick: move |_| {
                                let id = mc.card.peek().clone().unwrap_or_default();
                                mc.patch_item(&id, |x| x["verified_at"] = json!(chrono::Local::now().format("%Y-%m-%d").to_string()));
                                mc.write("mission_node_verify", json!({ "kind": "node", "id": node.clone() }));
                            },
                            Icon { name: IconName::Check, size: 16 }
                            if let Some(d) = verified.clone() { "Still true · {d}" } else { "Still true" }
                        }
                    }
                    button { class: "mc-ghost mc-danger", r#type: "button",
                        onclick: move |_| {
                            let id = mc.card.peek().clone().unwrap_or_default();
                            if let Some(items) = mc.board.write().as_mut().and_then(|b| b["items"].as_array_mut()) { items.retain(|i| i["id"] != id.as_str()); }
                            mc.card.set(None);
                            mc.write("mission_board_item_remove", json!({ "id": id }));
                        },
                        "Remove card"
                    }
                }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Add sheet

#[component]
pub fn AddSheet() -> Element {
    let mut mc = use_context::<Mc>();
    let state = use_context::<AppState>();
    let board = (mc.open_board)().unwrap_or_default();
    let mut link = use_signal(String::new);
    let mut note = use_signal(String::new);
    let mut sub = use_signal(String::new);
    let mut picking_goal = use_signal(|| false);
    let goals = (mc.goals)();
    let b1 = board.clone();
    let b2 = board.clone();
    let b3 = board.clone();

    rsx! {
        div { class: "mc-sheet-layer",
            div { class: "scrim", onclick: move |_| mc.adding.set(false) }
            div { class: "mc-sheet mc-add", role: "dialog", aria_label: "Add to board",
                div { class: "mc-sheet-top",
                    span { class: "mc-sheet-kind", "Add to board" }
                    button { class: "mc-icon-btn", r#type: "button", aria_label: "Close", onclick: move |_| mc.adding.set(false), Icon { name: IconName::X, size: 18 } }
                }
                div { class: "mc-add-grid",
                    label { class: "mc-add-opt",
                        input { r#type: "file", accept: "image/*", multiple: true, "data-mc-image": "{board}", class: "mc-file" }
                        Icon { name: IconName::Camera, size: 22 }
                        span { "Photos" }
                    }
                    label { class: "mc-add-opt",
                        input { r#type: "file", accept: ".jpg,.jpeg,.png,.webp,.gif,.heic,image/*", "data-mc-image": "{board}", class: "mc-file" }
                        Icon { name: IconName::Inbox, size: 22 }
                        span { "Files" }
                    }
                    button { class: "mc-add-opt", r#type: "button", onclick: move |_| { let _ = document::eval("window.__aosBoard && window.__aosBoard.pasteButton()"); },
                        Icon { name: IconName::Sparkle, size: 22 }
                        span { "Paste" }
                    }
                    button { class: "mc-add-opt", r#type: "button",
                        onclick: move |_| {
                            picking_goal.set(true);
                            if mc.goals.peek().is_none() { dioxus::core::spawn_forever(async move { mc.load_version().await }); }
                        },
                        Icon { name: IconName::Mountain, size: 22 }
                        span { "Goal" }
                    }
                }
                form { class: "mc-inline",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let u = link.peek().trim().to_string();
                        if u.starts_with("http") { add_from_link(mc, state, u); link.set(String::new()); mc.adding.set(false); }
                    },
                    input { r#type: "url", placeholder: "Image or page link (https://…)", value: "{link}", oninput: move |e| link.set(e.value()) }
                    button { class: "btn", r#type: "submit", disabled: !link().trim().starts_with("http"), "Add" }
                }
                form { class: "mc-inline",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let t = note.peek().trim().to_string();
                        if !t.is_empty() {
                            let id = add_card(mc, json!({ "board_id": b1.clone(), "kind": "text", "caption": t }));
                            note.set(String::new());
                            mc.adding.set(false);
                            mc.card.set(Some(id));
                        }
                    },
                    textarea { rows: "2", placeholder: "Write what you want, in your words", value: "{note}", oninput: move |e| note.set(e.value()) }
                    button { class: "btn", r#type: "submit", disabled: note().trim().is_empty(), "Add note" }
                }
                form { class: "mc-inline",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let t = sub.peek().trim().to_string();
                        if !t.is_empty() {
                            add_card(mc, json!({ "board_id": b2.clone(), "kind": "board", "caption": t, "h": 220.0 }));
                            sub.set(String::new());
                            mc.adding.set(false);
                        }
                    },
                    input { r#type: "text", placeholder: "New board inside this one (a subpage)", value: "{sub}", oninput: move |e| sub.set(e.value()) }
                    button { class: "btn", r#type: "submit", disabled: sub().trim().is_empty(), "Add board" }
                }
                if picking_goal() {
                    div { class: "mc-goal-pick",
                        match goals {
                            None => rsx! { p { class: "mc-empty", "Loading goals…" } },
                            Some(g) if g.is_empty() => rsx! { p { class: "mc-empty", "No goals yet. Add them in Version." } },
                            Some(g) => rsx! {
                                for x in g {
                                    button { key: "{txt(&x, \"id\")}", class: "mc-goal-row", r#type: "button",
                                        onclick: { let gid = txt(&x, "id"); let b = b3.clone(); let t = txt(&x, "title"); move |_| {
                                            add_card(mc, json!({ "board_id": b.clone(), "kind": "goal", "goal_id": gid.clone(), "caption": t.clone(), "h": 150.0 }));
                                            mc.adding.set(false);
                                        } },
                                        span { class: "mc-goal-h", "{txt(&x, \"horizon\")}" }
                                        span { "{txt(&x, \"title\")}" }
                                    }
                                }
                            },
                        }
                    }
                }
                p { class: "mc-add-hint", "On a computer you can also paste (Ctrl+V) or drop images anywhere on the board." }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Play: the board as a short show, one dream at a time with its why.

#[component]
pub fn PlayLayer() -> Element {
    let mut mc = use_context::<Mc>();
    let mut i = use_signal(|| 0usize);
    let slides: Vec<Value> = (mc.board)()
        .and_then(|b| b["items"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|x| image_src(x).is_some() && x["stage"] != "dropped")
        .collect();
    let n = slides.len().max(1);
    use_future(move || async move {
        loop {
            crate::live::net::sleep_ms(5200).await;
            let next = *i.peek() + 1;
            i.set(next);
        }
    });
    let cur = slides.get(i() % n).cloned().unwrap_or(Value::Null);
    let src = image_src(&cur).unwrap_or_default();
    let line = cur["why"].as_str().or(cur["context"].as_str()).unwrap_or("").to_string();
    rsx! {
        div { class: "mc-play", onclick: move |_| { let v = *i.peek() + 1; i.set(v) },
            img { key: "{i()}", class: "mc-play-img", src: "{src}", alt: "" }
            div { class: "mc-play-text", key: "t{i()}",
                span { class: "mc-play-cap", "{txt(&cur, \"caption\")}" }
                if !line.is_empty() { span { class: "mc-play-why", "{line}" } }
                if let Some(f) = cur["first_step"].as_str() { span { class: "mc-play-step", "First step: {f}" } }
            }
            button { class: "mc-icon-btn mc-play-x", r#type: "button", aria_label: "Close", onclick: move |e| { e.stop_propagation(); mc.playing.set(false) }, Icon { name: IconName::X, size: 20 } }
        }
    }
}
