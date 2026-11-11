//! Nora's voice calls (ARCHITECTURE.md §5, Calls).
//!
//! - **Voice**: Codex realtime on the owner's ChatGPT login ([`crate::realtime`]). One relay is
//!   kept warm, so answering waits only on the WebRTC handshake.
//! - **Hand-offs run as Claude turns.** When the voice hands a job off, the Codex turn is stopped
//!   at once and the job runs through Nora's Claude engine with our MCP tools. The result is
//!   spoken into the call. She is never silent for long: a nudge after 8 s, again every 25 s, and
//!   a hard stop at 90 s that says so.
//! - **Context at start**: today's plan, check-ins and mood, Mission (level, XP, goals), the Nora
//!   thread with timestamps, memory, and the last call write-ups, in the voice's developer item.
//! - **Write-up** at the end: one Claude turn, capped, with a transcript fallback. It goes into
//!   the Nora thread and the journal (a close-the-day call earns the journal; a quick call is
//!   kept as kind `call`, no XP).
//! - **21:00 close-the-day call**: the scheduler rings in the app once a day ([`close_day_tick`]).
//! - **[`OrbState`]**: his state for the orb's colour, from mood, energy and today's load.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Local, NaiveTime, Timelike};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::engine::{TurnRequest, TurnUpdate};
use crate::events::NoraEvent;
use crate::nora::Nora;
use crate::prompt;
use crate::realtime::{self, Note, Prepared, RelayConfig, Rpc};
use crate::store::{NewMsg, now_ms};

pub const STILL_LINE: &str = "Still on it, one moment.";
pub const FAILED_LINE: &str = "Sorry, that didn't work. Ask me again in a bit.";
pub const SLOW_LINE: &str = "That's taking too long, so I stopped it. I'll put it in chat.";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS nora_voice_call (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,                -- quick | close_day
    reason      TEXT NOT NULL DEFAULT '',
    ring_id     INTEGER,
    status      TEXT NOT NULL,                -- live | ended | failed
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER,
    seconds     INTEGER NOT NULL DEFAULT 0,
    transcript  TEXT NOT NULL DEFAULT '[]',
    handoffs    INTEGER NOT NULL DEFAULT 0,
    summary     TEXT NOT NULL DEFAULT '',
    actions     TEXT NOT NULL DEFAULT '[]',
    writeup     TEXT NOT NULL DEFAULT 'none', -- none | running | done | failed
    end_reason  TEXT NOT NULL DEFAULT ''
);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CallKind {
    #[default]
    Quick,
    CloseDay,
}

impl CallKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CallKind::Quick => "quick",
            CallKind::CloseDay => "close_day",
        }
    }
}

/// What the UI sees of a call.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CallView {
    pub id: String,
    pub kind: CallKind,
    pub reason: String,
    /// `connecting`, `live`, `ended` or `failed`.
    pub status: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub seconds: i64,
    /// Hand-offs running right now (the orb shows thinking while this is above zero).
    pub thinking: usize,
    pub handoffs: i64,
    pub summary: String,
    pub actions: Vec<String>,
    /// `none`, `running`, `done` or `failed`.
    pub writeup: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Line {
    pub who: String,
    pub text: String,
    pub at: i64,
}

#[derive(Clone)]
pub struct CallConfig {
    /// `None`: no codex binary, calls can't start.
    pub relay: Option<RelayConfig>,
    pub voice: String,
    /// Claude model for hand-offs and write-ups (`AOS_NORA_CALL_MODEL`, default `sonnet`: speed matters on a call).
    pub model: Option<String>,
    /// Keep one relay warm (`AOS_CALL_WARM=off` to disable).
    pub warm: bool,
    pub nudge: Duration,
    pub nudge_again: Duration,
    pub handoff_max: Duration,
    pub writeup_max: Duration,
    pub idle: Duration,
}

impl CallConfig {
    pub fn from_env() -> CallConfig {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let secs = |k: &str, d: u64| Duration::from_secs(env(k).and_then(|v| v.parse().ok()).unwrap_or(d));
        let workdir = aos_core::paths::aos_home().join("nora");
        let relay = aos_work::WorkConfig::from_env().codex_bin.map(|bin| {
            let home = realtime::ensure_codex_home(&workdir.join("codex-home"), &realtime::owner_codex_home());
            RelayConfig {
                bin,
                cwd: workdir.join("call"),
                codex_home: home,
                config: vec![
                    format!("model_reasoning_effort={}", env("AOS_CALL_RELAY_EFFORT").unwrap_or_else(|| "\"low\"".into())),
                ],
            }
        });
        CallConfig {
            relay,
            voice: env("AOS_NORA_VOICE").unwrap_or_else(|| realtime::DEFAULT_VOICE.into()),
            model: Some(env("AOS_NORA_CALL_MODEL").unwrap_or_else(|| "sonnet".into())).filter(|m| m != "default"),
            warm: env("AOS_CALL_WARM").map(|v| v != "off" && v != "0").unwrap_or(true),
            nudge: secs("AOS_CALL_NUDGE_SECS", 8),
            nudge_again: secs("AOS_CALL_NUDGE_AGAIN_SECS", 25),
            handoff_max: secs("AOS_CALL_HANDOFF_SECS", 90),
            writeup_max: secs("AOS_CALL_WRITEUP_SECS", 180),
            idle: secs("AOS_CALL_IDLE_SECS", 240),
        }
    }
}

struct LiveCall {
    id: String,
    kind: CallKind,
    reason: String,
    started_ms: i64,
    rpc: Arc<Rpc>,
    thread_id: String,
    transcript: Mutex<Vec<Line>>,
    opener: Mutex<Option<String>>,
    thinking: AtomicUsize,
    handoffs: AtomicUsize,
    last_heard: Mutex<Instant>,
    ended: AtomicBool,
    cancel: CancellationToken,
}

impl LiveCall {
    fn heard(&self) {
        *self.last_heard.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    fn view(&self, status: &str) -> CallView {
        CallView {
            id: self.id.clone(),
            kind: self.kind,
            reason: self.reason.clone(),
            status: status.into(),
            started_at: self.started_ms,
            thinking: self.thinking.load(Ordering::SeqCst),
            handoffs: self.handoffs.load(Ordering::SeqCst) as i64,
            writeup: "none".into(),
            ..Default::default()
        }
    }

    async fn say(&self, text: &str) {
        let t = text.trim();
        if t.is_empty() || self.ended.load(Ordering::SeqCst) {
            return;
        }
        if let Err(e) = self
            .rpc
            .request("thread/realtime/appendSpeech", json!({ "threadId": self.thread_id, "text": t }), Duration::from_secs(15))
            .await
        {
            warn!("nora call: speak failed: {e:#}");
        }
    }
}

struct Inner {
    nora: Nora,
    cfg: CallConfig,
    warm: tokio::sync::Mutex<Option<Prepared>>,
    live: Mutex<Option<Arc<LiveCall>>>,
}

#[derive(Clone)]
pub struct Calls(Arc<Inner>);

static GLOBAL: OnceLock<Calls> = OnceLock::new();

/// Answer to a call start.
#[derive(Debug, Clone, Serialize)]
pub struct Started {
    pub call_id: String,
    pub sdp: String,
    pub kind: CallKind,
}

impl Calls {
    /// Tables only, no background work (tests).
    pub fn new(nora: Nora, cfg: CallConfig) -> Result<Calls> {
        nora.db().conn().execute_batch(SCHEMA)?;
        // A call left `live` by a server that stopped is over.
        nora.db().conn().execute("UPDATE nora_voice_call SET status = 'ended', writeup = CASE WHEN writeup = 'running' THEN 'failed' ELSE writeup END WHERE status = 'live' OR writeup = 'running'", [])?;
        Ok(Calls(Arc::new(Inner { nora, cfg, warm: tokio::sync::Mutex::new(None), live: Mutex::new(None) })))
    }

