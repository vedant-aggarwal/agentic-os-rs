//! The Nora runtime: config, the merged thread, the engine switch and the chat turn worker.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use aos_core::Store;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::engine::{ClaudeCli, Engine, TurnRequest, TurnUpdate};
use crate::events::{Bus, NoraEvent};
use crate::grok::{GrokConfig, GrokWriter, NotYetWriter};
use crate::prompt;
use crate::store::{Msg, NewMsg, NoraStore, now_ms};

/// "What is he doing right now" from a calendar: `Some("Physics lecture")` when busy.
pub type CalendarFn = Arc<dyn Fn(DateTime<Local>) -> Option<String> + Send + Sync>;

/// Name of the device whose token Nora's Claude uses to reach our MCP endpoint.
pub const ENGINE_DEVICE: &str = "nora-engine";

#[derive(Clone)]
pub struct NoraConfig {
    pub db_path: PathBuf,
    /// The server's MCP endpoint (`http://127.0.0.1:8787/mcp`). `None` = turns run without tools.
    pub mcp_url: Option<String>,
    pub claude_bin: Option<PathBuf>,
    /// Chat model (`AOS_NORA_MODEL`); `None` = the CLI default.
    pub model: Option<String>,
    /// Cheap model for heartbeat and consolidation (`AOS_NORA_CHEAP_MODEL`, default `haiku`).
    pub cheap_model: String,
    /// How many recent messages each turn reads (`AOS_NORA_HISTORY`, default 40).
    pub history: usize,
    /// Empty working folder for the CLI (`~/.aos/nora`).
    pub workdir: PathBuf,
    /// His first name as Nora uses it (`AOS_OWNER_NAME`).
    pub owner: String,
    pub grok: GrokConfig,
    pub calendar: Option<CalendarFn>,
}

impl NoraConfig {
    pub fn from_env(db_path: PathBuf) -> NoraConfig {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        NoraConfig {
            db_path,
            mcp_url: None,
            claude_bin: aos_work::WorkConfig::from_env().claude_bin,
            model: env("AOS_NORA_MODEL"),
            cheap_model: env("AOS_NORA_CHEAP_MODEL").unwrap_or_else(|| "haiku".into()),
            history: env("AOS_NORA_HISTORY").and_then(|v| v.parse().ok()).unwrap_or(40),
            workdir: aos_core::paths::aos_home().join("nora"),
            owner: env("AOS_OWNER_NAME")
                .or_else(|| aos_core::seed::local_profile(&aos_core::paths::local_seed_path()).and_then(|p| p.name))
                .unwrap_or_else(|| "the owner".into()),
            grok: GrokConfig::from_env(),
            calendar: None,
        }
    }
}

/// Which engine answers a message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EngineChoice {
    /// Claude, which can hand over to Grok with `grok_ask`.
    #[default]
    Auto,
    Claude,
    Grok,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Hourly heartbeat (off by default).
    pub heartbeat_enabled: bool,
    /// Nightly memory consolidation with the cheap model (off by default).
    pub consolidation_enabled: bool,
}

pub(crate) struct Inner {
    pub cfg: NoraConfig,
    pub db: NoraStore,
    pub core: Arc<Store>,
    pub bus: Bus,
    pub engine: Arc<dyn Engine>,
    pub writer: Arc<dyn GrokWriter>,
    kick: mpsc::UnboundedSender<()>,
    kick_rx: Mutex<Option<mpsc::UnboundedReceiver<()>>>,
    mcp_config: Mutex<Option<String>>,
    pub shutdown: CancellationToken,
}

#[derive(Clone)]
pub struct Nora(pub(crate) Arc<Inner>);

static GLOBAL: OnceLock<Nora> = OnceLock::new();

impl Nora {
    /// Open the tables, without background tasks (tests use this with a fake engine).
    pub fn new(cfg: NoraConfig, engine: Arc<dyn Engine>) -> Result<Nora> {
        let core = Store::open(&cfg.db_path).with_context(|| format!("opening {}", cfg.db_path.display()))?;
        if core.was_created() {
            let _ = aos_core::seed::apply_local(&core, &aos_core::paths::local_seed_path());
        }
        let db = NoraStore::open(&cfg.db_path)?;
        db.fail_running_turns()?;
        let (kick, kick_rx) = mpsc::unbounded_channel();
        Ok(Nora(Arc::new(Inner {
            cfg,
            db,
            core: Arc::new(core),
            bus: Bus::new(),
            engine,
            writer: Arc::new(NotYetWriter),
            kick,
            kick_rx: Mutex::new(Some(kick_rx)),
            mcp_config: Mutex::new(None),
            shutdown: CancellationToken::new(),
        })))
    }

