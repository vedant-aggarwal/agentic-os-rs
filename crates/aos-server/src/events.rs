//! The live event bus.
//!
//! One `tokio::sync::broadcast` channel. A watcher task reads new `change_log` rows and
//! publishes a [`Event::Change`] for each, so writes from anywhere (this server, the `aos` CLI in
//! another process, an import) reach every SSE and WebSocket client. Handlers call
//! [`EventBus::poke`] after a write so the watcher runs immediately instead of on its next tick.
//!
//! A client that falls behind gets [`Event::Resync`] and should pull `/api/sync/changes`.

use std::sync::Arc;
use std::time::Duration;

use aos_core::sync::ChangeNotice;
use serde::Serialize;
use tokio::sync::{Notify, broadcast};
use tracing::warn;

use crate::state::AppState;

const TICK: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// First message on every stream: the newest version right now.
    Hello { latest: i64 },
    /// A row changed. Pull it with `/api/sync/changes?since=<version - 1>` or just resync.
    Change {
        version: i64,
        entity: String,
        id: String,
        op: String,
        actor: Option<String>,
    },
    /// You missed events; pull `/api/sync/changes` from your last version.
    Resync,
}

impl From<ChangeNotice> for Event {
    fn from(n: ChangeNotice) -> Self {
        Event::Change {
            version: n.version,
            entity: n.entity,
            id: n.id,
            op: n.op,
            actor: n.actor,
        }
    }
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
    poke: Arc<Notify>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        EventBus {
            tx,
            poke: Arc::new(Notify::new()),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// Publish to everyone listening. No listeners is fine.
    pub fn send(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    /// Ask the watcher to look at `change_log` now.
    pub fn poke(&self) {
        self.poke.notify_one();
    }
}

/// Watch `change_log` and publish changes until shutdown.
pub async fn watch_changes(state: AppState) {
    let mut last = match state.blocking(|s| Ok(s.latest_version()?)).await {
        Ok(v) => v,
        Err(e) => {
            warn!("change watcher could not start: {e}");
            return;
        }
    };
    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            _ = state.bus.poke.notified() => {}
            _ = tokio::time::sleep(TICK) => {}
        }
        loop {
            let since = last;
            match state
                .blocking(move |s| Ok(s.change_notices(since, 500)?))
                .await
            {
                Ok(notices) if notices.is_empty() => break,
                Ok(notices) => {
                    let full = notices.len() == 500;
                    for n in notices {
                        last = n.version;
                        state.bus.send(n.into());
                    }
                    if !full {
                        break;
                    }
                }
                Err(e) => {
                    warn!("change watcher: {e}");
                    break;
                }
            }
        }
    }
}
