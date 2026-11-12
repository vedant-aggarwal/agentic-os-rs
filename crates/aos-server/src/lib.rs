//! Agentic OS server.
//!
//! [`serve`] opens the store, starts the change watcher, builds the router and runs Axum with
//! graceful shutdown (Ctrl-C, and SIGTERM on Unix). It takes a closure that can add more routes,
//! which is how the Dioxus app mounts its SSR router next to the API (see `src/server/` in the
//! app crate). The `aos-server` binary runs the API alone.

pub mod auth;
pub mod events;
pub mod hinglish;
pub mod limit;
pub mod media;
pub mod mind;
pub mod mind_fetch;
pub mod personal;
pub mod routes;
pub mod state;
pub mod voice;
pub mod web;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use aos_core::{Store, paths, seed};
use axum::Router;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub use routes::api_router;
pub use state::AppState;

/// Default listen address: local only. Phones reach it through the Cloudflare tunnel or LAN.
pub const DEFAULT_ADDR: &str = "127.0.0.1:8787";

#[derive(Debug, Clone)]
pub struct Config {
    pub db_path: PathBuf,
    pub addr: SocketAddr,
}

impl Config {
    /// From the environment: `AOS_DB`, then `AOS_ADDR`, or `IP` + `PORT` (what `dx serve` sets).
    pub fn from_env() -> anyhow::Result<Config> {
        let addr = match (
            std::env::var("AOS_ADDR"),
            std::env::var("IP"),
            std::env::var("PORT"),
        ) {
            (Ok(a), _, _) => a,
            (_, Ok(ip), Ok(port)) => format!("{ip}:{port}"),
            (_, _, Ok(port)) => format!("127.0.0.1:{port}"),
            _ => DEFAULT_ADDR.to_string(),
        };
        Ok(Config {
            db_path: paths::default_db_path(),
            addr: addr
                .parse()
                .with_context(|| format!("bad listen address '{addr}'"))?,
        })
    }
}

/// Install a tracing subscriber (`RUST_LOG` filters, default `info`). Safe to call twice.
pub fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tower_http=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// Open the store (applying the personal seed to a new database) and wrap it in state.
pub fn open_state(
    db_path: &std::path::Path,
    registry: aos_tools::Registry,
) -> anyhow::Result<AppState> {
    let store = Store::open(db_path).with_context(|| format!("opening {}", db_path.display()))?;
    // A new database, or one another component created first (the app's server opens the Work
    // store before this) that has no goals yet, gets the personal seed.
    if (store.was_created() || store.list_goals(true)?.is_empty())
        && let Some(r) = seed::apply_local(&store, &paths::local_seed_path())?
    {
        info!(?r, "applied personal seed");
    }
    Ok(AppState::with_registry(Arc::new(store), registry))
}

/// Run the server until Ctrl-C / SIGTERM. `extend` receives the API router and may merge more
/// routes into it (the Dioxus SSR router, static files).
pub async fn serve(config: Config, extend: impl FnOnce(Router) -> Router) -> anyhow::Result<()> {
    serve_with(config, aos_tools::registry(), extend).await
}

/// Like [`serve`], with a registry that other crates have added tools to.
pub async fn serve_with(
    config: Config,
    registry: aos_tools::Registry,
    extend: impl FnOnce(Router) -> Router,
) -> anyhow::Result<()> {
    let state = open_state(&config.db_path, registry)?;
    info!(db = %config.db_path.display(), "store open");
    state.tracker.spawn(events::watch_changes(state.clone()));
    // Mind OS: fetch links and make thumbnails for saved items (Claude tagging runs in Nora).
    state.tracker.spawn(mind::worker(state.clone()));

    // The limiter is outermost so it also covers the Work, Nora and UI routes merged by `extend`.
    // CORS sits outside the limiter so the phone shell's preflight requests get answered first.
    let app = limit::guard(extend(api_router(state.clone())), limit::Limits::default()).layer(web::cors());
    let listener = tokio::net::TcpListener::bind(config.addr)
        .await
        .with_context(|| format!("binding {}", config.addr))?;
    info!("listening on http://{}", listener.local_addr()?);

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal(state.shutdown.clone()))
        .await?;

    // Streams ended when the token was cancelled; give background work a moment to finish.
    state.shutdown.cancel();
    state.tracker.close();
    if tokio::time::timeout(Duration::from_secs(10), state.tracker.wait())
        .await
        .is_err()
    {
        warn!("background tasks did not stop within 10 s");
    }
    info!("stopped");
    Ok(())
}

/// Resolves on Ctrl-C, SIGTERM (Unix) or when `token` is cancelled elsewhere; cancels `token`.
pub async fn shutdown_signal(token: CancellationToken) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("Ctrl-C: shutting down"),
        _ = term => info!("SIGTERM: shutting down"),
        _ = token.cancelled() => {}
    }
    token.cancel();
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod limit_tests;
