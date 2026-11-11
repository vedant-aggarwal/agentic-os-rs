//! `aos`: Agentic OS from the terminal.
//!
//! ```text
//! aos                                 today's overview (same as `aos today`)
//! aos task add Buy milk --due tomorrow
//! aos task done 7k2m --evidence "commit abc123"
//! aos habit log journal floor
//! aos today --json                    compact JSON for agents
//! aos tools                           every tool (from the running server when AOS_TOKEN is set)
//! aos mcp                             MCP server over stdio (for Claude, Codex, Grok Bot)
//! aos pair new | token --name X | list | revoke <id>
//! aos seed [path]                     apply a seed file (default ~/.aos/seed.local.toml)
//! ```
//!
//! The CLI works on the database file directly, so it runs with or without the server. The
//! server notices CLI writes through `change_log` and pushes them to devices.

mod remote;
mod render;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use aos_core::{Store, paths, seed};
use aos_tools::mcp::{self, McpAction};
use aos_tools::{Registry, ToolCtx, ToolError, cli_args};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(
    name = "aos",
    version,
    about = "Agentic OS from the terminal.",
    after_help = "Any tool: aos <group> <verb> [words] [--option value]   e.g. aos task add Buy milk --due tomorrow\n\
                  List tools with `aos tools`, one tool's options with `aos describe task add`."
)]
struct Cli {
    /// Database file [default: %USERPROFILE%\.aos\aos.db]
    #[arg(long, global = true, env = "AOS_DB")]
    db: Option<PathBuf>,
    /// Compact JSON output for scripts and agents
    #[arg(long, global = true)]
    json: bool,
    /// Who is acting, recorded in the change log [default: cli, or mcp for `aos mcp`]
    #[arg(long, global = true, env = "AOS_ACTOR")]
    actor: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every tool
    Tools,
    /// Show one tool's options: `aos describe task add`
    Describe { tool: Vec<String> },
    /// Run an MCP server on stdin/stdout
    Mcp,
    /// Pair devices and manage their tokens
    Pair {
        #[command(subcommand)]
        cmd: PairCmd,
    },
    /// Apply a seed file (pillars, areas, projects, habits, goals)
    Seed { path: Option<PathBuf> },
    #[command(external_subcommand)]
    Tool(Vec<String>),
}

#[derive(Subcommand)]
enum PairCmd {
    /// Make a 6-digit code for a phone or desktop to claim
    New {
        #[arg(long, default_value_t = aos_core::devices::DEFAULT_CODE_TTL_MINUTES)]
        minutes: i64,
    },
    /// Make a device token directly (for local agents such as Grok Bot)
    Token {
        #[arg(long)]
        name: String,
        #[arg(long)]
        platform: Option<String>,
    },
    /// List paired devices
    List,
    /// Revoke a device; the only way a device loses access
    Revoke { id: String },
}

