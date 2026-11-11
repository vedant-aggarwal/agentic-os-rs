//! Agentic OS Work tab: projects (folders), threads with Claude and Codex, approvals, diffs.
//! A Rust port of T3 Code's orchestration model (MIT, see `NOTICE`); map in
//! `design/specs/work-t3-port.md`.
//!
//! - [`types`]: shared with every client, including wasm (no runtime deps).
//! - With the `runtime` feature (server only): [`supervisor::Work`] runs the CLIs
//!   ([`claude`], [`codex`]) under [`proc`], persists to [`store`], streams [`types::WorkEvent`]s,
//!   and exposes [`http`] routes and registry [`tools`].

pub mod types;

#[cfg(feature = "runtime")]
pub mod claude;
#[cfg(feature = "runtime")]
pub mod codex;
#[cfg(feature = "runtime")]
pub mod config;
#[cfg(feature = "runtime")]
pub mod diff;
#[cfg(feature = "runtime")]
pub mod error;
#[cfg(feature = "runtime")]
pub mod http;
#[cfg(feature = "runtime")]
pub mod proc;
#[cfg(feature = "runtime")]
pub mod session;
#[cfg(feature = "runtime")]
pub mod store;
#[cfg(feature = "runtime")]
pub mod supervisor;
#[cfg(feature = "runtime")]
pub mod t3import;
#[cfg(feature = "runtime")]
pub mod tools;

#[cfg(feature = "runtime")]
pub use config::WorkConfig;
#[cfg(feature = "runtime")]
pub use error::WorkError;
#[cfg(feature = "runtime")]
pub use supervisor::Work;
/// Foundation's store, re-exported so the app can build the auth layer without its own dep.
#[cfg(feature = "runtime")]
pub use aos_core::Store as AuthStore;
