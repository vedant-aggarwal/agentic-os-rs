//! Shared server state, handed to every handler.

use std::sync::Arc;

use aos_core::Store;
use aos_tools::{Registry, ToolCtx, ToolError};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::events::EventBus;

/// Cheap to clone: everything inside is reference counted.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub registry: Arc<Registry>,
    pub bus: EventBus,
    /// Background work that shutdown should wait for.
    pub tracker: TaskTracker,
    /// Cancelled when the server starts shutting down; long-lived streams end on it.
    pub shutdown: CancellationToken,
    /// Voice takes on disk and the speech-to-text engine (Codex).
    pub voice: Arc<crate::voice::Voice>,
}

impl AppState {
    /// State with the built-in tools.
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_registry(store, aos_tools::registry())
    }

    /// State with a registry you extended (e.g. with `aos-work` tools).
    pub fn with_registry(store: Arc<Store>, registry: Registry) -> Self {
        AppState {
            store,
            registry: Arc::new(registry),
            bus: EventBus::new(1024),
            tracker: TaskTracker::new(),
            shutdown: CancellationToken::new(),
            voice: Arc::new(crate::voice::Voice::from_env()),
        }
    }

    /// Run blocking store code on Tokio's blocking pool (SQLite calls are synchronous).
    pub async fn blocking<T, F>(&self, f: F) -> Result<T, ToolError>
    where
        T: Send + 'static,
        F: FnOnce(&Store) -> Result<T, ToolError> + Send + 'static,
    {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || f(&store))
            .await
            .map_err(|e| {
                ToolError::Core(std::io::Error::other(format!("worker failed: {e}")).into())
            })?
    }

    /// Call a tool as `actor`, then wake the change watcher so clients hear about it at once.
    pub async fn call_tool(
        &self,
        actor: String,
        name: String,
        args: Value,
    ) -> Result<Value, ToolError> {
        let registry = self.registry.clone();
        let result = self
            .blocking(move |store| {
                let ctx = ToolCtx::new(store, &actor);
                registry.call(&ctx, &name, args)
            })
            .await;
        self.bus.poke();
        result
    }
}
