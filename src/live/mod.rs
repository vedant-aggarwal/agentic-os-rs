//! The client data layer: pairing, the cached day, the offline outbox and Nora's live thread.
//!
//! Reads come from server tools (`today`, `profile`, `area_list`, `journal_read`, `inbox_list`,
//! `xp_summary`) and are cached on the device, so the app opens with yesterday's view when the
//! server is out of reach. Writes are applied to the local view first, queued in a persisted
//! outbox and pushed with `/api/sync/push`; the server de-duplicates by `op_id`, so a resend after
//! a dropped connection never acts twice. A failed request never clears the pairing: only
//! "Unpair" in You does.

pub mod net;
pub mod types;

use dioxus::prelude::*;
use serde_json::{Value, json};

pub use net::Conn;
use net::NetError;
pub use types::*;

/// Name, avatar and goals: bundled at build time, replaced by the server's `profile` once paired.
pub static PROFILE: GlobalSignal<Profile> = Signal::global(Profile::bundled);
/// Overall level from the server's XP ledger.
pub static LEVEL: GlobalSignal<Level> = Signal::global(Level::default);
/// Level per pillar id (body, wealth, mind).
pub static PILLAR_LEVELS: GlobalSignal<Vec<PillarXp>> = Signal::global(Vec::new);

pub fn profile_name() -> String {
    PROFILE.read().display_name()
}

pub fn level() -> Level {
    LEVEL.read().clone()
}

