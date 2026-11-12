//! Mind OS: the library of saved things (design/specs/mind-os.md, design/MINDOS-BUILD.md).
//!
//! A quiet masonry grid of cards (image first), a search pill (words and #tags), a capture row
//! (type, mic, photo, paste; drag or paste anywhere on desktop), and a detail sheet with Nora's
//! read, his words verbatim, Nora's notes, tags, links, backlinks and "Add to board".
//! Data comes from the `mind_*` tools; writes go through the offline outbox.

use dioxus::prelude::*;
use serde_json::{Value, json};

use super::data::{Mc, short_day, txt};
use crate::components::voice::MicButton;
use crate::components::{Icon, IconName};
use crate::live::Live;
use crate::state::AppState;

const MIND_JS: &str = include_str!("../../js/mind.js");

/// Set by the capture sheet's "Open Mind" so the Mission page opens on Mind.
pub static OPEN_MIND: GlobalSignal<bool> = Signal::global(|| false);

const FILTERS: &[(&str, &str)] = &[
    ("", "Everything"),
    ("inbox", "Inbox"),
    ("note", "Notes"),
    ("link", "Links"),
    ("image", "Images"),
    ("video", "Reels"),
    ("quote", "Quotes"),
    ("person", "People"),
    ("book", "Books"),
    ("decision", "Decisions"),
    ("learning", "Learnings"),
];

fn kind_label(k: &str) -> &'static str {
    match k {
        "link" => "link",
        "image" => "image",
        "video" => "reel",
        "quote" => "quote",
        "file" => "file",
        "person" => "person",
        "book" => "book",
        "decision" => "decision",
        "learning" => "learning",
        "daily" => "day",
        _ => "note",
    }
}

#[derive(Clone, Copy)]
pub struct Mind {
    pub items: Signal<Option<Vec<Value>>>,
    pub query: Signal<String>,
    pub filter: Signal<&'static str>,
    pub open: Signal<Option<String>>,
    pub detail: Signal<Option<Value>>,
    pub stats: Signal<Value>,
    pub busy: Signal<bool>,
    seq: Signal<u64>,
    live: Live,
}

impl Mind {
    pub fn new(live: Live) -> Mind {
        Mind {
            items: Signal::new(None),
            query: Signal::new(String::new()),
            filter: Signal::new(""),
            open: Signal::new(None),
            detail: Signal::new(None),
            stats: Signal::new(Value::Null),
            busy: Signal::new(false),
            seq: Signal::new(0),
            live,
        }
    }

    /// Reload the grid for the current search and filter. Older answers are dropped.
    pub async fn load(&mut self) {
        let n = {
            let mut s = self.seq.write();
            *s += 1;
            *s
        };
        let f = *self.filter.peek();
        let safe = try_consume_context::<AppState>().map(|s| *s.safe.peek()).unwrap_or(false);
        let mut args = json!({ "query": self.query.peek().clone(), "safe": safe, "limit": 200 });
        if f == "inbox" {
            args["inbox"] = json!(true);
        } else if !f.is_empty() {
            args["kind"] = json!(f);
        }
        let mut live = self.live;
        if let Ok(v) = live.post_json("/api/tools/mind_search", args).await {
            if *self.seq.peek() == n {
                self.items.set(Some(v["items"].as_array().cloned().unwrap_or_default()));
            }
        }
        if let Ok(s) = live.post_json("/api/tools/mind_stats", json!({})).await {
            self.stats.set(s);
        }
    }

    pub async fn load_detail(&mut self) {
        let Some(id) = self.open.peek().clone() else { return };
        let mut live = self.live;
        if let Ok(v) = live.post_json("/api/tools/mind_read", json!({ "id": id })).await {
            if self.open.peek().as_deref() == Some(id.as_str()) {
                self.detail.set(Some(v));
            }
        }
    }