    /// Start the warm-up loop. Call inside a Tokio runtime.
    pub fn start(nora: Nora, cfg: CallConfig) -> Result<Calls> {
        let calls = Calls::new(nora, cfg)?;
        let _ = GLOBAL.set(calls.clone());
        if calls.0.cfg.relay.is_none() {
            warn!("nora calls: codex not found (set AOS_CODEX_BIN); calls can't start");
        } else if calls.0.cfg.warm {
            let c = calls.clone();
            let stop = c.0.nora.shutdown_token();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(15)).await;
                loop {
                    c.keep_warm().await;
                    tokio::select! {
                        _ = stop.cancelled() => break,
                        _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                    }
                }
                if let Some(p) = c.0.warm.lock().await.take() {
                    p.rpc.kill().await;
                }
            });
        }
        Ok(calls)
    }

    pub fn global() -> Option<Calls> {
        GLOBAL.get().cloned()
    }

    pub fn nora(&self) -> &Nora {
        &self.0.nora
    }

    fn live(&self) -> Option<Arc<LiveCall>> {
        self.0.live.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn is_live(&self) -> bool {
        self.live().is_some()
    }

    /// A fresh relay in the warm slot unless one younger than 15 minutes is there.
    async fn keep_warm(&self) {
        let Some(relay) = self.0.cfg.relay.clone() else { return };
        if self.is_live() {
            return;
        }
        let mut slot = self.0.warm.lock().await;
        if let Some(p) = slot.as_ref()
            && !p.rpc.ended()
            && p.made.elapsed() < Duration::from_secs(15 * 60)
        {
            return;
        }
        if let Some(old) = slot.take() {
            old.rpc.kill().await;
        }
        match realtime::prepare(&relay).await {
            Ok(p) => *slot = Some(p),
            Err(e) => warn!("nora call: warm-up failed: {e:#}"),
        }
    }

    async fn take_relay(&self) -> Result<Prepared> {
        let relay = self.0.cfg.relay.clone().context("Calls need the codex CLI on the server (set AOS_CODEX_BIN).")?;
        let warm = self.0.warm.lock().await.take();
        if let Some(p) = warm {
            if !p.rpc.ended() && p.made.elapsed() < Duration::from_secs(20 * 60) {
                return Ok(p);
            }
            p.rpc.kill().await;
        }
        realtime::prepare(&relay).await
    }

    /// Start a call from the phone's WebRTC offer. `ring_id`: answering a ring.
    pub async fn start_call(&self, sdp: &str, reason: Option<&str>, ring_id: Option<i64>, kind: Option<CallKind>) -> Result<Started> {
        anyhow::ensure!(sdp.starts_with("v="), "Send the WebRTC offer (sdp).");
        if let Some(old) = self.live() {
            self.end_call(&old.id, "replaced by a new call").await;
        }
        let ring = match ring_id {
            Some(id) => self.answer_ring(id)?,
            None => None,
        };
        let kind = kind.unwrap_or(match &ring {
            Some((_, source)) if source == "close_day" => CallKind::CloseDay,
            _ => CallKind::Quick,
        });
        let reason = ring
            .as_ref()
            .map(|(r, _)| r.clone())
            .or_else(|| reason.map(str::to_string))
            .unwrap_or_else(|| if kind == CallKind::CloseDay { "Close the day".into() } else { String::new() });

        let mut prep = self.take_relay().await?;
        let id = format!("call-{}", aos_core::ids::new_id());
        let owner = self.0.nora.config().owner.clone();
        let context = {
            let n = self.0.nora.clone();
            tokio::task::spawn_blocking(move || call_context(&n, 14_000)).await.unwrap_or_default()
        };
        let persona = voice_persona(&owner, kind, &reason, &context);
        let opener = opener_line(&owner, kind, Local::now().hour());

        let (sdp_tx, sdp_rx) = oneshot::channel::<Result<String, String>>();
        let call = Arc::new(LiveCall {
            id: id.clone(),
            kind,
            reason: reason.clone(),
            started_ms: now_ms(),
            rpc: prep.rpc.clone(),
            thread_id: prep.thread_id.clone(),
            transcript: Mutex::new(Vec::new()),
            opener: Mutex::new(Some(opener)),
            thinking: AtomicUsize::new(0),
            handoffs: AtomicUsize::new(0),
            last_heard: Mutex::new(Instant::now()),
            ended: AtomicBool::new(false),
            cancel: CancellationToken::new(),
        });
        let notes = std::mem::replace(&mut prep.notes, mpsc::unbounded_channel().1);
        tokio::spawn(self.clone().drive(call.clone(), notes, Some(sdp_tx)));

        let t1 = Instant::now();
        let started = async {
            prep.rpc
                .request("thread/realtime/start", realtime::start_params(&prep.thread_id, sdp, &self.0.cfg.voice, &persona), Duration::from_secs(45))
                .await?;
            let answer = tokio::time::timeout(Duration::from_secs(45), sdp_rx)
                .await
                .map_err(|_| anyhow!("Codex didn't connect the call in time"))?
                .map_err(|_| anyhow!("Codex stopped before answering"))?
                .map_err(|e| anyhow!("{e}"))?;
            anyhow::ensure!(answer.starts_with("v="), "Codex returned no call answer");
            Ok::<String, anyhow::Error>(answer)
        }
        .await;
        let answer = match started {
            Ok(a) => a,
            Err(e) => {
                call.ended.store(true, Ordering::SeqCst);
                call.cancel.cancel();
                prep.rpc.kill().await;
                self.rewarm_later();
                return Err(e);
            }
        };
        info!("nora call {id}: answered {:.1} s after the offer", t1.elapsed().as_secs_f32());
        {
            let c = self.0.nora.db().conn();
            c.execute(
                "INSERT INTO nora_voice_call (id, kind, reason, ring_id, status, started_at) VALUES (?1, ?2, ?3, ?4, 'live', ?5)",
                params![id, kind.as_str(), reason, ring_id, call.started_ms],
            )?;
        }
        *self.0.live.lock().unwrap_or_else(|e| e.into_inner()) = Some(call.clone());
        self.emit(call.view("live"));
        // Her opener once the phone says its audio is up (`ready`), or after 7 s anyway.
        let me = self.clone();
        let cid = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(7)).await;
            me.ready(&cid).await;
        });
        // The idle watch: a phone that dropped off without hanging up.
        let me = self.clone();
        let c2 = call.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = c2.cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(15)) => {}
                }
                let quiet = c2.last_heard.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                if quiet > me.0.cfg.idle && c2.thinking.load(Ordering::SeqCst) == 0 {
                    me.end_call(&c2.id, "quiet for too long").await;
                    break;
                }
            }
        });
        self.rewarm_later();
        Ok(Started { call_id: id, sdp: answer, kind })
    }

    fn rewarm_later(&self) {
        if !self.0.cfg.warm {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            me.keep_warm().await;
        });
    }

    /// The phone's audio is up: she says her opener, once.
    pub async fn ready(&self, id: &str) {
        let Some(call) = self.live().filter(|c| c.id == id) else { return };
        let line = call.opener.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(line) = line {
            call.say(&line).await;
        }
    }

    /// Notifications from the relay for one call.
    async fn drive(self, call: Arc<LiveCall>, mut notes: mpsc::UnboundedReceiver<Note>, mut sdp_tx: Option<oneshot::Sender<Result<String, String>>>) {
        loop {
            let note = tokio::select! {
                _ = call.cancel.cancelled() => break,
                n = notes.recv() => n,
            };
            let Some((method, p)) = note else {
                if !call.ended.load(Ordering::SeqCst) {
                    self.end_call(&call.id, "the voice engine stopped").await;
                }
                break;
            };
            if let Some(t) = p["threadId"].as_str()
                && t != call.thread_id
            {
                continue;
            }
            match method.as_str() {
                "thread/realtime/sdp" => {
                    if let Some(tx) = sdp_tx.take() {
                        let _ = tx.send(Ok(p["sdp"].as_str().unwrap_or("").to_string()));
                    }
                }
                "thread/realtime/error" => {
                    let msg = p["message"].as_str().unwrap_or("realtime error").to_string();
                    warn!("nora call {}: {msg}", call.id);
                    if let Some(tx) = sdp_tx.take() {
                        let _ = tx.send(Err(msg));
                    }
                }
                "thread/realtime/closed" => {
                    let why = p["reason"].as_str().unwrap_or("closed").to_string();
                    self.end_call(&call.id, &why).await;
                    break;
                }
                "thread/realtime/transcript/delta" => {
                    call.heard();
                    let who = if p["role"] == "user" { "you" } else { "nora" };
                    let text = p["delta"].as_str().unwrap_or("").to_string();
                    if !text.is_empty() {
                        self.0.nora.bus().send(NoraEvent::CallTranscript { call_id: call.id.clone(), who: who.into(), text, done: false });
                    }
                }
                "thread/realtime/transcript/done" => {
                    call.heard();
                    let who = if p["role"] == "user" { "you" } else { "nora" };
                    let text = p["text"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
                    if !text.is_empty() {
                        push_line(&mut call.transcript.lock().unwrap_or_else(|e| e.into_inner()), who, &text, now_ms());
                        self.0.nora.bus().send(NoraEvent::CallTranscript { call_id: call.id.clone(), who: who.into(), text, done: true });
                    }
                }
                "turn/started" => call.heard(),
                "item/started" => {
                    if let Some(raw) = realtime::user_message_text(&p["item"]) {
                        let asked = realtime::parse_delegation(&raw).0;
                        call.heard();
                        // Codex only provides the voice: stop its turn, run the job on Claude.
                        if let Some(turn) = p["turnId"].as_str() {
                            let rpc = call.rpc.clone();
                            let params = json!({ "threadId": call.thread_id, "turnId": turn });
                            tokio::spawn(async move {
                                if let Err(e) = rpc.request("turn/interrupt", params, Duration::from_secs(5)).await {
                                    warn!("nora call: stopping the Codex turn: {e:#}");
                                }
                            });
                        }
                        tokio::spawn(self.clone().handoff(call.clone(), asked));
                    }
                }
                "item/completed" if p["item"]["type"] == "mcpToolCall" || p["item"]["type"] == "commandExecution" => {
                    warn!("nora call: the Codex relay ran a tool ({}); it should not", p["item"]["type"]);
                }
                _ => {}
            }
        }
    }

    /// One hand-off: a Claude turn with all tools, the result spoken into the call.
    async fn handoff(self, call: Arc<LiveCall>, asked: String) {
        let nora = self.0.nora.clone();
        let owner = nora.config().owner.clone();
        info!("nora call {}: hand-off \"{}\"", call.id, prompt::clip(&asked, 160));
        call.handoffs.fetch_add(1, Ordering::SeqCst);
        call.thinking.fetch_add(1, Ordering::SeqCst);
        self.emit(call.view("live"));
        let mut m = NewMsg::new("user", "call", "handoff", asked.clone());
        m.turn_id = Some(call.id.clone());
        let _ = nora.post(m);
        nora.bus().send(NoraEvent::CallHandoff { call_id: call.id.clone(), status: "running".into(), text: asked.clone() });

        let done = CancellationToken::new();
        // Never silent: a nudge after `nudge`, then every `nudge_again`.
        {
            let (c, d, first, again) = (call.clone(), done.clone(), self.0.cfg.nudge, self.0.cfg.nudge_again);
            tokio::spawn(async move {
                let mut wait = first;
                loop {
                    tokio::select! {
                        _ = d.cancelled() => break,
                        _ = c.cancel.cancelled() => break,
                        _ = tokio::time::sleep(wait) => c.say(STILL_LINE).await,
                    }
                    wait = again;
                }
            });
        }

        let heard_so_far = {
            let t = call.transcript.lock().unwrap_or_else(|e| e.into_inner());
            let skip = t.len().saturating_sub(12);
            t.iter().skip(skip).map(|l| format!("{}: {}", if l.who == "you" { owner.as_str() } else { "Nora" }, l.text)).collect::<Vec<_>>().join("\n")
        };
        let memory = nora.db().memory_context(&asked, &owner).unwrap_or_default();
        let context = {
            let n = nora.clone();
            tokio::task::spawn_blocking(move || call_context(&n, 6_000)).await.unwrap_or_default()
        };
        let req = TurnRequest {
            system: format!("{}\n{}", prompt::system_prompt(&owner), call_rules(&owner)),
            prompt: format!(
                "Now: {now}.\n\n{memory}\n{context}\n\nOn the call so far:\n{heard}\n\n[The voice handed you this from {owner}]\n{asked}",
                now = prompt::local_time(now_ms()),
                heard = if heard_so_far.is_empty() { "(nothing yet)".into() } else { heard_so_far },
            ),
            model: self.0.cfg.model.clone(),
            mcp_config: nora.mcp_config(),
            deny_tools: vec!["nora_send".into(), "call_request".into()],
            timeout: Some(self.0.cfg.handoff_max),
            ..Default::default()
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let run = nora.engine().run(req, tx);
        let bus = nora.bus().clone();
        let cid = call.id.clone();
        let progress = async move {
            while let Some(u) = rx.recv().await {
                if let TurnUpdate::Tool { title, done: false, .. } = u {
                    bus.send(NoraEvent::CallHandoff { call_id: cid.clone(), status: "tool".into(), text: title });
                }
            }
        };
        let (out, _) = tokio::join!(tokio::time::timeout(self.0.cfg.handoff_max + Duration::from_secs(5), run), progress);
        done.cancel();

        let (status, spoken_text) = match out {
            Ok(o) if o.ok && o.final_text().is_some() => {
                let text = o.final_text().unwrap_or_default().to_string();
                let mut m = NewMsg::new("claude", "call", "final", text.clone());
                m.turn_id = Some(call.id.clone());
                let _ = nora.post(m);
                ("done", spoken(&text))
            }
            Ok(o) => {
                let err = o.error.unwrap_or_else(|| "no answer".into());
                warn!("nora call {}: hand-off failed: {err}", call.id);
                let slow = err.contains("timed out");
                let mut m = NewMsg::new("system", "call", "error", format!("On the call I couldn't do: \"{}\" ({})", prompt::clip(&asked, 300), prompt::clip(&err, 200)));
                m.turn_id = Some(call.id.clone());
                let _ = nora.post(m);
                ("failed", if slow { SLOW_LINE.to_string() } else { FAILED_LINE.to_string() })
            }
            Err(_) => ("failed", SLOW_LINE.to_string()),
        };
        call.say(&spoken_text).await;
        call.thinking.fetch_sub(1, Ordering::SeqCst);
        nora.bus().send(NoraEvent::CallHandoff { call_id: call.id.clone(), status: status.into(), text: spoken_text });
        if !call.ended.load(Ordering::SeqCst) {
            self.emit(call.view("live"));
        }
    }

    /// Hang up (from the phone, the idle watch or the voice engine). Safe to call twice.
    pub async fn end_call(&self, id: &str, why: &str) {
        let Some(call) = self.live().filter(|c| c.id == id) else { return };
        if call.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        info!("nora call {id} ended: {why}");
        call.cancel.cancel();
        let _ = call.rpc.request("thread/realtime/stop", json!({ "threadId": call.thread_id }), Duration::from_secs(5)).await;
        call.rpc.kill().await;
        {
            let mut live = self.0.live.lock().unwrap_or_else(|e| e.into_inner());
            if live.as_ref().is_some_and(|c| c.id == id) {
                *live = None;
            }
        }
        let ended = now_ms();
        let seconds = (ended - call.started_ms) / 1000;
        let transcript = call.transcript.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let spoke = transcript.iter().any(|l| l.who == "you");
        let writeup = if spoke { "running" } else { "none" };
        let _ = self.0.nora.db().conn().execute(
            "UPDATE nora_voice_call SET status = 'ended', ended_at = ?2, seconds = ?3, transcript = ?4, handoffs = ?5, writeup = ?6, end_reason = ?7 WHERE id = ?1",
            params![id, ended, seconds, serde_json::to_string(&transcript).unwrap_or_default(), call.handoffs.load(Ordering::SeqCst) as i64, writeup, why],
        );
        let mut v = call.view("ended");
        v.ended_at = Some(ended);
        v.seconds = seconds;
        v.writeup = writeup.into();
        v.thinking = 0;
        self.emit(v);
        if spoke {
            tokio::spawn(self.clone().write_up(id.to_string()));
        } else {
            let mins = (seconds / 60).max(1);
            let _ = self.0.nora.post(NewMsg::new("system", "call", "call", format!("Call ended after {} (nothing said).", plural(mins, "minute"))));
        }
        self.rewarm_later();
    }

    /// The write-up: summary and action points into the thread and the journal. Always ends.
    pub async fn write_up(self, id: String) {
        let nora = self.0.nora.clone();
        let owner = nora.config().owner.clone();
        let Ok(Some(row)) = self.call_row(&id) else { return };
        let transcript: Vec<Line> = serde_json::from_str(&row.1).unwrap_or_default();
        let (kind, seconds) = (row.0.kind, row.0.seconds);
        let lines = transcript.iter().map(|l| format!("{}: {}", if l.who == "you" { owner.as_str() } else { "Nora" }, l.text)).collect::<Vec<_>>().join("\n");
        let handoffs = nora
            .db()
            .search(None, Some(row.0.started_at), Some(row.0.ended_at.unwrap_or_else(now_ms) + 60_000), 200)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| m.source == "call" && m.turn_id.as_deref() == Some(id.as_str()))
            .map(|m| format!("- {}: {}", if m.engine == "user" { "asked" } else { "result" }, prompt::clip(&m.text, 300)))
            .collect::<Vec<_>>()
            .join("\n");
        let mins = (seconds / 60).max(1);
        let req = TurnRequest {
            system: format!("{}\nYou are writing up a voice call that just ended. Reply with one JSON object only.", prompt::system_prompt(&owner)),
            prompt: writeup_prompt(&owner, kind, mins, &prompt::clip(&lines, 24_000), &handoffs),
            model: self.0.cfg.model.clone(),
            mcp_config: nora.mcp_config(),
            deny_tools: vec!["nora_send".into(), "call_request".into()],
            timeout: Some(self.0.cfg.writeup_max),
            ..Default::default()
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let out = tokio::time::timeout(self.0.cfg.writeup_max + Duration::from_secs(5), nora.engine().run(req, tx)).await;
        let parsed = match &out {
            Ok(o) if o.ok => o.final_text().and_then(parse_writeup),
            Ok(o) => {
                warn!("nora call write-up failed: {:?}", o.error);
                None
            }
            Err(_) => {
                warn!("nora call write-up timed out");
                None
            }
        };
        let failed = parsed.is_none();
        let w = parsed.unwrap_or_else(|| WriteUp { summary: fallback_summary(&transcript, &owner, seconds), ..Default::default() });
        if let Err(e) = self.settle(&id, kind, mins, &w, failed) {
            warn!("nora call write-up save: {e:#}");
        }
    }

    fn settle(&self, id: &str, kind: CallKind, mins: i64, w: &WriteUp, failed: bool) -> Result<()> {
        let nora = &self.0.nora;
        let what = if kind == CallKind::CloseDay { "Close-the-day call" } else { "Call" };
        let mut text = format!("{what}, {}. {}", plural(mins, "min"), w.summary.trim());
        if failed {
            text = format!("{what}, {}. Couldn't write it up; from the transcript: {}", plural(mins, "min"), w.summary.trim());
        }
        if !w.actions.is_empty() {
            text.push_str("\nAction points:");
            for a in &w.actions {
                text.push_str(&format!("\n- {a}"));
            }
        }
        let mut m = NewMsg::new("claude", "call", "call_writeup", text.clone());
        m.turn_id = Some(id.to_string());
        nora.post(m)?;
        // Journal: his reflections on a close-the-day call count as journaling; a quick call is kept, no XP.
        let journal = if kind == CallKind::CloseDay || !w.journal.trim().is_empty() {
            let body = if w.journal.trim().is_empty() { text.clone() } else { format!("{}\n\n{text}", w.journal.trim()) };
            aos_core::journal::NewJournal { text: body, kind: Some("journal".into()), source: Some("call".into()), ..Default::default() }
        } else {
            aos_core::journal::NewJournal { text: text.clone(), kind: Some("call".into()), source: Some("call".into()), ..Default::default() }
        };
        nora.core().add_journal("nora", journal)?;
        nora.db().conn().execute(
            "UPDATE nora_voice_call SET summary = ?2, actions = ?3, writeup = ?4 WHERE id = ?1",
            params![id, w.summary, serde_json::to_string(&w.actions)?, if failed { "failed" } else { "done" }],
        )?;
        if let Some(v) = self.view(id)? {
            self.emit(v);
        }
        Ok(())
    }

    fn call_row(&self, id: &str) -> Result<Option<(CallView, String)>> {
        let c = self.0.nora.db().conn();
        Ok(c.query_row(&format!("SELECT {VIEW_COLS}, transcript FROM nora_voice_call WHERE id = ?1"), [id], |r| Ok((view_row(r)?, r.get(12)?)))
            .optional()?)
    }

    pub fn view(&self, id: &str) -> Result<Option<CallView>> {
        if let Some(c) = self.live().filter(|c| c.id == id) {
            return Ok(Some(c.view("live")));
        }
        Ok(self.call_row(id)?.map(|r| r.0))
    }

    /// The live call and any ring, for a client that just connected.
    pub fn current(&self) -> Result<Value> {
        let ringing = self.0.nora.db().ringing_call()?;
        Ok(json!({ "live": self.live().map(|c| c.view("live")), "ringing": ringing, "orb": orb_state(&self.0.nora) }))
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<CallView>> {
        let c = self.0.nora.db().conn();
        let mut s = c.prepare(&format!("SELECT {VIEW_COLS} FROM nora_voice_call ORDER BY started_at DESC LIMIT ?1"))?;
        Ok(s.query_map([limit.clamp(1, 100) as i64], view_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn transcript(&self, id: &str) -> Result<Vec<Line>> {
        if let Some(c) = self.live().filter(|c| c.id == id) {
            return Ok(c.transcript.lock().unwrap_or_else(|e| e.into_inner()).clone());
        }
        Ok(self.call_row(id)?.map(|r| serde_json::from_str(&r.1).unwrap_or_default()).unwrap_or_default())
    }

    /// `(reason, source)` of a ring, now answered.
    fn answer_ring(&self, id: i64) -> Result<Option<(String, String)>> {
        let c = self.0.nora.db().conn();
        let r = c
            .query_row("SELECT reason, source FROM nora_call WHERE id = ?1", [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .optional()?;
        c.execute("UPDATE nora_call SET status = 'answered', updated_at = ?2 WHERE id = ?1", params![id, now_ms()])?;
        Ok(r)
    }

    pub fn decline_ring(&self, id: i64) -> Result<()> {
        self.0.nora.db().conn().execute("UPDATE nora_call SET status = 'declined', updated_at = ?2 WHERE id = ?1 AND status = 'ringing'", params![id, now_ms()])?;
        Ok(())
    }

    /// Ring him in the app now (the close-the-day call uses source `close_day`).
    pub fn ring(&self, reason: &str, source: &str) -> Result<crate::store::Call> {
        let call = self.0.nora.db().add_call(reason, source)?;
        self.0.nora.bus().send(NoraEvent::CallRequested { call: call.clone() });
        Ok(call)
    }

    fn emit(&self, call: CallView) {
        self.0.nora.bus().send(NoraEvent::CallState { call });
    }
}

const VIEW_COLS: &str = "id, kind, reason, status, started_at, ended_at, seconds, handoffs, summary, actions, writeup, ring_id";

fn view_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CallView> {
    let kind: String = r.get(1)?;
    let actions: String = r.get(9)?;
    Ok(CallView {
        id: r.get(0)?,
        kind: if kind == "close_day" { CallKind::CloseDay } else { CallKind::Quick },
        reason: r.get(2)?,
        status: r.get(3)?,
        started_at: r.get(4)?,
        ended_at: r.get(5)?,
        seconds: r.get(6)?,
        thinking: 0,
        handoffs: r.get(7)?,
        summary: r.get(8)?,
        actions: serde_json::from_str(&actions).unwrap_or_default(),
        writeup: r.get(10)?,
    })
}

/// Join a transcript part onto the last line when the same person is still talking.
pub fn push_line(t: &mut Vec<Line>, who: &str, text: &str, at: i64) {
    if let Some(last) = t.last_mut()
        && last.who == who
        && at - last.at < 4000
    {
        last.text.push(' ');
        last.text.push_str(text);
        last.at = at;
        return;
    }
    t.push(Line { who: who.into(), text: text.into(), at });
}

fn plural(n: i64, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// Plain speakable text: no markdown, links or ids.
pub fn spoken(text: &str) -> String {
    let mut s = String::new();
    let mut rest = text;
    // [label](url) -> label
    while let Some(i) = rest.find('[') {
        s.push_str(&rest[..i]);
        let tail = &rest[i..];
        match (tail.find("]("), tail.find(')')) {
            (Some(j), Some(k)) if j < k => {
                s.push_str(&tail[1..j]);
                rest = &tail[k + 1..];
            }
            _ => {
                s.push('[');
                rest = &tail[1..];
            }
        }
    }
    s.push_str(rest);
    let s: String = s.chars().filter(|c| !matches!(c, '*' | '#' | '`' | '_' | '|')).collect();
    let words: Vec<&str> = s.split_whitespace().filter(|w| !w.starts_with("http://") && !w.starts_with("https://")).collect();
    let mut out = words.join(" ");
    out = out.replace("- ", "");
    prompt::clip(&out, 700).replace(" [...]", "...")
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct WriteUp {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub journal: String,
}

/// Her JSON reply, tolerating a code fence or words around it.
pub fn parse_writeup(text: &str) -> Option<WriteUp> {
    let t = text.replace("```json", "").replace("```", "");
    let t = t.trim();
    let candidates = [t.to_string(), t.find('{').and_then(|a| t.rfind('}').map(|b| t[a..=b].to_string())).unwrap_or_default()];
    for c in candidates {
        if let Ok(mut w) = serde_json::from_str::<WriteUp>(&c)
            && !w.summary.trim().is_empty()
        {
            w.summary = prompt::clip(w.summary.trim(), 1500);
            w.actions = w.actions.into_iter().map(|a| prompt::clip(a.trim(), 200)).filter(|a| !a.is_empty()).take(12).collect();
            return Some(w);
        }
    }
    None
}

/// A summary from the transcript alone, for when the write-up turn fails or runs out of time.
pub fn fallback_summary(t: &[Line], owner: &str, seconds: i64) -> String {
    let his: Vec<&str> = t.iter().filter(|l| l.who == "you" && l.text.len() > 2).map(|l| l.text.as_str()).take(4).collect();
    let mins = (seconds / 60).max(1);
    if his.is_empty() {
        return format!("A {}-minute call.", mins);
    }
    format!("A {mins}-minute call. {owner} said: {}", prompt::clip(&his.join(" / "), 280))
}

pub fn call_rules(owner: &str) -> String {
    format!(
        "# This is a live phone call. These rules win over anything above.
You are the thinking half of the call. The voice talks with {owner} and hands you what he asks, word for word or in short. \
Do the work with your tools now, then answer in one to three short spoken sentences: plain text, no markdown, links, ids, JSON or lists. \
Your answer is spoken to him exactly as you write it. Match his language (Hinglish in, Hinglish out, \"kar rahi hoon\"). Say times the way people do (\"at 6 in the evening\").
- He is on the line, so what he asks for on this call is his say: do it now, don't ask \"should I?\". Use his words; add nothing he didn't say.
- Skip the short opening line you'd write in chat. Go straight to the tools, then the answer.
- A tool that fails: say what didn't work in one sentence. Never say something is done unless a tool did it.
- Never state a plan, task, time, number or result unless a tool returned it.
- Anything that messages other people, spends money or deletes data: say what you would do and ask for his yes. The voice will relay it."
    )
}

fn opener_line(owner: &str, kind: CallKind, hour: u32) -> String {
    let greet = match hour {
        5..=11 => "Good morning",
        12..=16 => "Hi",
        _ => "Hey",
    };
    match kind {
        CallKind::CloseDay => format!("Hi {owner}. Let's close the day. How did today go?"),
        CallKind::Quick => format!("{greet} {owner}, I'm here. What do you need?"),
    }
}

pub fn voice_persona(owner: &str, kind: CallKind, reason: &str, context: &str) -> String {
    let why = match (kind, reason.trim()) {
        (CallKind::CloseDay, _) => CLOSE_DAY_SCRIPT.to_string(),
        (_, "") => format!("{owner} rang you."),
        (_, r) => format!("Why this call: {r}"),
    };
    let p = format!(
        "Your name is Nora. You are {owner}'s personal assistant, talking with {owner} on a voice call in the Agentic OS app.
Talk like a person on a call: warm, quick, one or two short sentences, then let {owner} talk. Match his language: Hinglish in, natural Hinglish out (Roman Hindi mixed with English); English in, English out. You're a woman: \"kar rahi hoon\", \"dekhti hoon\".
{why}
Codex does every job, with all of {owner}'s tools: tasks, habits, alarms and reminders, journal, memory, mood, calendar, messages, the app. Hand it every request to do or change something and every question about his data you can't answer from the facts below, in his words. Also hand it his answer when you asked him something for Codex (\"he said yes\") and any status he states (\"I'm going to sleep\"). Codex's answer is spoken to him directly, so don't repeat it.
Never say something is done, and never make up his data, times or numbers, until Codex answers. If it failed, say so plainly.
Never read out links, ids, markdown or long lists. When he says bye, say a short goodbye.
What you know right now:
{context}"
    );
    // initialItems are capped at about 8k tokens in total.
    prompt::clip(&p, 22_000)
}

const CLOSE_DAY_SCRIPT: &str = "This is the nightly close-the-day call, 20 to 30 minutes. Ask one question at a time and listen more than you talk:
1. How today went, in a sentence or two.
2. Today's plan: what got done, what slipped, and the real reason.
3. Mood and energy now, and what moved them.
4. One win, one thing that was hard, one thing he learned.
5. Tomorrow: the top three things, and when.
Hand off any task, reminder or change he decides on. Don't lecture. When the five are covered, sum up in two sentences and say good night.";

fn writeup_prompt(owner: &str, kind: CallKind, mins: i64, transcript: &str, handoffs: &str) -> String {
    let journal_rule = match kind {
        CallKind::CloseDay => "\"journal\": his reflections from this call written as his journal entry for today, first person, his words where possible, plain text",
        CallKind::Quick => "\"journal\": \"\" unless he reflected on his day or feelings; then those reflections as his journal entry, first person",
    };
    format!(
        "A {mins}-minute voice call with {owner} just ended ({kind}). Transcript:\n{transcript}\n\nJobs done during the call:\n{handoffs}\n\n\
Save anything durable he said with `remember`. Do not message anyone or change tasks now.\n\
Then reply with JSON only, no code fence: {{\"summary\": \"2-3 sentences\", \"actions\": [\"action points, one line each, who does what\"], {journal_rule}}}",
        kind = if kind == CallKind::CloseDay { "the nightly close-the-day call" } else { "a quick call" },
        handoffs = if handoffs.is_empty() { "(none)" } else { handoffs },
    )
}

// --- context ----------------------------------------------------------------------------------

fn hhmm(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339).map(|t| t.with_timezone(&Local).format("%H:%M").to_string()).unwrap_or_default()
}

/// Everything the voice (or a hand-off) should know at the start of a call, clipped to `max` chars.
pub fn call_context(nora: &Nora, max: usize) -> String {
    let core = nora.core();
    let owner = &nora.config().owner;
    let now = chrono::Utc::now();
    let today = Local::now().date_naive();
    let mut out = vec![format!("Now: {}.", prompt::local_time(now_ms()))];

    if let Ok(t) = core.today_overview(today, now) {
        out.push(format!("Today's plan: {} done, {} open, {} skipped.", t.score.done, t.score.open, t.score.skipped));
        let mut open = Vec::new();
        let mut done = Vec::new();
        for g in &t.areas {
            let area = g.area.clone().unwrap_or_else(|| "No area".into());
            for n in &g.tasks {
                let at = n.task.scheduled.as_deref().and_then(|s| s.split_once('T')).map(|(_, t)| format!(" at {t}")).unwrap_or_default();
                let line = format!("[{area}] {}{at}", n.task.title);
                if n.task.status.is_active() { open.push(line) } else if n.task.status.as_str() == "done" { done.push(line) }
            }
        }
        if !open.is_empty() {
            out.push(format!("Open: {}", open.into_iter().take(20).collect::<Vec<_>>().join("; ")));
        }
        if !done.is_empty() {
            out.push(format!("Done: {}", done.into_iter().take(15).collect::<Vec<_>>().join("; ")));
        }
        let habits: Vec<String> = t
            .habits
            .iter()
            .filter(|h| h.due_today)
            .map(|h| format!("{} ({}, streak {})", h.habit.name, h.today.map(|s| s.as_str()).unwrap_or("not yet"), h.streak.current))
            .collect();
        if !habits.is_empty() {
            out.push(format!("Habits today: {}", habits.join("; ")));
        }
        if !t.alarms.is_empty() {
            out.push(format!("Alarms and reminders next 24 h: {}", t.alarms.iter().map(|a| format!("{} {}", hhmm(&a.at), a.label)).collect::<Vec<_>>().join("; ")));
        }
        let checkins: Vec<String> = t
            .moods
            .iter()
            .map(|m| {
                let mut s = format!("{} mood {:+.1}", hhmm(&m.at), m.valence);
                if let Some(e) = m.energy {
                    s.push_str(&format!(", energy {e}/5"));
                }
                if !m.labels.is_empty() {
                    s.push_str(&format!(", felt {}", m.labels.join("/")));
                }
                if !m.reasons.is_empty() {
                    s.push_str(&format!(", because {}", m.reasons.join("/")));
                }
                if let Some(n) = m.note.as_deref().filter(|n| !n.trim().is_empty()) {
                    s.push_str(&format!(" ({})", prompt::clip(n, 140)));
                }
                s
            })
            .collect();
        out.push(if checkins.is_empty() { "Check-ins today: none yet.".into() } else { format!("Check-ins today (mood -1..1): {}", checkins.join("; ")) });
        out.push(format!("Journal written today: {}.", if t.journal_written { "yes" } else { "no" }));
        let goals: Vec<String> = t.goals.iter().map(|g| format!("{} {:.0}%", g.name, g.progress * 100.0)).collect();
        out.push(format!("Mission: level {}, {} XP today. Goals: {}.", t.xp.level.level, t.xp.today, if goals.is_empty() { "none".into() } else { goals.join("; ") }));
    }
    if let Ok(x) = core.xp_summary(today) {
        out.push(format!("XP this week: {}.", x.week));
    }
    out.push(format!("Orb (his state): {}", orb_state(nora).summary()));
    // Last call write-ups.
    if let Ok(mut s) = nora.db().conn().prepare("SELECT started_at, kind, summary, actions FROM nora_voice_call WHERE summary != '' ORDER BY started_at DESC LIMIT 3") {
        let rows: Vec<String> = s
            .query_map([], |r| {
                let actions: Vec<String> = serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default();
                Ok(format!(
                    "[{}] {}: {}{}",
                    prompt::local_time(r.get(0)?),
                    if r.get::<_, String>(1)? == "close_day" { "close-the-day call" } else { "call" },
                    prompt::clip(&r.get::<_, String>(2)?, 400),
                    if actions.is_empty() { String::new() } else { format!(" Actions: {}", actions.join("; ")) }
                ))
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();
        if !rows.is_empty() {
            out.push(format!("Last calls:\n{}", rows.join("\n")));
        }
    }
    if let Ok(m) = nora.db().memory_context("", owner)
        && !m.trim().is_empty()
    {
        out.push(prompt::clip(m.trim(), 2500));
    }
    if let Ok(msgs) = nora.db().recent(25, None)
        && !msgs.is_empty()
    {
        out.push(format!("Nora thread (newest last):\n{}", prompt::clip(&prompt::format_thread(&msgs, owner), 5000)));
    }
    let s = out.join("\n");
    if s.chars().count() <= max { s } else { prompt::clip(&s, max) }
}

// --- orb --------------------------------------------------------------------------------------

/// His state for the orb, the call pill and the portrait ring (design/specs/orb-and-call.md §3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OrbState {
    /// calm | flow | energised | busy | stressed | low
    pub state: String,
    /// `#RRGGBB`
    pub color: String,
    /// 0..1
    pub intensity: f64,
    pub label: String,
    /// Real facts behind the colour.
    pub reasons: Vec<String>,
    pub updated_at: i64,
}

impl OrbState {
    pub fn of(state: &str, label: &str, reasons: Vec<String>) -> OrbState {
        let (color, intensity) = match state {
            "flow" => ("#3FD0FF", 0.9),
            "energised" => ("#FFC24B", 1.0),
            "busy" => ("#FF8A3D", 0.9),
            "stressed" => ("#FF5A4E", 0.85),
            "low" => ("#6A63C9", 0.55),
            _ => ("#6DAEFF", 0.8),
        };
        let state = if ["flow", "energised", "busy", "stressed", "low"].contains(&state) { state } else { "calm" };
        OrbState { state: state.into(), color: color.into(), intensity, label: label.into(), reasons, updated_at: now_ms() }
    }

    fn summary(&self) -> String {
        if self.reasons.is_empty() { self.label.clone() } else { format!("{} ({})", self.label, self.reasons.join("; ")) }
    }
}

/// Starting thresholds, to be tuned with his data: the inputs are mood valence (-1..1), energy (1..5),
/// what he said he felt, and today's open tasks.
pub fn classify(valence: Option<f64>, energy: Option<i64>, labels: &[String], open: u32) -> (&'static str, &'static str) {
    let felt = |words: &[&str]| labels.iter().any(|l| words.iter().any(|w| l.to_lowercase().contains(w)));
    if felt(&["stress", "anxious", "overwhelm", "angry", "panic"]) || (valence.is_some_and(|v| v <= -0.3) && open >= 5) {
        return ("stressed", "Stressed");
    }
    if energy.is_some_and(|e| e <= 2) || valence.is_some_and(|v| v <= -0.3) || felt(&["tired", "sad", "low", "drained"]) {
        return ("low", "Low");
    }
    if open >= 8 {
        return ("busy", "Busy");
    }
    if valence.is_some_and(|v| v >= 0.4) && energy.is_some_and(|e| e >= 4) {
        return ("energised", "Energised");
    }
    if felt(&["focus", "flow"]) {
        return ("flow", "In flow");
    }
    ("calm", "Calm")
}

pub fn orb_state(nora: &Nora) -> OrbState {
    if let Ok(Some(raw)) = nora.db().get_kv("orb_override")
        && let Ok(v) = serde_json::from_str::<Value>(&raw)
        && v["until"].as_i64().unwrap_or(0) > now_ms()
    {
        let reasons = vec![format!("Set on {}: {}", prompt::local_time(v["at"].as_i64().unwrap_or(0)), v["why"].as_str().unwrap_or("he said so"))];
        return OrbState::of(v["state"].as_str().unwrap_or("calm"), v["label"].as_str().unwrap_or("Calm"), reasons);
    }
    let today = Local::now().date_naive();
    let mood = nora.core().moods_on(today).ok().and_then(|m| m.into_iter().last());
    let open = nora.core().day_tasks(today).map(|n| n.iter().filter(|t| t.task.status.is_active()).count() as u32).unwrap_or(0);
    let mut reasons = Vec::new();
    if let Some(m) = &mood {
        reasons.push(format!("Mood at {}: {:+.1}", hhmm(&m.at), m.valence));
        if let Some(e) = m.energy {
            reasons.push(format!("Energy {e}/5"));
        }
        if !m.labels.is_empty() {
            reasons.push(format!("Felt {}", m.labels.join(", ")));
        }
    } else {
        reasons.push("No check-in today".into());
    }
    reasons.push(format!("{} open today", plural(open as i64, "task")));
    let labels = mood.as_ref().map(|m| m.labels.clone()).unwrap_or_default();
    let (state, label) = classify(mood.as_ref().map(|m| m.valence), mood.as_ref().and_then(|m| m.energy), &labels, open);
    OrbState::of(state, label, reasons)
}

// --- the 21:00 close-the-day ring -------------------------------------------------------------

/// Called by the scheduler: ring once a day at `close_day_at` (default 21:00) unless turned off
/// (`close_day_call` = `off`) or a call is live. Rings within the hour after the time, so a server
/// that was off at 21:00 still rings at 21:20.
pub fn close_day_tick(nora: &Nora) -> Result<bool> {
    let db = nora.db();
    if db.get_kv("close_day_call")?.as_deref() == Some("off") {
        return Ok(false);
    }
    let at = db.get_kv("close_day_at")?.and_then(|s| NaiveTime::parse_from_str(&s, "%H:%M").ok()).unwrap_or(NaiveTime::from_hms_opt(21, 0, 0).unwrap_or_default());
    let now = Local::now();
    let day = now.format("%Y-%m-%d").to_string();
    let since = now.time().signed_duration_since(at).num_minutes();
    if !(0..60).contains(&since) || db.get_kv("close_day_rang")?.as_deref() == Some(day.as_str()) {
        return Ok(false);
    }
    if Calls::global().is_some_and(|c| c.is_live()) || db.ringing_call()?.is_some() {
        return Ok(false);
    }
    db.set_kv("close_day_rang", &day)?;
    let call = db.add_call("Close the day: your journaling call", "close_day")?;
    nora.bus().send(NoraEvent::CallRequested { call });
    Ok(true)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallSettings {
    pub close_day_call: bool,
    /// `HH:MM`, local.
    pub close_day_at: String,
    pub voice: String,
}

pub fn call_settings(nora: &Nora) -> Result<CallSettings> {
    let db = nora.db();
    Ok(CallSettings {
        close_day_call: db.get_kv("close_day_call")?.as_deref() != Some("off"),
        close_day_at: db.get_kv("close_day_at")?.unwrap_or_else(|| "21:00".into()),
        voice: Calls::global().map(|c| c.0.cfg.voice.clone()).unwrap_or_else(|| realtime::DEFAULT_VOICE.into()),
    })
}

pub fn set_call_settings(nora: &Nora, s: &CallSettings) -> Result<CallSettings> {
    if NaiveTime::parse_from_str(&s.close_day_at, "%H:%M").is_err() {
        bail!("close_day_at must be HH:MM");
    }
    nora.db().set_kv("close_day_call", if s.close_day_call { "on" } else { "off" })?;
    nora.db().set_kv("close_day_at", &s.close_day_at)?;
    call_settings(nora)
}

// --- tools ------------------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OrbSetArgs {
    /// calm, flow, energised, busy, stressed or low.
    pub state: String,
    /// Why, in his words (shown when he long-presses the orb).
    #[serde(default)]
    pub why: Option<String>,
    /// How long it holds (default 3 hours).
    #[serde(default)]
    pub hours: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CallListArgs {
    /// How many recent calls (default 10).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EmptyArgs {}

pub fn tools(nora: &Nora) -> Vec<aos_tools::Tool> {
    use aos_tools::{Risk, Tool, ToolCtx, ToolError};
    let err = |e: anyhow::Error| ToolError::InvalidArgs(format!("{e:#}"));
    let (n1, n2, n3) = (nora.clone(), nora.clone(), nora.clone());
    vec![
        Tool::new("orb_state", "His state as the orb shows it (colour, label and the facts behind it).", Risk::Free, move |_c: &ToolCtx<'_>, _a: EmptyArgs| {
            Ok(serde_json::to_value(orb_state(&n1)).unwrap_or_default())
        })
        .read_only(),
        Tool::new(
            "orb_set_state",
            "Set his state for the orb when he tells you how he is (calm, flow, energised, busy, stressed, low). Holds for `hours` (default 3).",
            Risk::Free,
            move |_c: &ToolCtx<'_>, a: OrbSetArgs| {
                let o = OrbState::of(&a.state.to_lowercase(), "", vec![]);
                let label = match o.state.as_str() {
                    "flow" => "In flow",
                    "energised" => "Energised",
                    "busy" => "Busy",
                    "stressed" => "Stressed",
                    "low" => "Low",
                    _ => "Calm",
                };
                let until = now_ms() + (a.hours.unwrap_or(3.0).clamp(0.1, 24.0) * 3_600_000.0) as i64;
                let v = json!({ "state": o.state, "label": label, "why": a.why.unwrap_or_else(|| "he said so".into()), "at": now_ms(), "until": until });
                n2.db().set_kv("orb_override", &v.to_string()).map_err(err)?;
                let orb = orb_state(&n2);
                n2.bus().send(NoraEvent::OrbState { orb: orb.clone() });
                Ok(serde_json::to_value(orb).unwrap_or_default())
            },
        )
        .positional(&["state"]),
        Tool::new("call_list", "Recent voice calls with Nora: when, how long, summary and action points.", Risk::Free, move |_c: &ToolCtx<'_>, a: CallListArgs| {
            let c = n3.db().conn();
            let mut s = c.prepare("SELECT started_at, kind, seconds, summary, actions FROM nora_voice_call ORDER BY started_at DESC LIMIT ?1").map_err(|e| err(e.into()))?;
            let rows: Vec<Value> = s
                .query_map([a.limit.unwrap_or(10).clamp(1, 50) as i64], |r| {
                    let actions: Vec<String> = serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default();
                    Ok(json!({ "at": prompt::local_time(r.get(0)?), "kind": r.get::<_, String>(1)?, "seconds": r.get::<_, i64>(2)?, "summary": r.get::<_, String>(3)?, "actions": actions }))
                })
                .map_err(|e| err(e.into()))?
                .filter_map(|x| x.ok())
                .collect();
            Ok(json!({ "calls": rows }))
        })
        .read_only(),
    ]
}

// --- HTTP -------------------------------------------------------------------------------------

pub mod http {
    //! | Method | Path | |
    //! |---|---|---|
    //! | GET | `/api/nora/call` | live call, ring, orb |
    //! | POST | `/api/nora/call/start` | `{sdp, reason?, ring_id?, kind?}` → `{call_id, sdp, kind}` |
    //! | POST | `/api/nora/call/{id}/ready` | phone audio is up: she says her opener |
    //! | POST | `/api/nora/call/{id}/end` | hang up |
    //! | GET | `/api/nora/call/{id}` | one call, with transcript |
    //! | POST | `/api/nora/call/ring` | `{reason?, close_day?}` ring in the app now |
    //! | POST | `/api/nora/call/decline` | `{ring_id}` |
    //! | GET | `/api/nora/calls` | recent calls |
    //! | GET | `/api/nora/orb` | [`super::OrbState`] |
    //! | GET/POST | `/api/nora/call-settings` | close-the-day time and switch |

    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use serde::Deserialize;
    use serde_json::json;

    use super::{CallKind, CallSettings, Calls};

    pub fn router_with_auth(calls: Calls, store: Arc<aos_core::Store>) -> Router {
        aos_server::auth::require_device(router(calls), store)
    }

    pub fn router(calls: Calls) -> Router {
        Router::new()
            .route("/api/nora/call", get(current))
            .route("/api/nora/call/start", post(start))
            .route("/api/nora/call/ring", post(ring))
            .route("/api/nora/call/decline", post(decline))
            .route("/api/nora/call/{id}", get(one))
            .route("/api/nora/call/{id}/ready", post(ready))
            .route("/api/nora/call/{id}/end", post(end))
            .route("/api/nora/calls", get(recent))
            .route("/api/nora/orb", get(orb))
            .route("/api/nora/call-settings", get(settings).post(set_settings))
            .with_state(calls)
    }

    fn reply<T: serde::Serialize>(r: anyhow::Result<T>) -> Response {
        match r {
            Ok(v) => Json(v).into_response(),
            Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": { "code": "call", "message": format!("{e:#}") } }))).into_response(),
        }
    }

    #[derive(Deserialize)]
    struct StartBody {
        sdp: String,
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        ring_id: Option<i64>,
        #[serde(default)]
        kind: Option<CallKind>,
    }

    async fn start(State(c): State<Calls>, Json(b): Json<StartBody>) -> Response {
        match c.start_call(&b.sdp, b.reason.as_deref(), b.ring_id, b.kind).await {
            Ok(v) => Json(v).into_response(),
            Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": { "code": "call", "message": format!("Couldn't start the call: {e:#}") } }))).into_response(),
        }
    }

    async fn current(State(c): State<Calls>) -> Response {
        reply(c.current())
    }

    async fn ready(State(c): State<Calls>, Path(id): Path<String>) -> Response {
        let me = c.clone();
        tokio::spawn(async move { me.ready(&id).await });
        Json(json!({ "ok": true })).into_response()
    }

    async fn end(State(c): State<Calls>, Path(id): Path<String>) -> Response {
        c.end_call(&id, "hung up").await;
        reply(c.view(&id))
    }

    async fn one(State(c): State<Calls>, Path(id): Path<String>) -> Response {
        reply((|| Ok(json!({ "call": c.view(&id)?, "transcript": c.transcript(&id)? })))())
    }

    #[derive(Deserialize)]
    struct RingBody {
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        close_day: bool,
    }

    async fn ring(State(c): State<Calls>, Json(b): Json<RingBody>) -> Response {
        let (reason, source) = if b.close_day {
            ("Close the day: your journaling call".to_string(), "close_day")
        } else {
            (b.reason.unwrap_or_else(|| "A quick check-in".into()), "app")
        };
        reply(c.ring(&reason, source))
    }

    #[derive(Deserialize)]
    struct DeclineBody {
        ring_id: i64,
    }

    async fn decline(State(c): State<Calls>, Json(b): Json<DeclineBody>) -> Response {
        reply(c.decline_ring(b.ring_id).map(|_| json!({ "ok": true })))
    }

    async fn recent(State(c): State<Calls>) -> Response {
        reply(c.recent(20))
    }

    async fn orb(State(c): State<Calls>) -> Response {
        Json(super::orb_state(c.nora())).into_response()
    }

    async fn settings(State(c): State<Calls>) -> Response {
        reply(super::call_settings(c.nora()))
    }

    async fn set_settings(State(c): State<Calls>, Json(s): Json<CallSettings>) -> Response {
        reply(super::set_call_settings(c.nora(), &s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nora::testing::nora_with;

    fn calls(texts: &[&str]) -> (Calls, std::path::PathBuf) {
        let (nora, _e, p) = nora_with(texts, None);
        let mut cfg = CallConfig::from_env();
        cfg.relay = None;
        cfg.warm = false;
        (Calls::new(nora, cfg).unwrap(), p)
    }

    #[test]
    fn joins_lines_of_one_speaker() {
        let mut t = Vec::new();
        push_line(&mut t, "you", "set an alarm", 1000);
        push_line(&mut t, "you", "in two minutes", 2500);
        push_line(&mut t, "nora", "Done.", 3000);
        push_line(&mut t, "nora", "Anything else?", 9000);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].text, "set an alarm in two minutes");
    }

    #[test]
    fn spoken_strips_markdown_and_links() {
        assert_eq!(spoken("**Done.** Alarm set for [21:50](https://x.y/a) — `ok`"), "Done. Alarm set for 21:50 — ok");
        assert_eq!(spoken("- one\n- two"), "one two");
    }

    #[test]
    fn parses_writeup_with_fence_and_words() {
        let w = parse_writeup("Here:\n```json\n{\"summary\":\"Set an alarm.\",\"actions\":[\"Nora: alarm at 21:50\"],\"journal\":\"\"}\n```").unwrap();
        assert_eq!(w.summary, "Set an alarm.");
        assert_eq!(w.actions.len(), 1);
        assert!(parse_writeup("not json").is_none());
        assert!(parse_writeup("{\"summary\":\"\"}").is_none());
    }

    #[test]
    fn fallback_uses_his_words() {
        let t = vec![Line { who: "nora".into(), text: "Hi".into(), at: 0 }, Line { who: "you".into(), text: "remind me at 9".into(), at: 1 }];
        assert_eq!(fallback_summary(&t, "V", 75), "A 1-minute call. V said: remind me at 9");
    }

    #[test]
    fn classify_states() {
        assert_eq!(classify(None, None, &[], 2).0, "calm");
        assert_eq!(classify(Some(-0.6), Some(3), &[], 6).0, "stressed");
        assert_eq!(classify(Some(0.2), Some(1), &[], 1).0, "low");
        assert_eq!(classify(Some(0.1), Some(3), &[], 9).0, "busy");
        assert_eq!(classify(Some(0.7), Some(5), &[], 2).0, "energised");
        assert_eq!(classify(Some(0.1), Some(3), &["Stressed".into()], 0).0, "stressed");
    }

    #[test]
    fn persona_has_context_and_close_day_script() {
        let p = voice_persona("V", CallKind::CloseDay, "", "Now: x.");
        assert!(p.contains("close-the-day"));
        assert!(p.contains("Now: x."));
        assert!(voice_persona("V", CallKind::Quick, "", "").contains("V rang you"));
    }

    #[test]
    fn context_has_plan_mood_thread_and_orb() {
        let (c, _p) = calls(&[]);
        let n = c.nora().clone();
        n.core().add_task("t", aos_core::tasks::NewTask { title: "Gym".into(), scheduled: Some("today".into()), ..Default::default() }).unwrap();
        n.core().log_mood("t", aos_core::mood::NewMood { valence: -0.5, energy: Some(2), labels: vec!["tired".into()], ..Default::default() }).unwrap();
        n.post(NewMsg::new("user", "app", "message", "hello there")).unwrap();
        let ctx = call_context(&n, 14_000);
        assert!(ctx.contains("Gym"), "{ctx}");
        assert!(ctx.contains("energy 2/5"));
        assert!(ctx.contains("hello there"));
        assert!(ctx.contains("Orb (his state): Low"));
    }

    #[tokio::test]
    async fn writeup_lands_in_thread_and_journal() {
        let (c, _p) = calls(&["{\"summary\":\"He asked for an alarm.\",\"actions\":[\"Nora: alarm in 2 min (done)\"],\"journal\":\"\"}"]);
        let n = c.nora().clone();
        let t = serde_json::to_string(&vec![Line { who: "you".into(), text: "set an alarm in two minutes".into(), at: now_ms() }]).unwrap();
        n.db()
            .conn()
            .execute(
                "INSERT INTO nora_voice_call (id, kind, status, started_at, ended_at, seconds, transcript, writeup) VALUES ('c1', 'quick', 'ended', ?1, ?1, 40, ?2, 'running')",
                params![now_ms(), t],
            )
            .unwrap();
        c.clone().write_up("c1".into()).await;
        let v = c.view("c1").unwrap().unwrap();
        assert_eq!(v.writeup, "done");
        assert_eq!(v.actions, vec!["Nora: alarm in 2 min (done)"]);
        let last = n.thread(5, None).unwrap().pop().unwrap();
        assert_eq!(last.kind, "call_writeup");
        assert!(last.text.contains("Action points:\n- Nora: alarm in 2 min (done)"));
        let today = Local::now().date_naive();
        let j = n.core().read_journal(today, today, 5).unwrap();
        assert_eq!(j[0].kind, "call");
        // A quick call is not journaling.
        assert!(!n.core().journal_written_on(today).unwrap());
    }

    #[tokio::test]
    async fn writeup_falls_back_to_transcript() {
        let (c, _p) = calls(&["no json here"]);
        let n = c.nora().clone();
        let t = serde_json::to_string(&vec![Line { who: "you".into(), text: "I had a long day".into(), at: now_ms() }]).unwrap();
        n.db()
            .conn()
            .execute("INSERT INTO nora_voice_call (id, kind, status, started_at, seconds, transcript) VALUES ('c2', 'close_day', 'ended', ?1, 1500, ?2)", params![now_ms(), t])
            .unwrap();
        c.clone().write_up("c2".into()).await;
        assert_eq!(c.view("c2").unwrap().unwrap().writeup, "failed");
        let today = Local::now().date_naive();
        // A close-the-day call is journaling, even on the fallback.
        assert!(n.core().journal_written_on(today).unwrap());
        assert!(n.thread(5, None).unwrap().last().unwrap().text.contains("I had a long day"));
    }

    #[test]
    fn close_day_rings_once_and_can_be_turned_off() {
        let (c, _p) = calls(&[]);
        let n = c.nora().clone();
        let now = Local::now();
        let at = (now - chrono::Duration::minutes(5)).format("%H:%M").to_string();
        // Near midnight the window wraps; skip the timing part then.
        if now.hour() == 0 && now.minute() < 6 {
            return;
        }
        n.db().set_kv("close_day_at", &at).unwrap();
        assert!(close_day_tick(&n).unwrap());
        assert!(!close_day_tick(&n).unwrap());
        let ring = n.db().ringing_call().unwrap().unwrap();
        assert_eq!(ring.source, "close_day");
        // Answering marks it answered and makes it a close-the-day call.
        assert_eq!(c.answer_ring(ring.id).unwrap().unwrap().1, "close_day");
        assert!(n.db().ringing_call().unwrap().is_none());
        n.db().set_kv("close_day_rang", "").unwrap();
        set_call_settings(&n, &CallSettings { close_day_call: false, close_day_at: at, voice: String::new() }).unwrap();
        assert!(!close_day_tick(&n).unwrap());
    }

    #[test]
    fn orb_override_holds() {
        let (c, _p) = calls(&[]);
        let n = c.nora().clone();
        let mut r = aos_tools::registry();
        r.extend(tools(&n));
        let ctx = aos_tools::ToolCtx::new(n.core(), "test");
        let o = r.call(&ctx, "orb_set_state", json!({ "state": "stressed", "why": "exam tomorrow" })).unwrap();
        assert_eq!(o["color"], "#FF5A4E");
        assert_eq!(orb_state(&n).state, "stressed");
        assert!(r.call(&ctx, "call_list", json!({})).unwrap()["calls"].as_array().unwrap().is_empty());
    }
}