pub fn pillar_level(pillar_id: &str) -> u32 {
    PILLAR_LEVELS.read().iter().find(|p| p.id == pillar_id).map_or(1, |p| p.level.max(1))
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum InboxState {
    #[default]
    Loading,
    /// The server has no inbox tools yet.
    NoTool,
    Items(Vec<InboxItem>),
    Failed(String),
}

#[derive(Clone, Copy)]
pub struct Live {
    pub conn: Signal<Option<Conn>>,
    /// Storage read finished (the pairing screen waits for this).
    pub ready: Signal<bool>,
    /// "Not now" on the pairing screen, for this launch.
    pub pair_skipped: Signal<bool>,
    /// Re-pair asked for from You → Server.
    pub pair_open: Signal<bool>,
    pub online: Signal<bool>,
    /// The server refused the token (401). The pairing stays; Settings offers re-pair.
    pub auth_bad: Signal<bool>,
    pub today: Signal<Option<TodayView>>,
    pub areas: Signal<AreaTree>,
    pub journal: Signal<Option<Vec<JournalEntry>>>,
    pub inbox: Signal<InboxState>,
    pub inbox_tasks: Signal<Vec<TaskNode>>,
    pub xp: Signal<XpSummary>,
    pub outbox: Signal<Vec<Op>>,
    pub nora: Signal<Vec<NoraMsg>>,
    /// Reply being written right now (streamed deltas). `Some("")` = typing.
    pub nora_live: Signal<Option<String>>,
    pub nora_engine: Signal<&'static str>,
    pub nora_error: Signal<Option<String>>,
    flushing: Signal<bool>,
    seq: Signal<u64>,
}

impl Live {
    fn new() -> Live {
        Live {
            conn: Signal::new(None),
            ready: Signal::new(false),
            pair_skipped: Signal::new(false),
            pair_open: Signal::new(false),
            online: Signal::new(false),
            auth_bad: Signal::new(false),
            today: Signal::new(None),
            areas: Signal::new(AreaTree::default()),
            journal: Signal::new(None),
            inbox: Signal::new(InboxState::Loading),
            inbox_tasks: Signal::new(Vec::new()),
            xp: Signal::new(XpSummary::default()),
            outbox: Signal::new(Vec::new()),
            nora: Signal::new(Vec::new()),
            nora_live: Signal::new(None),
            nora_engine: Signal::new("auto"),
            nora_error: Signal::new(None),
            flushing: Signal::new(false),
            seq: Signal::new(0),
        }
    }

    pub fn paired(&self) -> bool {
        self.conn.read().is_some()
    }

    fn conn_now(&self) -> Option<Conn> {
        self.conn.peek().clone()
    }

    fn new_op_id(&mut self) -> String {
        let n = {
            let mut s = self.seq.write();
            *s += 1;
            *s
        };
        let t = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        format!("op-{t:x}-{n:x}-{:x}", (t as u64).wrapping_mul(2654435761) % 0xffff)
    }

    // -----------------------------------------------------------------------------------------
    // Pairing

    pub async fn pair(&mut self, url: &str, code: &str, name: &str) -> Result<(), String> {
        let conn = net::pair(url, code, name, net::platform()).await.map_err(|e| match e {
            NetError::Unauthorized => "That code is wrong, used or expired. Make a new one with `aos pair new`.".to_string(),
            other => other.message(),
        })?;
        net::save("conn", &serde_json::to_string(&conn).unwrap_or_default());
        net::install_for_server_fns(&conn);
        self.conn.set(Some(conn));
        self.auth_bad.set(false);
        self.online.set(true);
        self.pair_open.set(false);
        self.after_connect();
        Ok(())
    }

    /// Explicit unpair: revoke this device on the server (best effort) and forget the token.
    pub async fn unpair(&mut self) {
        if let Some(c) = self.conn_now() {
            let _ = net::post(&c, &format!("/api/devices/{}/revoke", c.device_id), &json!({})).await;
        }
        net::remove("conn");
        net::clear_server_fn_auth();
        self.conn.set(None);
        self.pair_skipped.set(false);
        self.online.set(false);
    }

    fn after_connect(&self) {
        let mut me = *self;
        spawn(async move {
            me.flush().await;
            me.refresh().await;
        });
        self.start_nora_stream();
    }

    // -----------------------------------------------------------------------------------------
    // Reads

    fn note_err(&mut self, e: &NetError) {
        match e {
            NetError::Offline(_) => self.online.set(false),
            NetError::Unauthorized => self.auth_bad.set(true),
            _ => {}
        }
    }

    async fn call(&mut self, tool: &str, args: Value) -> Result<Value, NetError> {
        let Some(c) = self.conn_now() else {
            return Err(NetError::Offline("not paired".into()));
        };
        let r = net::tool(&c, tool, args).await;
        match &r {
            Ok(_) => {
                self.online.set(true);
                if *self.auth_bad.peek() {
                    self.auth_bad.set(false);
                }
            }
            Err(e) => self.note_err(e),
        }
        r
    }

    /// Fetch the day, profile, areas and XP. Pending outbox writes are laid over the result.
    pub async fn refresh(&mut self) {
        if self.conn_now().is_none() {
            return;
        }
        if let Ok(v) = self.call("today", json!({})).await {
            if let Ok(mut t) = serde_json::from_value::<TodayView>(v.clone()) {
                net::save("today", &v.to_string());
                for op in self.outbox.peek().iter() {
                    apply_local(&mut t, op);
                }
                *LEVEL.write() = t.xp.level.clone();
                self.today.set(Some(t));
            }
        }
        if let Ok(v) = self.call("profile", json!({})).await {
            if let Ok(p) = serde_json::from_value::<Profile>(v) {
                let merged = PROFILE.peek().merged_with(p);
                net::save("profile", &serde_json::to_string(&merged).unwrap_or_default());
                *PROFILE.write() = merged;
            }
        }
        if let Ok(v) = self.call("area_list", json!({})).await {
            if let Ok(a) = serde_json::from_value::<AreaTree>(v.clone()) {
                net::save("areas", &v.to_string());
                self.areas.set(a);
            }
        }
        if let Ok(v) = self.call("xp_summary", json!({})).await {
            if let Ok(x) = serde_json::from_value::<XpSummary>(v) {
                *PILLAR_LEVELS.write() = x.pillars.clone();
                self.xp.set(x);
            }
        }
    }

    pub async fn load_journal(&mut self) {
        if let Ok(v) = self.call("journal_read", json!({ "days": 60, "limit": 100 })).await {
            let list = v.get("entries").cloned().unwrap_or(v);
            if let Ok(e) = serde_json::from_value::<Vec<JournalEntry>>(list) {
                net::save("journal", &serde_json::to_string(&e).unwrap_or_default());
                self.journal.set(Some(e));
            }
        } else if self.journal.peek().is_none() {
            self.journal.set(Some(Vec::new()));
        }
    }

    pub async fn load_inbox(&mut self) {
        match self.call("inbox_list", json!({ "limit": 50 })).await {
            Ok(v) => {
                let items = serde_json::from_value::<Vec<InboxItem>>(v["items"].clone()).unwrap_or_default();
                self.inbox.set(InboxState::Items(items));
            }
            Err(e) if e.is_not_found_tool() => self.inbox.set(InboxState::NoTool),
            Err(NetError::Offline(_)) if self.conn_now().is_none() => self.inbox.set(InboxState::Failed("Pair with your server to see the inbox.".into())),
            Err(e) => self.inbox.set(InboxState::Failed(e.message())),
        }
        // Inbox tasks: open tasks with no area yet.
        if let Ok(v) = self.call("task_list", json!({ "status": "open", "limit": 100 })).await {
            let list = v.get("tasks").cloned().unwrap_or(v);
            if let Ok(nodes) = serde_json::from_value::<Vec<TaskNode>>(list) {
                self.inbox_tasks.set(nodes.into_iter().filter(|n| n.task.area_id.is_none()).collect());
            }
        }
    }

    /// Open tasks of one area (Life → area page).
    pub async fn area_tasks(&mut self, area: &str) -> Vec<TaskNode> {
        match self.call("task_list", json!({ "area": area, "limit": 100 })).await {
            Ok(v) => serde_json::from_value(v.get("tasks").cloned().unwrap_or(v)).unwrap_or_default(),
            Err(_) => self
                .today
                .peek()
                .as_ref()
                .map(|t| t.areas.iter().filter(|a| a.area_id.as_deref() == Some(area)).flat_map(|a| a.tasks.clone()).collect())
                .unwrap_or_default(),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Writes: local first, then the outbox

    /// Apply a write to the local view, queue it and try to send it.
    pub fn write(&mut self, tool: &str, args: Value) {
        let op = Op { op_id: self.new_op_id(), tool: tool.to_string(), args };
        if let Some(t) = self.today.write().as_mut() {
            apply_local(t, &op);
        }
        self.outbox.write().push(op);
        self.persist_outbox();
        let mut me = *self;
        spawn(async move { me.flush().await });
    }

    fn persist_outbox(&self) {
        net::save("outbox", &serde_json::to_string(&*self.outbox.peek()).unwrap_or_default());
    }

    /// Send queued writes in order. Keeps them on network errors; drops ones the server rejects
    /// for good (and says why).
    pub async fn flush(&mut self) {
        let Some(c) = self.conn_now() else { return };
        if *self.flushing.peek() || self.outbox.peek().is_empty() {
            return;
        }
        self.flushing.set(true);
        let batch: Vec<Op> = self.outbox.peek().iter().take(50).cloned().collect();
        match net::post(&c, "/api/sync/push", &json!({ "ops": batch })).await {
            Ok(v) => {
                self.online.set(true);
                self.auth_bad.set(false);
                let results = v["results"].as_array().cloned().unwrap_or_default();
                let mut problems = Vec::new();
                let done: Vec<String> = results
                    .iter()
                    .filter_map(|r| {
                        let id = r["op_id"].as_str()?.to_string();
                        if r["outcome"]["ok"] == json!(false) {
                            problems.push(r["outcome"]["error"]["message"].as_str().unwrap_or("rejected").to_string());
                        }
                        Some(id)
                    })
                    .collect();
                self.outbox.write().retain(|op| !done.contains(&op.op_id));
                self.persist_outbox();
                if let Some(p) = problems.first() {
                    self.nora_error.set(None);
                    toast_global(format!("Not saved: {p}"));
                }
                self.flushing.set(false);
                let more = !self.outbox.peek().is_empty();
                self.refresh().await;
                if more {
                    Box::pin(self.flush()).await;
                }
                return;
            }
            Err(e) => self.note_err(&e),
        }
        self.flushing.set(false);
    }

    pub fn tick_task(&mut self, id: &str) {
        self.write("task_done", json!({ "id": id }));
    }

    pub fn skip_task(&mut self, id: &str, reason: &str) {
        self.write("task_skip", json!({ "id": id, "reason": reason, "genuine": true }));
    }

    pub fn add_task(&mut self, title: &str, area: Option<String>, parent: Option<String>) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut args = json!({ "title": title, "due": today });
        if let Some(a) = area {
            args["area"] = json!(a);
        }
        if let Some(p) = parent {
            args["parent"] = json!(p);
        }
        self.write("task_add", args);
    }

    pub fn log_habit(&mut self, id: &str, status: &str) {
        self.write("habit_log", json!({ "habit": id, "status": status }));
    }

    pub fn add_habit(&mut self, name: &str, area: Option<String>) {
        let mut args = json!({ "name": name });
        if let Some(a) = area {
            args["area"] = json!(a);
        }
        self.write("habit_add", args);
    }

    pub fn log_mood(&mut self, valence: f64, energy: i64, reasons: Vec<String>) {
        self.write("mood_log", json!({ "valence": valence, "energy": energy, "reasons": reasons }));
    }

    pub fn send_nora(&mut self, text: &str) {
        let engine = *self.nora_engine.peek();
        self.write("nora_send", json!({ "text": text, "engine": engine }));
    }

    /// Messages typed here that the server has not taken yet.
    pub fn nora_pending(&self) -> Vec<String> {
        self.outbox
            .read()
            .iter()
            .filter(|o| o.tool == "nora_send")
            .filter_map(|o| o.args["text"].as_str().map(str::to_string))
            .collect()
    }

    // -----------------------------------------------------------------------------------------
    // Nora

    pub async fn load_nora(&mut self) {
        let Some(c) = self.conn_now() else { return };
        match net::get(&c, "/api/nora/thread?limit=80").await {
            Ok(v) => {
                let mut msgs: Vec<NoraMsg> = serde_json::from_value(v).unwrap_or_default();
                msgs.sort_by_key(|m| m.id);
                net::save("nora", &serde_json::to_string(&msgs).unwrap_or_default());
                self.nora.set(msgs);
                self.nora_error.set(None);
            }
            Err(e) => {
                self.note_err(&e);
                self.nora_error.set(Some(e.message()));
            }
        }
    }

    fn upsert_msg(&mut self, m: NoraMsg) {
        let mut list = self.nora.write();
        match list.iter_mut().find(|x| x.id == m.id) {
            Some(x) => *x = m,
            None => {
                list.push(m);
                list.sort_by_key(|m| m.id);
            }
        }
    }

    /// Follow `/api/nora/events` for as long as the app runs; reconnects with a backoff.
    fn start_nora_stream(&self) {
        static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let mut me = *self;
        spawn(async move {
            let mut wait = 1000u32;
            loop {
                let Some(c) = me.conn_now() else {
                    net::sleep_ms(3000).await;
                    continue;
                };
                me.load_nora().await;
                let r = net::sse(&c, "/api/nora/events", |ev| {
                    wait = 1000;
                    match ev["type"].as_str().unwrap_or("") {
                        "message" | "message_updated" => {
                            if let Ok(m) = serde_json::from_value::<NoraMsg>(ev["msg"].clone()) {
                                let ends = m.engine != "user" && m.kind == "final";
                                me.upsert_msg(m);
                                if ends {
                                    me.nora_live.set(None);
                                }
                            }
                        }
                        "turn_started" => me.nora_live.set(Some(String::new())),
                        "delta" => {
                            let t = ev["text"].as_str().unwrap_or("").to_string();
                            let mut live = me.nora_live.write();
                            live.get_or_insert_with(String::new).push_str(&t);
                        }
                        "turn_done" => me.nora_live.set(None),
                        "resync" => {
                            let mut m2 = me;
                            spawn(async move { m2.load_nora().await });
                        }
                        _ => {}
                    }
                })
                .await;
                if let Err(e) = &r {
                    me.note_err(e);
                    if *e == NetError::Unauthorized {
                        wait = 30_000;
                    }
                }
                net::sleep_ms(wait).await;
                wait = (wait * 2).min(30_000);
            }
        });
    }
}

/// The write as the server will apply it, on the local copy of the day.
fn apply_local(t: &mut TodayView, op: &Op) {
    let a = &op.args;
    let id = a["id"].as_str().unwrap_or("");
    let mut set_task = |f: &mut dyn FnMut(&mut Task)| {
        for g in t.areas.iter_mut() {
            for n in g.tasks.iter_mut() {
                if n.task.id == id {
                    f(&mut n.task);
                }
                for s in n.subtasks.iter_mut() {
                    if s.id == id {
                        f(s);
                    }
                }
            }
        }
    };
    match op.tool.as_str() {
        "task_done" => set_task(&mut |x| x.status = "done".into()),
        "task_skip" => set_task(&mut |x| {
            x.status = "skipped".into();
            x.skip_reason = a["reason"].as_str().map(str::to_string);
        }),
        "task_add" => {
            let task = Task {
                id: format!("local-{}", op.op_id),
                title: a["title"].as_str().unwrap_or("").to_string(),
                area_id: a["area"].as_str().map(str::to_string),
                parent_id: a["parent"].as_str().map(str::to_string),
                status: "open".into(),
                xp_weight: 1,
                ..Default::default()
            };
            if let Some(parent) = task.parent_id.clone() {
                for g in t.areas.iter_mut() {
                    if let Some(n) = g.tasks.iter_mut().find(|n| n.task.id == parent) {
                        n.subtasks.push(task.clone());
                        return;
                    }
                }
            }
            match t.areas.iter_mut().find(|g| g.area_id == task.area_id) {
                Some(g) => g.tasks.push(TaskNode { task, ..Default::default() }),
                None => t.areas.push(AreaTasks { area_id: task.area_id.clone(), tasks: vec![TaskNode { task, ..Default::default() }], ..Default::default() }),
            }
        }
        "habit_log" => {
            let key = a["habit"].as_str().unwrap_or("");
            let status = a["status"].as_str().unwrap_or("done").to_string();
            if let Some(h) = t.habits.iter_mut().find(|h| h.id == key || h.name == key) {
                let was = h.counts();
                h.today = Some(status);
                if !was && h.counts() {
                    h.streak.current += 1;
                    h.streak.best = h.streak.best.max(h.streak.current);
                    h.streak.state = "intact".into();
                }
            }
        }
        "habit_add" => t.habits.push(HabitView {
            id: format!("local-{}", op.op_id),
            name: a["name"].as_str().unwrap_or("").to_string(),
            area_id: a["area"].as_str().map(str::to_string),
            due_today: true,
            xp: 5,
            streak: Streak { state: "broken".into(), ..Default::default() },
            ..Default::default()
        }),
        "mood_log" => t.moods.push(Mood {
            id: format!("local-{}", op.op_id),
            at: chrono::Utc::now().to_rfc3339(),
            valence: a["valence"].as_f64().unwrap_or(0.0),
            energy: a["energy"].as_i64(),
            reasons: serde_json::from_value(a["reasons"].clone()).unwrap_or_default(),
            ..Default::default()
        }),
        _ => {}
    }
}

/// Toasts from outside a component: the shell picks this up.
pub static TOAST: GlobalSignal<Option<String>> = Signal::global(|| None);

fn toast_global(text: String) {
    *TOAST.write() = Some(text);
}

/// Create the data layer, restore what the device kept, connect, and keep it fresh.
pub fn use_live_provider() -> Live {
    let live = use_context_provider(Live::new);
    use_effect(move || {
        let mut me = live;
        spawn(async move {
            if let Some(p) = net::load("profile").await.and_then(|s| serde_json::from_str::<Profile>(&s).ok()) {
                let merged = PROFILE.peek().merged_with(p);
                *PROFILE.write() = merged;
            }
            if let Some(t) = net::load("today").await.and_then(|s| serde_json::from_str::<TodayView>(&s).ok()) {
                *LEVEL.write() = t.xp.level.clone();
                me.today.set(Some(t));
            }
            if let Some(a) = net::load("areas").await.and_then(|s| serde_json::from_str::<AreaTree>(&s).ok()) {
                me.areas.set(a);
            }
            if let Some(j) = net::load("journal").await.and_then(|s| serde_json::from_str::<Vec<JournalEntry>>(&s).ok()) {
                me.journal.set(Some(j));
            }
            if let Some(n) = net::load("nora").await.and_then(|s| serde_json::from_str::<Vec<NoraMsg>>(&s).ok()) {
                me.nora.set(n);
            }
            if let Some(o) = net::load("outbox").await.and_then(|s| serde_json::from_str::<Vec<Op>>(&s).ok()) {
                if let Some(t) = me.today.write().as_mut() {
                    for op in &o {
                        apply_local(t, op);
                    }
                }
                me.outbox.set(o);
            }
            let conn = net::load("conn").await.and_then(|s| serde_json::from_str::<Conn>(&s).ok());
            if let Some(c) = &conn {
                net::install_for_server_fns(c);
            }
            me.conn.set(conn.clone());
            me.ready.set(true);
            if conn.is_some() {
                me.after_connect();
            }
            // Keep fresh: send the outbox and re-read every 20 s while paired.
            loop {
                net::sleep_ms(20_000).await;
                if me.conn_now().is_some() {
                    me.flush().await;
                    me.refresh().await;
                }
            }
        });
    });
    live
}
