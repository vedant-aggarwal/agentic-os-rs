//! Mission Control data on the client: JSON from the `mission_*` tools, kept in signals.
//!
//! Writes go through the offline outbox (`Live::write`) with ids made on the device, are laid
//! over the local copy at once, and the view reloads from the server after the outbox flushes.

use dioxus::prelude::*;
use serde_json::{Value, json};

use crate::live::Live;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seg {
    Viz,
    Mind,
    Version,
    NewMe,
    Growth,
}

impl Seg {
    pub fn slug(self) -> &'static str {
        match self {
            Seg::Viz => "viz",
            Seg::Mind => "mind",
            Seg::Version => "version",
            Seg::NewMe => "newme",
            Seg::Growth => "growth",
        }
    }
    pub fn from_slug(s: &str) -> Option<Seg> {
        Some(match s {
            "viz" => Seg::Viz,
            "mind" => Seg::Mind,
            "version" => Seg::Version,
            "newme" => Seg::NewMe,
            "growth" => Seg::Growth,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy)]
pub struct Mc {
    pub seg: Signal<Seg>,
    pub overview: Signal<Value>,
    pub boards: Signal<Option<Vec<Value>>>,
    /// The open board: `{board, items, breadcrumb, path}`.
    pub board: Signal<Option<Value>>,
    pub open_board: Signal<Option<String>>,
    /// gallery | canvas
    pub mode: Signal<&'static str>,
    pub card: Signal<Option<String>>,
    pub adding: Signal<bool>,
    pub playing: Signal<bool>,
    pub busy: Signal<bool>,
    pub goals: Signal<Option<Vec<Value>>>,
    pub statements: Signal<Vec<Value>>,
    pub rules: Signal<Option<Vec<Value>>>,
    pub drops: Signal<Vec<Value>>,
    pub horizon: Signal<&'static str>,
    seq: Signal<u64>,
    live: Live,
}

impl Mc {
    pub fn new(live: Live) -> Mc {
        Mc {
            live,
            seg: Signal::new(Seg::Viz),
            overview: Signal::new(Value::Null),
            boards: Signal::new(None),
            board: Signal::new(None),
            open_board: Signal::new(None),
            mode: Signal::new("gallery"),
            card: Signal::new(None),
            adding: Signal::new(false),
            playing: Signal::new(false),
            busy: Signal::new(false),
            goals: Signal::new(None),
            statements: Signal::new(Vec::new()),
            rules: Signal::new(None),
            drops: Signal::new(Vec::new()),
            horizon: Signal::new("short"),
            seq: Signal::new(0),
        }
    }

    /// A 16-character id made on the device (same alphabet as the server's ids).
    pub fn new_id(&mut self) -> String {
        const A: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
        let n = {
            let mut s = self.seq.write();
            *s += 1;
            *s
        };
        let t = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) as u64;
        let mut x = (t ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15)) | 1;
        (0..16)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                A[(x % 32) as usize] as char
            })
            .collect()
    }

    async fn tool(live: &mut Live, name: &str, args: Value) -> Option<Value> {
        live.post_json(&format!("/api/tools/{name}"), args).await.ok()
    }

    pub async fn load_overview(&mut self) {
        let mut live = self.live;
        if let Some(v) = Self::tool(&mut live, "mission_overview", json!({})).await {
            self.overview.set(v);
        }
    }

    pub async fn load_boards(&mut self) {
        let mut live = self.live;
        if let Some(v) = Self::tool(&mut live, "mission_board_list", json!({})).await {
            self.boards.set(Some(v["boards"].as_array().cloned().unwrap_or_default()));
        }
    }

    pub async fn load_board(&mut self) {
        let Some(id) = self.open_board.peek().clone() else { return };
        let mut live = self.live;
        if let Some(v) = Self::tool(&mut live, "mission_board_get", json!({ "id": id })).await {
            if self.open_board.peek().as_deref() == Some(id.as_str()) {
                self.board.set(Some(v));
            }
        }
    }

    pub async fn load_version(&mut self) {
        let mut live = self.live;
        // Statements first: the decks render once goals arrive, so both land together.
        if let Some(v) = Self::tool(&mut live, "mission_identity_list", json!({})).await {
            self.statements.set(v["statements"].as_array().cloned().unwrap_or_default());
        }
        if let Some(v) = Self::tool(&mut live, "mission_goal_list", json!({})).await {
            self.goals.set(Some(v["goals"].as_array().cloned().unwrap_or_default()));
        }
    }

    pub async fn load_newme(&mut self) {
        let mut live = self.live;
        if let Some(v) = Self::tool(&mut live, "mission_rule_list", json!({})).await {
            self.rules.set(Some(v["rules"].as_array().cloned().unwrap_or_default()));
        }
        if let Some(v) = Self::tool(&mut live, "mission_drop_list", json!({})).await {
            self.drops.set(v["drops"].as_array().cloned().unwrap_or_default());
        }
    }

    /// Queue a write (offline-safe), then reload what it touches once it reached the server.
    pub fn write(&mut self, tool: &str, args: Value) {
        let mut live = self.live;
        live.write(tool, args);
        let mut me = *self;
        let tool = tool.to_string();
        dioxus::core::spawn_forever(async move {
            crate::live::net::sleep_ms(350).await;
            live.flush().await;
            crate::live::net::sleep_ms(150).await;
            if tool.starts_with("mission_board") {
                me.load_board().await;
                if tool == "mission_board_add" || tool == "mission_board_update" || tool == "mission_board_item_add" {
                    me.load_boards().await;
                }
            } else if tool.starts_with("mission_goal") || tool.starts_with("mission_identity") || tool == "mission_evidence_add" {
                me.load_version().await;
            } else {
                me.load_newme().await;
            }
            me.load_overview().await;
        });
    }

    /// Change one card in the local copy of the open board.
    pub fn patch_item(&mut self, id: &str, f: impl FnOnce(&mut Value)) {
        let mut guard = self.board.write();
        if let Some(it) = guard.as_mut().and_then(|b| b["items"].as_array_mut()).and_then(|items| items.iter_mut().find(|i| i["id"] == id)) {
            f(it);
        }
    }

    pub fn item(&self, id: &str) -> Option<Value> {
        self.board.read().as_ref()?["items"].as_array()?.iter().find(|i| i["id"] == id).cloned()
    }
}