fn main() -> ExitCode {
    let mut cli = Cli::parse();
    // Options typed after a tool's words reach us inside the raw tool args; pull them out.
    if let Some(Cmd::Tool(words)) = &mut cli.cmd {
        let mut kept = Vec::new();
        let mut it = std::mem::take(words).into_iter();
        while let Some(w) = it.next() {
            match w.as_str() {
                "--json" => cli.json = true,
                "--db" => cli.db = it.next().map(PathBuf::from),
                "--actor" => cli.actor = it.next(),
                _ => kept.push(w),
            }
        }
        *words = kept;
    }
    let json = cli.json;
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let code = e
                .downcast_ref::<ToolError>()
                .map_or("error", ToolError::code);
            if json {
                println!(
                    "{}",
                    json!({ "error": { "code": code, "message": format!("{e:#}") } })
                );
            } else {
                eprintln!("error: {e:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn open_store(db: Option<PathBuf>) -> Result<Store> {
    let path = db.unwrap_or_else(paths::default_db_path);
    let store = Store::open(&path).with_context(|| format!("opening {}", path.display()))?;
    if store.was_created()
        && let Some(r) = seed::apply_local(&store, &paths::local_seed_path())?
    {
        eprintln!("applied personal seed: {r:?}");
    }
    Ok(store)
}

fn print(json: bool, value: &Value, human: impl FnOnce(&Value) -> String) {
    if json {
        println!("{value}");
    } else {
        println!("{}", human(value).trim_end());
    }
}

fn run(cli: Cli) -> Result<()> {
    let mut registry = aos_tools::registry();
    registry.extend(aos_inbox::tools::tools());
    let json = cli.json;
    match cli.cmd.unwrap_or_else(|| Cmd::Tool(vec!["today".into()])) {
        Cmd::Tools => {
            // The running server has more tools (Work, Nora) than this process; ask it first.
            let (tools, from): (Vec<Value>, &str) = match remote::tools() {
                Some(t) => (t, "server"),
                None => (registry.tools().iter().map(|t| t.describe()).collect(), "local"),
            };
            if json {
                println!("{}", json!({ "tools": tools, "from": from }));
            } else {
                for t in &tools {
                    println!(
                        "{:<14} {:<5} {}",
                        t["name"].as_str().unwrap_or("").replacen('_', " ", 1),
                        t["risk"].as_str().unwrap_or(""),
                        t["description"].as_str().unwrap_or("")
                    );
                }
                if from == "local" {
                    eprintln!("(local tools only: set AOS_TOKEN to list the running server's tools)");
                }
            }
        }
        Cmd::Describe { tool } => {
            let (t, _) =
                cli_args::resolve(&registry, &tool).context("no such tool (see `aos tools`)")?;
            print(json, &t.describe(), |_| render::describe(t));
        }
        Cmd::Mcp => {
            let store = open_store(cli.db)?;
            mcp_stdio(&registry, &store, cli.actor.as_deref().unwrap_or("mcp"))?;
        }
        Cmd::Pair { cmd } => {
            let store = open_store(cli.db)?;
            pair(&store, cmd, json)?;
        }
        Cmd::Seed { path } => {
            let store = open_store(cli.db)?;
            let path = path.unwrap_or_else(paths::local_seed_path);
            let report = seed::apply_local(&store, &path)?
                .with_context(|| format!("no seed file at {}", path.display()))?;
            print(json, &json!(report), |v| {
                format!("seeded from {}: {v}", path.display())
            });
        }
        Cmd::Tool(words) if words.starts_with(&["inbox".to_string(), "whatsapp".to_string()]) => {
            inbox_whatsapp(&words[2..])?;
        }
        Cmd::Tool(words) => {
            let Some((tool, used)) = cli_args::resolve(&registry, &words) else {
                bail!("unknown command '{}' (see `aos tools`)", words.join(" "));
            };
            if words[used..].iter().any(|w| w == "--help" || w == "-h") {
                println!("{}", render::describe(tool));
                return Ok(());
            }
            let args = cli_args::parse(tool, &words[used..])?;
            let store = open_store(cli.db)?;
            let ctx = ToolCtx::new(&store, cli.actor.as_deref().unwrap_or("cli"));
            let out = tool.call(&ctx, args)?;
            print(json, &out, |v| render::tool_output(tool.name, v));
        }
    }
    Ok(())
}

/// `aos inbox whatsapp pair`: link this PC to WhatsApp once, with a QR code in this terminal.
fn inbox_whatsapp(rest: &[String]) -> Result<()> {
    let cfg = aos_inbox::InboxConfig::load().whatsapp;
    match rest.first().map(String::as_str) {
        Some("pair") => {
            print!("{}", aos_inbox::whatsapp::pair_instructions(&cfg));
            let status = aos_inbox::whatsapp::pair_blocking(&cfg)
                .with_context(|| format!("could not run {} (install the Go wacli, see design/INBOX-BUILD.md)", cfg.bin().display()))?;
            if !status.success() {
                bail!("pairing did not finish ({status})");
            }
            println!("Linked. Start the server; WhatsApp messages now flow into the inbox.");
            Ok(())
        }
        _ => bail!("usage: aos inbox whatsapp pair"),
    }
}

fn pair(store: &Store, cmd: PairCmd, json: bool) -> Result<()> {
    match cmd {
        PairCmd::New { minutes } => {
            let code = store.create_pairing_code(minutes)?;
            print(json, &json!(code), |_| {
                format!(
                    "Pairing code: {}\nValid until {} (UTC). Enter it in the app, or POST /api/pair/claim.",
                    code.code, code.expires_at
                )
            });
        }
        PairCmd::Token { name, platform } => {
            let paired = store.create_device_token(&name, platform.as_deref())?;
            print(json, &json!(paired), |_| {
                format!(
                    "Device {} ({}) paired.\nToken (shown once): {}\nUse it as: Authorization: Bearer <token>",
                    paired.device.name, paired.device.id, paired.token
                )
            });
        }
        PairCmd::List => {
            let devices = store.list_devices()?;
            print(json, &json!({ "devices": devices }), |_| {
                devices
                    .iter()
                    .map(|d| {
                        let state = if d.revoked_at.is_some() {
                            "revoked"
                        } else {
                            "active"
                        };
                        format!(
                            "{}  {:<20} {:<8} last seen {}",
                            d.id,
                            d.name,
                            state,
                            d.last_seen_at.as_deref().unwrap_or("never")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            });
        }
        PairCmd::Revoke { id } => {
            let d = store.revoke_device(&id)?;
            print(json, &json!(d), |_| {
                format!("revoked {} ({})", d.name, d.id)
            });
        }
    }
    Ok(())
}

/// MCP over stdio: one JSON-RPC message per line in, one per line out. Logs go to stderr so
/// stdout stays clean protocol.
fn mcp_stdio(registry: &Registry, store: &Store, actor: &str) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match mcp::route_text(registry, &line) {
            McpAction::Respond(v) => Some(v),
            McpAction::Nothing => None,
            McpAction::Call { id, name, args } => {
                let ctx = ToolCtx::new(store, actor);
                Some(mcp::call_response(id, registry.call(&ctx, &name, args)))
            }
        };
        if let Some(v) = reply {
            writeln!(stdout, "{v}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