    pub fn open_item(&mut self, id: String) {
        self.detail.set(self.items.peek().as_ref().and_then(|l| l.iter().find(|i| i["id"] == id.as_str()).cloned()));
        self.open.set(Some(id));
        let mut me = *self;
        dioxus::core::spawn_forever(async move { me.load_detail().await });
    }

    /// Queue a write, then refresh the grid (and the open item) once it reached the server.
    pub fn write(&mut self, tool: &str, args: Value) {
        let mut live = self.live;
        live.write(tool, args);
        let mut me = *self;
        dioxus::core::spawn_forever(async move {
            crate::live::net::sleep_ms(300).await;
            live.flush().await;
            crate::live::net::sleep_ms(120).await;
            me.load().await;
            me.load_detail().await;
        });
    }

    /// Save typed or spoken words, or a lone link. Shows at once at the top of the grid.
    pub fn save_text(&mut self, mc: &mut Mc, text: &str, origin: &str) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let id = mc.new_id();
        let is_url = (text.starts_with("https://") || text.starts_with("http://")) && !text.contains(char::is_whitespace);
        let kind = if is_url { "link" } else if text.starts_with('"') || text.starts_with('\u{201C}') { "quote" } else { "note" };
        let host = if is_url { text.split("//").nth(1).and_then(|r| r.split('/').next()).map(|h| h.trim_start_matches("www.").to_string()) } else { None };
        self.prepend(json!({ "id": id, "kind": kind, "raw": text, "source_domain": host, "status": "saved", "inbox": 1, "tags": [], "saved_at": now_iso() }));
        self.write("mind_save", json!({ "id": id, "text": text, "origin": origin }));
    }

    pub fn save_image(&mut self, mc: &mut Mc, asset: &str, w: f64, h: f64) {
        let id = mc.new_id();
        self.prepend(json!({ "id": id, "kind": "image", "raw": "", "thumb_asset_id": asset, "width": w, "height": h, "status": "saved", "inbox": 1, "tags": [], "saved_at": now_iso() }));
        let mut args = json!({ "id": id, "asset_id": asset, "origin": "paste" });
        if w > 0.0 {
            args["width"] = json!(w as i64);
            args["height"] = json!(h as i64);
        }
        self.write("mind_save", args);
    }

    fn prepend(&mut self, item: Value) {
        let mut g = self.items.write();
        match g.as_mut() {
            Some(list) => list.insert(0, item),
            None => *g = Some(vec![item]),
        }
    }

    pub fn media(&self, asset: &str) -> Option<String> {
        let c = self.live.conn.peek().clone()?;
        Some(format!("{}/api/media/{asset}?token={}", c.url.trim_end_matches('/'), c.token))
    }
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn busy_status(it: &Value) -> bool {
    matches!(it["status"].as_str(), Some("saved" | "fetching" | "reading"))
}

/// The image a card shows: thumbnail, original, or the page's image link while it downloads.
fn card_image(m: &Mind, it: &Value) -> Option<String> {
    if let Some(a) = it["thumb_asset_id"].as_str().or(it["asset_id"].as_str()) {
        return m.media(a);
    }
    it["image_url"].as_str().map(String::from)
}