/// URL of a board image: our server's media route with the device token, or the card's own link.
pub fn image_src(item: &Value) -> Option<String> {
    if let Some(a) = item["asset_id"].as_str().or(item["child_cover"].as_str()).or(item["cover"].as_str()) {
        let live = consume_context::<Live>();
        let conn = live.conn.peek().clone()?;
        return Some(format!("{}/api/media/{a}?token={}", conn.url.trim_end_matches('/'), conn.token));
    }
    item["url"].as_str().or(item["cover_url"].as_str()).filter(|_| item["node_kind"] == "image" || item["cover_url"].is_string()).map(String::from)
}

pub fn txt(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

/// Section of a goal or statement -> Aura pillar hue.
pub fn hue_of(section: &str) -> &'static str {
    match section {
        "physical" => "physical",
        "monetary" => "money",
        "mental" => "mind",
        _ => "me",
    }
}

pub fn section_name(section: &str) -> &'static str {
    match section {
        "physical" => "Physical",
        "mental" => "Mental",
        "monetary" => "Monetary",
        "social" => "Social",
        _ => "Other",
    }
}

/// "12 Nov" from an ISO time or date.
pub fn short_day(t: &str) -> String {
    chrono::NaiveDate::parse_from_str(t.get(..10).unwrap_or(t), "%Y-%m-%d").map(|d| d.format("%-d %b").to_string()).unwrap_or_default()
}

impl Mc {
    pub fn live(&self) -> Live {
        self.live
    }
}
