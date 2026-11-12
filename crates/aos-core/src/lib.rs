//! Agentic OS core: the domain (areas, tasks, habits, XP, mood, journal, alarms),
//! the rules (streaks, levels, rollups) and the SQLite store they live in.
//!
//! Everything here is synchronous and plain Rust. The server wraps calls in
//! `tokio::task::spawn_blocking`; the CLI calls them directly.
//!
//! Layout:
//! - [`store`]: opening the database, migrations, the single writer, the change log.
//! - one module per domain area, each adding methods to [`Store`].
//! - pure rule modules with unit tests: [`streak`], [`xp`], [`tasks::rollup`].

pub mod alarms;
pub mod areas;
pub mod devices;
pub mod error;
pub mod goals;
pub mod habits;
pub mod ids;
pub mod journal;
pub mod mind;
pub mod mission;
pub mod model;
pub mod mood;
pub mod paths;
pub mod seed;
pub mod store;
pub mod streak;
pub mod sync;
pub mod tasks;
pub mod time;
pub mod today;
pub mod wave;
pub mod xp;

pub use error::{Error, Result};
pub use store::Store;