/// Install the paste/drop/upload module and keep it configured.
pub fn use_mind_bridge(mut mind: Mind, mut mc: Mc, live: Live, mut state: AppState) {
    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(MIND_JS);
            while let Ok(msg) = eval.recv::<Value>().await {
                match msg["type"].as_str().unwrap_or("") {
                    "uploaded" => {
                        let asset = msg["asset"]["id"].as_str().unwrap_or_default().to_string();
                        mind.save_image(&mut mc, &asset, msg["w"].as_f64().unwrap_or(0.0), msg["h"].as_f64().unwrap_or(0.0));
                        state.toast("Saved to Mind");
                    }
                    "paste-url" | "paste-text" => {
                        let t = msg["text"].as_str().or(msg["url"].as_str()).unwrap_or_default().to_string();
                        mind.save_text(&mut mc, &t, "paste");
                        state.toast("Saved to Mind");
                    }
                    "busy" => mind.busy.set(msg["on"].as_bool().unwrap_or(false)),
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
                "(function f(n) {{ var M = window.__aosMind; if (M && M.config) {{ M.config({cfg}); }} else if (n < 60) {{ setTimeout(function () {{ f(n + 1); }}, 100); }} }})(0);"
            ));
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Home: capture, search, filters, grid

#[component]
pub fn MindHome() -> Element {
    let mut mind = use_context::<Mind>();
    let mut mc = use_context::<Mc>();
    let live = use_context::<Live>();
    let state = use_context::<AppState>();
    let mut draft = use_signal(String::new);
    let mut spoke = use_signal(|| false);

    // First load, and again when the connection or Safe mode changes.
    use_effect(move || {
        let _ = (state.safe)();
        if live.conn.read().is_some() {
            spawn(async move { mind.load().await });
        }
    });
    // While cards are still being read, refresh quietly.
    use_future(move || async move {
        loop {
            crate::live::net::sleep_ms(3500).await;
            let pending = mind.items.peek().as_ref().is_some_and(|l| l.iter().any(busy_status));
            if pending {
                mind.load().await;
                if mind.detail.peek().as_ref().is_some_and(busy_status) {
                    mind.load_detail().await;
                }
            }
        }
    });

    let items = (mind.items)();
    let filter = (mind.filter)();
    let stats = (mind.stats)();
    let query = (mind.query)();
    let reading = stats["reading"].as_i64().unwrap_or(0);

    let mut submit = move || {
        let t = draft.peek().clone();
        if !t.trim().is_empty() {
            let origin = if *spoke.peek() { "voice" } else { "capture" };
            mind.save_text(&mut mc, &t, origin);
            draft.set(String::new());
            spoke.set(false);
        }
    };

    rsx! {
        section { class: "mind-page",
            div { class: "mind-top",
                div { class: "mind-title",
                    h2 { class: "mc-h", "Mind" }
                    p { class: "mind-count",
                        "{stats[\"items\"].as_i64().unwrap_or(0)} saved"
                        if stats["inbox"].as_i64().unwrap_or(0) > 0 { " · {stats[\"inbox\"]} new" }
                        if reading > 0 { " · Nora is reading {reading}" }
                    }
                }
                label { class: "mind-search",
                    span { class: "mind-search-ico", aria_hidden: "true",
                        svg { view_box: "0 0 24 24", width: "17", height: "17", fill: "none", stroke: "currentColor", stroke_width: "1.9", stroke_linecap: "round",
                            circle { cx: "11", cy: "11", r: "7" }
                            path { d: "m20 20-3.5-3.5" }
                        }
                    }
                    input {
                        id: "mind-search",
                        r#type: "search",
                        placeholder: "Search your mind",
                        aria_label: "Search your mind",
                        value: "{query}",
                        oninput: move |e| {
                            mind.query.set(e.value());
                            spawn(async move {
                                crate::live::net::sleep_ms(180).await;
                                mind.load().await;
                            });
                        },
                    }
                    if !query.is_empty() {
                        button { class: "mind-search-x", r#type: "button", aria_label: "Clear search",
                            onclick: move |_| { mind.query.set(String::new()); spawn(async move { mind.load().await }); },
                            Icon { name: IconName::X, size: 15 }
                        }
                    } else {
                        kbd { class: "mind-kbd", "/" }
                    }
                }
            }
            form { class: "mind-capture",
                onsubmit: move |e| { e.prevent_default(); submit() },
                input {
                    r#type: "text",
                    class: "mind-capture-field",
                    placeholder: "Save a thought, a link, a quote…",
                    aria_label: "Save to Mind",
                    value: "{draft}",
                    oninput: move |e| draft.set(e.value()),
                }
                MicButton {
                    target: "mind",
                    class: "mind-mic",
                    size: 18,
                    on_text: move |t: String| {
                        let cur = draft.peek().clone();
                        draft.set(if cur.trim().is_empty() { t } else { format!("{} {t}", cur.trim_end()) });
                        spoke.set(true);
                    },
                }
                label { class: "mind-icon-btn", title: "Add a photo", aria_label: "Add a photo",
                    input { r#type: "file", accept: "image/*", multiple: true, "data-mind-image": "1", class: "sr-only" }
                    Icon { name: IconName::Camera, size: 18 }
                }
                button { class: "mind-icon-btn", r#type: "button", title: "Paste", aria_label: "Paste from clipboard",
                    onclick: move |_| { let _ = document::eval("window.__aosMind && window.__aosMind.pasteButton();"); },
                    Icon { name: IconName::Plus, size: 18 }
                }
                if !draft().trim().is_empty() {
                    button { class: "mind-save", r#type: "submit", "Save" }
                }
            }
            nav { class: "mind-filters", aria_label: "Filter",
                for (k, label) in FILTERS.iter().copied() {
                    button {
                        key: "{k}",
                        r#type: "button",
                        class: if filter == k { "on" },
                        onclick: move |_| { mind.filter.set(k); spawn(async move { mind.load().await }); },
                        "{label}"
                    }
                }
            }
            match items {
                None => rsx! { p { class: "mc-empty", if live.paired() { "Opening your mind…" } else { "Pair this device to open your library." } } },
                Some(list) if list.is_empty() && !query.is_empty() => rsx! {
                    div { class: "mind-empty", h3 { "Nothing matches \u{201C}{query}\u{201D}" } p { "Search looks at your words, Nora's descriptions, tags (#tag) and page text." } }
                },
                Some(list) if list.is_empty() => rsx! {
                    div { class: "mind-empty",
                        div { class: "mind-empty-orb", aria_hidden: "true" }
                        h3 { "Remember everything. Organise nothing." }
                        p { "Type or say a thought above, paste a link or a picture, or drag one in. Nora writes what it is and tags it." }
                    }
                },
                Some(list) => rsx! {
                    div { class: "mind-grid",
                        for it in list {
                            MindCard { key: "{it[\"id\"].as_str().unwrap_or_default()}", it: it.clone() }
                        }
                    }
                },
            }
            if (mind.busy)() { div { class: "mind-busy", "Saving picture…" } }
        }
    }
}

#[component]
fn MindCard(it: Value) -> Element {
    let mut mind = use_context::<Mind>();
    let id = txt(&it, "id");
    let kind = txt(&it, "kind");
    let img = card_image(&mind, &it);
    let title = it["title"].as_str().filter(|t| !t.is_empty()).map(String::from);
    let desc = txt(&it, "description");
    let raw = txt(&it, "raw");
    let domain = it["source_domain"].as_str().or(it["site_name"].as_str()).unwrap_or_default().to_string();
    let color = it["color"].as_str().unwrap_or("#9b8cff").to_string();
    let reading = busy_status(&it);
    let ratio = match (it["width"].as_f64(), it["height"].as_f64()) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => format!("{}", (w / h).clamp(0.4, 2.4)),
        _ => "1.25".into(),
    };
    let text_card = img.is_none();
    let excerpt: String = raw.chars().take(if kind == "quote" { 220 } else { 180 }).collect();

    rsx! {
        button {
            class: "mind-card",
            class: if text_card { "is-text" },
            class: if reading { "is-reading" },
            "data-kind": "{kind}",
            style: "--c: {color}",
            r#type: "button",
            onclick: move |_| mind.open_item(id.clone()),
            if let Some(src) = img {
                div { class: "mind-card-media", style: "aspect-ratio: {ratio}",
                    img { src: "{src}", alt: "{title.clone().unwrap_or_default()}", loading: "lazy" }
                    if kind == "video" { span { class: "mind-play", aria_hidden: "true" } }
                }
            }
            div { class: "mind-card-body",
                if text_card && kind == "quote" {
                    p { class: "mind-quote", "{excerpt.trim_matches(|c| c == '\"' || c == '\u{201C}' || c == '\u{201D}')}" }
                } else if text_card && (kind == "note" || title.is_none()) && !excerpt.is_empty() && !excerpt.starts_with("http") {
                    p { class: "mind-excerpt", "{excerpt}" }
                } else if let Some(t) = title.clone() {
                    p { class: "mind-card-title", "{t}" }
                }
                if !text_card && title.is_some() && !desc.is_empty() {
                    p { class: "mind-card-desc", "{desc}" }
                } else if text_card && kind != "note" && kind != "quote" && !desc.is_empty() {
                    p { class: "mind-card-desc", "{desc}" }
                }
                div { class: "mind-card-foot",
                    span { class: "mind-chip", "{kind_label(&kind)}" }
                    if !domain.is_empty() { span { class: "mind-domain", "{domain}" } }
                    if reading { span { class: "mind-reading", "reading" } }
                    if it["sensitive"] == 1 { span { class: "mind-lock", Icon { name: IconName::Lock, size: 12 } } }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Detail

/// His words with `[[links]]` as tappable chips.
fn word_parts(raw: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(i) = rest.find("[[") {
        let after = &rest[i + 2..];
        let Some(end) = after.find("]]") else { break };
        if i > 0 {
            out.push((false, rest[..i].to_string()));
        }
        out.push((true, after[..end].to_string()));
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        out.push((false, rest.to_string()));
    }
    out
}

#[component]
pub fn MindDetail() -> Element {
    let mut mind = use_context::<Mind>();
    let mc = use_context::<Mc>();
    let mut state = use_context::<AppState>();
    let mut thought = use_signal(String::new);
    let mut tag_draft = use_signal(String::new);
    let mut link_q = use_signal(String::new);
    let mut link_hits = use_signal(Vec::<Value>::new);
    let mut boards_open = use_signal(|| false);
    let Some(it) = (mind.detail)() else {
        return rsx! { div { class: "mind-layer", div { class: "scrim", onclick: move |_| mind.open.set(None) } } };
    };
    let id = txt(&it, "id");
    let kind = txt(&it, "kind");
    let img = it["asset_id"].as_str().or(it["thumb_asset_id"].as_str()).and_then(|a| mind.media(a)).or_else(|| it["image_url"].as_str().map(String::from));
    let title = it["title"].as_str().unwrap_or_default().to_string();
    let raw = txt(&it, "raw");
    let url = it["source_url"].as_str().map(String::from);
    let domain = txt(&it, "source_domain");
    let desc = txt(&it, "description");
    let summary = txt(&it, "summary");
    let reading = busy_status(&it);
    let color = it["color"].as_str().unwrap_or("#9b8cff").to_string();
    let tags: Vec<Value> = it["tags"].as_array().cloned().unwrap_or_default();
    let links: Vec<Value> = it["links"].as_array().cloned().unwrap_or_default();
    let backs: Vec<Value> = it["backlinks"].as_array().cloned().unwrap_or_default();
    let notes: Vec<Value> = it["notes"].as_array().cloned().unwrap_or_default();
    let on_boards: Vec<Value> = it["boards"].as_array().cloned().unwrap_or_default();
    let boards: Vec<Value> = (mc.boards)().unwrap_or_default();
    let saved = it["saved_at"].as_str().map(short_day).unwrap_or_default();
    let origin = txt(&it, "origin");
    let inbox = it["inbox"] == 1;
    let pinned = it["pinned"] == 1;
    let sensitive = it["sensitive"] == 1;
    let links_for_words = links.clone();
    let failed = it["error"].as_str().map(String::from);

    let mut set = move |field: &'static str, v: Value| {
        let id = mind.open.peek().clone().unwrap_or_default();
        if let Some(d) = mind.detail.write().as_mut() {
            d[field] = v.clone();
        }
        mind.write("mind_update", json!({ "id": id, field: v }));
    };
    let mut close = move || {
        mind.open.set(None);
        mind.detail.set(None);
    };

    rsx! {
        div { class: "mind-layer", style: "--c: {color}",
            div { class: "scrim", onclick: move |_| close() }
            article { class: "mind-sheet", role: "dialog", aria_label: "Saved item",
                div { class: "mind-sheet-top",
                    span { class: "mind-chip", "{kind_label(&kind)}" }
                    span { class: "mind-sheet-date", "Saved {saved}", if !origin.is_empty() && origin != "app" { " · {origin}" } }
                    button { class: "mc-icon-btn", r#type: "button", aria_label: "Close", onclick: move |_| close(), Icon { name: IconName::X, size: 18 } }
                }
                div { class: "mind-sheet-grid",
                    if let Some(src) = img.clone() {
                        div { class: "mind-sheet-media",
                            img { class: "mind-sheet-glow", src: "{src}", alt: "", aria_hidden: "true" }
                            img { class: "mind-sheet-img", src: "{src}", alt: "{title}" }
                        }
                    }
                    div { class: "mind-inspector",
                        input {
                            class: "mind-title-field",
                            r#type: "text",
                            value: "{title}",
                            placeholder: "Untitled",
                            aria_label: "Title",
                            onchange: move |e| set("title", json!(e.value().trim())),
                        }
                        if let Some(u) = url.clone() {
                            a { class: "mind-source", href: "{u}", target: "_blank", rel: "noopener noreferrer", "{domain}" span { aria_hidden: "true", " ↗" } }
                        }
                        section { class: "mind-block mind-nora",
                            h4 { Icon { name: IconName::Sparkle, size: 14 } "Nora's read" }
                            if reading {
                                p { class: "mind-shimmer", "Reading…" }
                            } else if desc.is_empty() && summary.is_empty() {
                                p { class: "mind-muted",
                                    if sensitive { "Private: kept on your PC, never sent to Claude." } else if failed.is_some() { "Nora couldn't read this one." } else { "Nothing yet." }
                                }
                            } else {
                                if !desc.is_empty() { p { "{desc}" } }
                                if !summary.is_empty() { p { class: "mind-summary", "{summary}" } }
                            }
                        }
                        if !raw.trim().is_empty() && url.as_deref() != Some(raw.trim()) {
                            section { class: "mind-block mind-words",
                                h4 { "Your words" }
                                p { class: "mind-raw",
                                    for (i, (is_link, part)) in word_parts(&raw).into_iter().enumerate() {
                                        if is_link {
                                            {
                                                let target = links_for_words.iter().find(|l| l["to_title"].as_str().is_some_and(|t| t.eq_ignore_ascii_case(part.split('|').next().unwrap_or("").trim()))).and_then(|l| l["to_id"].as_str().map(String::from));
                                                let label = part.split('|').next_back().unwrap_or("").to_string();
                                                rsx! {
                                                    button { key: "{i}", class: "mind-wl", class: if target.is_none() { "is-dangling" }, r#type: "button",
                                                        onclick: move |_| match target.clone() { Some(t) => mind.open_item(t), None => state.toast("Nothing has that title yet. Save it and the link fills in.") },
                                                        "{label}"
                                                    }
                                                }
                                            }
                                        } else {
                                            span { key: "{i}", "{part}" }
                                        }
                                    }
                                }
                            }
                        }
                        if !notes.is_empty() {
                            section { class: "mind-block",
                                h4 { "Added later" }
                                for n in notes.iter() {
                                    div { key: "{txt(n, \"id\")}", class: "mind-note",
                                        span { class: "mind-note-meta", if n["by"] == "nora" { "Nora" } else { "You" } " · {short_day(n[\"at\"].as_str().unwrap_or(\"\"))}" }
                                        p { "{txt(n, \"text\")}" }
                                    }
                                }
                            }
                        }
                        form { class: "mind-add-row",
                            onsubmit: move |e| {
                                e.prevent_default();
                                let t = thought.peek().trim().to_string();
                                if !t.is_empty() {
                                    let id = mind.open.peek().clone().unwrap_or_default();
                                    mind.write("mind_append", json!({ "id": id, "text": t }));
                                    thought.set(String::new());
                                }
                            },
                            input { r#type: "text", placeholder: "Add a thought ([[links]] work)", value: "{thought}", oninput: move |e| thought.set(e.value()) }
                        }
                        section { class: "mind-block",
                            h4 { "Tags" }
                            div { class: "mind-tags",
                                for t in tags.iter() {
                                    {
                                        let tag = txt(t, "tag");
                                        let tg = tag.clone();
                                        rsx! {
                                            span { key: "{tag}", class: "mind-tag", class: if t["by"] == "claude" { "by-claude" },
                                                "#{tag}"
                                                button { r#type: "button", aria_label: "Remove tag {tag}",
                                                    onclick: move |_| {
                                                        let id = mind.open.peek().clone().unwrap_or_default();
                                                        if let Some(a) = mind.detail.write().as_mut().and_then(|d| d["tags"].as_array_mut()) {
                                                            a.retain(|x| x["tag"] != tg.as_str());
                                                        }
                                                        mind.write("mind_update", json!({ "id": id, "tags_remove": [tg.clone()] }));
                                                    },
                                                    "×"
                                                }
                                            }
                                        }
                                    }
                                }
                                form { class: "mind-tag-add",
                                    onsubmit: move |e| {
                                        e.prevent_default();
                                        let t = tag_draft.peek().trim().trim_start_matches('#').to_string();
                                        if !t.is_empty() {
                                            let id = mind.open.peek().clone().unwrap_or_default();
                                            mind.write("mind_update", json!({ "id": id, "tags_add": [t] }));
                                            tag_draft.set(String::new());
                                        }
                                    },
                                    input { r#type: "text", placeholder: "+ tag", value: "{tag_draft}", oninput: move |e| tag_draft.set(e.value()) }
                                }
                            }
                        }
                        section { class: "mind-block",
                            h4 { "Links" }
                            if links.is_empty() && backs.is_empty() {
                                p { class: "mind-muted", "Nothing linked yet. Write [[a title]] in a thought, or link below." }
                            }
                            for l in links.iter().filter(|l| l["rel"] != "part_of") {
                                {
                                    let to = l["to_id"].as_str().map(String::from);
                                    let is_item = l["to_kind"] == "mind_item";
                                    let label = l["label"].as_str().or(l["to_title"].as_str()).unwrap_or("Untitled").to_string();
                                    rsx! {
                                        button { key: "{txt(l, \"id\")}", class: "mind-link-row", r#type: "button",
                                            disabled: !is_item || to.is_none(),
                                            onclick: move |_| if let Some(t) = to.clone() { mind.open_item(t) },
                                            span { class: "mind-link-rel", "{txt(l, \"rel\").replace('_', \" \")}" }
                                            span { class: "mind-link-label", "{label}" }
                                            if is_item && to.is_none() { span { class: "mind-muted", " (not saved yet)" } }
                                            if l["removed"] == 1 { span { class: "mind-muted", " (removed)" } }
                                        }
                                    }
                                }
                            }
                            if !backs.is_empty() {
                                p { class: "mind-sub-h", "Linked from" }
                                for b in backs.iter() {
                                    {
                                        let from = txt(b, "from_id");
                                        rsx! {
                                            button { key: "{txt(b, \"id\")}", class: "mind-link-row is-back", r#type: "button", onclick: move |_| mind.open_item(from.clone()),
                                                span { class: "mind-link-label", "{txt(b, \"label\")}" }
                                                if let Some(c) = b["context"].as_str() { span { class: "mind-link-ctx", "{c}" } }
                                            }
                                        }
                                    }
                                }
                            }
                            div { class: "mind-link-find",
                                input {
                                    r#type: "search",
                                    placeholder: "Link to another item…",
                                    value: "{link_q}",
                                    oninput: move |e| {
                                        let q = e.value();
                                        link_q.set(q.clone());
                                        let mut live = mind.live;
                                        spawn(async move {
                                            if q.trim().len() < 2 { link_hits.set(Vec::new()); return; }
                                            if let Ok(v) = live.post_json("/api/tools/mind_search", json!({ "query": q, "limit": 6 })).await {
                                                link_hits.set(v["items"].as_array().cloned().unwrap_or_default());
                                            }
                                        });
                                    },
                                }
                                for h in link_hits().into_iter().filter(|h| h["id"] != id.as_str()) {
                                    {
                                        let to = txt(&h, "id");
                                        let label = h["title"].as_str().map(String::from).unwrap_or_else(|| txt(&h, "raw").chars().take(60).collect());
                                        rsx! {
                                            button { key: "{to}", class: "mind-link-hit", r#type: "button",
                                                onclick: move |_| {
                                                    let from = mind.open.peek().clone().unwrap_or_default();
                                                    mind.write("mind_link", json!({ "from_id": from, "to_id": to.clone() }));
                                                    link_q.set(String::new());
                                                    link_hits.set(Vec::new());
                                                },
                                                span { class: "mind-chip", "{kind_label(h[\"kind\"].as_str().unwrap_or(\"note\"))}" }
                                                "{label}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        section { class: "mind-block",
                            h4 { "Boards" }
                            for b in on_boards.iter() {
                                p { key: "{txt(b, \"card_id\")}", class: "mind-on-board", "On " b { "{txt(b, \"title\")}" } }
                            }
                            button { class: "mc-ghost", r#type: "button", onclick: move |_| boards_open.set(!boards_open()), Icon { name: IconName::Plus, size: 15 } "Add to board" }
                            if boards_open() {
                                div { class: "mind-board-pick",
                                    if boards.is_empty() { p { class: "mind-muted", "No boards yet. Make one in Visualization." } }
                                    for b in boards.iter() {
                                        {
                                            let bid = txt(b, "id");
                                            let name = txt(b, "title");
                                            let nm = name.clone();
                                            rsx! {
                                                button { key: "{bid}", class: "mind-board-btn", r#type: "button",
                                                    onclick: move |_| {
                                                        let id = mind.open.peek().clone().unwrap_or_default();
                                                        mind.write("mind_board_add", json!({ "id": id, "board_id": bid.clone() }));
                                                        boards_open.set(false);
                                                        state.toast(format!("Added to {nm}"));
                                                    },
                                                    "{name}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "mind-actions",
                            if inbox { button { class: "btn btn-primary", r#type: "button", onclick: move |_| set("inbox", json!(false)), "Keep" } }
                            button { class: "mc-ghost", r#type: "button", onclick: move |_| set("pinned", json!(!pinned)), if pinned { "Unpin" } else { "Pin" } }
                            button { class: "mc-ghost", r#type: "button", title: "Private items stay on your PC and are never sent to Claude", onclick: move |_| set("sensitive", json!(!sensitive)), Icon { name: IconName::Lock, size: 14 } if sensitive { "Private" } else { "Mark private" } }
                            button { class: "mc-ghost mc-danger", r#type: "button",
                                onclick: move |_| { set("archived", json!(true)); close(); },
                                "Archive"
                            }
                        }
                    }
                }
            }
        }
    }
}