    /// Start everything: Claude engine, MCP token, turn worker, scheduler, Grok watcher.
    /// Call inside a Tokio runtime.
    pub fn start(cfg: NoraConfig) -> Result<Nora> {
        let engine = Arc::new(ClaudeCli { bin: cfg.claude_bin.clone(), cwd: cfg.workdir.clone() });
        let nora = Nora::new(cfg, engine)?;
        if let Err(e) = nora.ensure_mcp_token() {
            warn!("nora: no MCP token, turns run without tools: {e:#}");
        }
        nora.spawn_turn_worker();
        tokio::spawn(crate::scheduler::run(nora.clone()));
        tokio::spawn(crate::grok::watch(nora.clone()));
        info!("nora started");
        Ok(nora)
    }

    pub fn install_global(&self) {
        let _ = GLOBAL.set(self.clone());
    }

    pub fn global() -> Option<Nora> {
        GLOBAL.get().cloned()
    }

    pub fn stop(&self) {
        self.0.shutdown.cancel();
    }

    pub fn db(&self) -> &NoraStore {
        &self.0.db
    }

    pub fn core(&self) -> &Arc<Store> {
        &self.0.core
    }

    pub fn bus(&self) -> &Bus {
        &self.0.bus
    }

    pub fn config(&self) -> &NoraConfig {
        &self.0.cfg
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.0.shutdown.clone()
    }

    /// The Grok Bot write side (a stub until it is built).
    pub fn grok_writer(&self) -> &Arc<dyn GrokWriter> {
        &self.0.writer
    }

    pub fn engine(&self) -> &Arc<dyn Engine> {
        &self.0.engine
    }

