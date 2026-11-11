//! Live events for the UI: one broadcast channel, streamed at `GET /api/nora/events` (SSE).

use serde::Serialize;
use tokio::sync::broadcast;

use crate::store::{Call, Heartbeat, Msg, TurnRow};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NoraEvent {
    /// A new message in the thread (from him, Claude, Grok or the system).
    Message { msg: Msg },
    /// A message changed (an acknowledgement became the final reply).
    MessageUpdated { msg: Msg },
    /// A Claude turn started: show "Nora is typing".
    TurnStarted { turn_id: String },
    /// Streaming text of the reply being written (not stored; the full text arrives as `Message`).
    Delta { turn_id: String, text: String },
    /// A tool call inside a turn, for a progress line.
    Tool { turn_id: String, name: String, title: String, done: bool },
    TurnDone { turn: TurnRow },
    /// An alarm or reminder is due now. Push delivery comes later.
    AlarmFired { alarm: aos_core::model::Alarm, late_secs: i64 },
    Heartbeat { beat: Heartbeat },
    /// Nora wants to call him (stub: the call runtime comes in phase D).
    CallRequested { call: Call },
    CallMissed { call: Call },
    /// Something was queued for the Grok Bot write side.
    GrokQueued { id: i64, kind: String },
}

#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<NoraEvent>,
}

impl Bus {
    pub fn new() -> Bus {
        Bus { tx: broadcast::channel(512).0 }
    }

    pub fn send(&self, e: NoraEvent) {
        let _ = self.tx.send(e);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<NoraEvent> {
        self.tx.subscribe()
    }
}

impl Default for Bus {
    fn default() -> Self {
        Bus::new()
    }
}
