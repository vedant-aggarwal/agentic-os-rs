//! Nora, the assistant inside Agentic OS (ARCHITECTURE.md §5).
//!
//! - [`nora::Nora`]: the runtime. One merged thread ([`store`]), the engine switch
//!   (auto / claude / grok), and a turn worker that runs Claude ([`engine`]) one turn at a time.
//! - [`grok`]: the Grok Bot read side (transcript watcher) and the write-side seam.
//! - [`memory`]: long-term facts, short-term loops, daily episodes, nightly consolidation.
//! - [`scheduler`]: alarms to the second, missed calls, the hourly [`heartbeat`], consolidation.
//! - [`tools`]: registry tools; [`http`]: `/api/nora/*` routes; [`events`]: the live bus.
//!
//! ```ignore
//! let mut cfg = aos_nora::NoraConfig::from_env(db_path);
//! cfg.mcp_url = Some("http://127.0.0.1:8787/mcp".into());
//! let nora = aos_nora::Nora::start(cfg)?;
//! registry.extend(aos_nora::tools::tools(&nora));
//! let routes = aos_nora::http::router_with_auth(nora.clone(), auth_store);
//! ```

pub mod call;
pub mod engine;
pub mod events;
pub mod grok;
#[cfg(windows)]
pub mod grok_uia;
pub mod grok_write;
pub mod heartbeat;
pub mod http;
pub mod memory;
pub mod mind;
pub mod mission;
pub mod morning;
pub mod nora;
pub mod prompt;
pub mod realtime;
pub mod scheduler;
pub mod store;
pub mod tools;
pub mod wave;

pub use events::NoraEvent;
pub use nora::{EngineChoice, Nora, NoraConfig, Settings};