    /// A fresh device token for the engine (the old one is revoked), and the MCP config with it.
    fn ensure_mcp_token(&self) -> Result<()> {
        let Some(url) = self.0.cfg.mcp_url.clone() else { return Ok(()) };
        for d in self.0.core.list_devices()? {
            if d.name == ENGINE_DEVICE && d.revoked_at.is_none() {
                self.0.core.revoke_device(&d.id)?;
            }
        }
        let paired = self.0.core.create_device_token(ENGINE_DEVICE, Some("server"))?;
        let cfg = json!({ "mcpServers": { "aos": { "type": "http", "url": url, "headers": { "Authorization": format!("Bearer {}", paired.token) } } } });
        *self.0.mcp_config.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg.to_string());
        Ok(())
    }

    fn mcp_config(&self) -> Option<String> {
        self.0.mcp_config.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    // --- thread ---------------------------------------------------------------------------

    /// Append a message and tell listeners. `None` when it was a duplicate external id.
    pub fn post(&self, m: NewMsg) -> Result<Option<Msg>> {
        let msg = self.0.db.append(&m)?;
        if let Some(msg) = &msg {
            self.0.bus.send(NoraEvent::Message { msg: msg.clone() });
        }
        Ok(msg)
    }

    /// A message from him. `auto`/`claude` start a Claude turn; `grok` queues it for Grok Bot.
    pub fn send(&self, text: &str, engine: EngineChoice, source: &str) -> Result<Msg> {
        let text = text.trim();
        anyhow::ensure!(!text.is_empty(), "message is empty");
        let kind = if engine == EngineChoice::Grok { "to_grok" } else { "message" };
        let msg = self.post(NewMsg::new("user", source, kind, text))?.context("message not stored")?;
        match engine {
            EngineChoice::Grok => {
                let item = self.0.db.queue_grok("message", text)?;
                self.0.bus.send(NoraEvent::GrokQueued { id: item.id, kind: item.kind });
            }
            _ => {
                let _ = self.0.kick.send(());
            }
        }
        Ok(msg)
    }

    pub fn thread(&self, limit: usize, before: Option<i64>) -> Result<Vec<Msg>> {
        self.0.db.recent(limit.clamp(1, 500), before)
    }

    pub fn settings(&self) -> Result<Settings> {
        let flag = |k: &str| -> Result<bool> { Ok(self.0.db.get_kv(k)?.as_deref() == Some("true")) };
        Ok(Settings { heartbeat_enabled: flag("heartbeat_enabled")?, consolidation_enabled: flag("consolidation_enabled")? })
    }

    pub fn set_settings(&self, s: &Settings) -> Result<Settings> {
        self.0.db.set_kv("heartbeat_enabled", &s.heartbeat_enabled.to_string())?;
        self.0.db.set_kv("consolidation_enabled", &s.consolidation_enabled.to_string())?;
        self.settings()
    }

    // --- chat turns -----------------------------------------------------------------------

    /// One worker runs turns one at a time; messages that arrive mid-turn go into the next one.
    pub fn spawn_turn_worker(&self) {
        let Some(mut rx) = self.0.kick_rx.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        let nora = self.clone();
        tokio::spawn(async move {
            // Answer anything left unanswered by a restart.
            let _ = nora.0.kick.send(());
            loop {
                tokio::select! {
                    _ = nora.0.shutdown.cancelled() => break,
                    k = rx.recv() => {
                        if k.is_none() { break }
                        while rx.try_recv().is_ok() {}
                        if let Err(e) = nora.run_turn().await {
                            warn!("nora turn: {e:#}");
                        }
                    }
                }
            }
        });
    }

    /// Run one Claude turn over the unanswered messages. Returns the turn id, if any ran.
    pub async fn run_turn(&self) -> Result<Option<String>> {
        let db = &self.0.db;
        let cfg = &self.0.cfg;
        let new = db.user_msgs_after(db.last_answered()?)?;
        if new.is_empty() {
            return Ok(None);
        }
        let upto = db.latest_msg_id()?;
        let turn_id = aos_core::ids::new_id();
        db.start_turn(&turn_id, "claude", "chat", upto)?;
        self.0.bus.send(NoraEvent::TurnStarted { turn_id: turn_id.clone() });

        let mut history = db.recent(cfg.history + 1, None)?;
        let more_before = history.len() > cfg.history;
        if more_before {
            history.remove(0);
        }
        let query = new.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join(" ");
        let memory = db.memory_context(&query, &cfg.owner)?;
        let req = TurnRequest {
            system: prompt::system_prompt(&cfg.owner),
            prompt: prompt::turn_prompt(now_ms(), &cfg.owner, &history, &new, &memory, more_before),
            model: cfg.model.clone(),
            mcp_config: self.mcp_config(),
            deny_tools: vec!["nora_send".into()],
            timeout: Some(Duration::from_secs(600)),
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        let run = self.0.engine.run(req, tx);
        let consume = async {
            let mut posted: Vec<i64> = Vec::new();
            while let Some(u) = rx.recv().await {
                match u {
                    TurnUpdate::Delta(text) => self.0.bus.send(NoraEvent::Delta { turn_id: turn_id.clone(), text }),
                    TurnUpdate::Tool { name, title, done } => {
                        self.0.bus.send(NoraEvent::Tool { turn_id: turn_id.clone(), name, title, done })
                    }
                    TurnUpdate::Text(text) => {
                        let kind = if posted.is_empty() { "ack" } else { "progress" };
                        let mut m = NewMsg::new("claude", "app", kind, text.trim());
                        m.turn_id = Some(turn_id.clone());
                        match self.post(m) {
                            Ok(Some(msg)) => posted.push(msg.id),
                            Ok(None) => {}
                            Err(e) => warn!("nora: storing reply: {e:#}"),
                        }
                    }
                }
            }
            posted
        };
        let (out, posted) = tokio::join!(run, consume);

        if let Some(last) = posted.last()
            && let Some(msg) = db.set_msg_kind(*last, "final")?
        {
            self.0.bus.send(NoraEvent::MessageUpdated { msg });
        }
        if !out.ok {
            let err = out.error.clone().unwrap_or_else(|| "unknown error".into());
            let mut m = NewMsg::new("system", "claude", "error", format!("Claude turn failed: {}", prompt::clip(&err, 400)));
            m.turn_id = Some(turn_id.clone());
            self.post(m)?;
        }
        let summary = summarize(&cfg.owner, &new, out.final_text(), &out.tools);
        let status = if out.ok { "done" } else { "failed" };
        if let Some(turn) = db.end_turn(&turn_id, status, &summary, &out.tools, out.error.as_deref(), out.cost_usd)? {
            self.0.bus.send(NoraEvent::TurnDone { turn });
        }
        if out.ok {
            // Grok keeps context from these once the write side exists.
            let item = db.queue_grok("summary", &summary)?;
            self.0.bus.send(NoraEvent::GrokQueued { id: item.id, kind: item.kind });
        }
        Ok(Some(turn_id))
    }
}

/// A 1-3 line summary of a turn for the Grok side, built without another model call.
pub fn summarize(owner: &str, asked: &[Msg], answer: Option<&str>, tools: &[String]) -> String {
    let one_line = |s: &str, n: usize| prompt::clip(&s.split_whitespace().collect::<Vec<_>>().join(" "), n);
    let asked = asked.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join(" / ");
    let mut s = format!("[Claude Nora] {owner}: {}", one_line(&asked, 160));
    if let Some(a) = answer {
        s.push_str(&format!("\nNora: {}", one_line(a, 200)));
    }
    if !tools.is_empty() {
        let mut t = tools.to_vec();
        t.dedup();
        s.push_str(&format!("\nTools: {}", t.join(", ")));
    }
    s
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::engine::TurnOutcome;
    use futures::future::BoxFuture;

    /// Replies with the given text blocks, after a tool call when `tool` is set.
    pub struct FakeEngine {
        pub texts: Vec<String>,
        pub tool: Option<String>,
        pub seen: Mutex<Vec<TurnRequest>>,
    }

    impl Engine for FakeEngine {
        fn run(&self, req: TurnRequest, updates: mpsc::UnboundedSender<TurnUpdate>) -> BoxFuture<'static, TurnOutcome> {
            self.seen.lock().unwrap().push(req);
            let texts = self.texts.clone();
            let tool = self.tool.clone();
            Box::pin(async move {
                let mut out = TurnOutcome { ok: true, ..Default::default() };
                for (i, t) in texts.iter().enumerate() {
                    let _ = updates.send(TurnUpdate::Delta(t.clone()));
                    let _ = updates.send(TurnUpdate::Text(t.clone()));
                    out.texts.push(t.clone());
                    if i == 0 && let Some(name) = &tool {
                        let _ = updates.send(TurnUpdate::Tool { name: name.clone(), title: name.clone(), done: false });
                        out.tools.push(name.clone());
                    }
                }
                out
            })
        }
    }

    pub fn nora_with(texts: &[&str], tool: Option<&str>) -> (Nora, Arc<FakeEngine>, PathBuf) {
        let path = std::env::temp_dir().join(format!("aos-nora-{}.db", aos_core::ids::new_id()));
        let mut cfg = NoraConfig::from_env(path.clone());
        cfg.grok.dir = None;
        cfg.owner = "V".into();
        let engine = Arc::new(FakeEngine { texts: texts.iter().map(|s| s.to_string()).collect(), tool: tool.map(String::from), seen: Mutex::new(vec![]) });
        (Nora::new(cfg, engine.clone()).unwrap(), engine, path)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[tokio::test]
    async fn turn_posts_ack_then_final_and_queues_summary() {
        let (nora, engine, _p) = nora_with(&["On it.", "Alarm set for 06:00."], Some("alarm_set"));
        let mut events = nora.bus().subscribe();
        nora.send("wake me at 6", EngineChoice::Auto, "app").unwrap();
        let turn = nora.run_turn().await.unwrap();
        assert!(turn.is_some());
        let msgs = nora.thread(10, None).unwrap();
        let kinds: Vec<&str> = msgs.iter().map(|m| m.kind.as_str()).collect();
        assert_eq!(kinds, vec!["message", "ack", "final"]);
        assert_eq!(msgs[2].engine, "claude");
        // The prompt carried the message with a timestamp.
        let prompt = engine.seen.lock().unwrap()[0].prompt.clone();
        assert!(prompt.contains("V: wake me at 6"));
        assert!(prompt.contains("Now: 20"));
        let q = nora.db().grok_queue(10).unwrap();
        assert_eq!(q.len(), 1);
        assert!(q[0].text.contains("Tools: alarm_set"));
        // Nothing new: no second turn.
        assert!(nora.run_turn().await.unwrap().is_none());
        let mut saw_typing = false;
        while let Ok(e) = events.try_recv() {
            saw_typing |= matches!(e, NoraEvent::TurnStarted { .. });
        }
        assert!(saw_typing);
    }

    #[tokio::test]
    async fn grok_choice_queues_and_skips_claude() {
        let (nora, engine, _p) = nora_with(&["hi"], None);
        nora.send("ask grok about the scouts", EngineChoice::Grok, "app").unwrap();
        assert!(nora.run_turn().await.unwrap().is_none());
        assert!(engine.seen.lock().unwrap().is_empty());
        assert_eq!(nora.db().grok_queue(10).unwrap()[0].kind, "message");
    }

    #[test]
    fn settings_default_off() {
        let (nora, _e, _p) = nora_with(&[], None);
        assert_eq!(nora.settings().unwrap(), Settings::default());
        let s = nora.set_settings(&Settings { heartbeat_enabled: true, consolidation_enabled: false }).unwrap();
        assert!(s.heartbeat_enabled);
    }
}
