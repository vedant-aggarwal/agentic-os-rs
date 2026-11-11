//! Agentic OS tools.
//!
//! - [`registry`]: the `Tool` type and the `Registry` (ARCHITECTURE.md §2, "one tool registry,
//!   three doors").
//! - [`tools`]: the tools themselves (tasks, habits, mood, journal, alarms, XP, today).
//! - [`mcp`]: the Model Context Protocol message handling, shared by the stdio server in the
//!   CLI and the HTTP endpoint in the server.
//! - [`cli_args`]: turning `aos task add Buy milk --due tomorrow` into tool arguments.
//!
//! ## Adding tools from another crate
//!
//! ```ignore
//! let mut registry = aos_tools::registry();
//! registry.extend(aos_work::tools(supervisor.clone())); // Vec<Tool>
//! aos_server::serve_with(config, registry, |r| r).await?;
//! ```
//! A tool is `Tool::new(name, description, risk, handler)` where the handler is any
//! `Fn(&ToolCtx, Input) -> Result<Output, ToolError> + Send + Sync + 'static`. Handlers run on a
//! blocking thread; capture your own state (an `Arc<Supervisor>`, a `tokio::runtime::Handle`
//! for async work) in the closure. `Input` derives `Deserialize + JsonSchema`, `Output`
//! derives `Serialize`; the schema, CLI parsing and MCP listing come for free.
//!
//! Tool names use underscores (`task_add`), because Claude's tool-name rule allows only
//! letters, digits, `_` and `-`. `task.add` is accepted as an alias everywhere.

pub mod cli_args;
pub mod mcp;
pub mod registry;
pub mod tools;

pub use registry::{Registry, Risk, Tool, ToolCtx, ToolError};

/// The registry with every built-in tool.
pub fn registry() -> Registry {
    Registry::new(tools::all())
}
