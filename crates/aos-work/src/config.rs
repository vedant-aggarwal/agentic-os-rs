//! Where things live. Nothing personal is hardcoded: every path comes from the environment or is
//! derived from the home folder at runtime.
//!
//! | Variable | Default |
//! |---|---|
//! | `AOS_WORK_DB` | the main Agentic OS database (`AOS_DB`, default `~/.aos/aos.db`); Work's tables are prefixed `work_` |
//! | `AOS_CLAUDE_BIN` | `claude.exe` / `claude` found on PATH or in `~/.local/bin` |
//! | `AOS_CODEX_BIN` | `codex.exe` on PATH, else the native binary inside the npm `@openai/codex` package |
//! | `AOS_T3_DB` | `~/.t3/userdata/statev2.sqlite`, else `state.sqlite` (read only, for importing projects) |
//! | `AOS_WORK_IDLE_MINUTES` | `30`: idle CLI processes are stopped after this and resumed on the next message |

use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct WorkConfig {
    pub db_path: PathBuf,
    pub claude_bin: Option<PathBuf>,
    pub codex_bin: Option<PathBuf>,
    pub t3_db: Option<PathBuf>,
    pub idle_timeout: Duration,
}

impl WorkConfig {
    pub fn from_env() -> WorkConfig {
        let env_path = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let db_path = env_path("AOS_WORK_DB").unwrap_or_else(default_db_path);
        let idle = std::env::var("AOS_WORK_IDLE_MINUTES").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(30);
        WorkConfig {
            db_path,
            claude_bin: env_path("AOS_CLAUDE_BIN").or_else(find_claude),
            codex_bin: env_path("AOS_CODEX_BIN").or_else(find_codex),
            t3_db: env_path("AOS_T3_DB").or_else(default_t3_db).filter(|p| p.exists()),
            idle_timeout: Duration::from_secs(idle.max(1) * 60),
        }
    }

    /// For tests: a database at `db_path`, no T3 import, binaries from the environment.
    pub fn for_test(db_path: PathBuf) -> WorkConfig {
        WorkConfig { db_path, t3_db: None, ..WorkConfig::from_env() }
    }
}

/// The same file as the rest of Agentic OS (foundation's `aos_core::paths`).
pub fn default_db_path() -> PathBuf {
    aos_core::paths::default_db_path()
}

fn default_t3_db() -> Option<PathBuf> {
    let dir = dirs::home_dir()?.join(".t3").join("userdata");
    ["statev2.sqlite", "state.sqlite"].iter().map(|f| dir.join(f)).find(|p| p.exists())
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// First match for `file` in the PATH folders.
fn on_path(file: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(file)).find(|p| p.is_file())
}

/// The real `claude` binary. On Windows the npm shims (`claude.cmd`, `.ps1`) can't be spawned
/// safely with arguments (BatBadBut, tech-stack R12), so only a real `.exe` counts.
fn find_claude() -> Option<PathBuf> {
    let local = dirs::home_dir().map(|h| h.join(".local").join("bin").join(exe("claude")));
    local.filter(|p| p.is_file()).or_else(|| on_path(&exe("claude")))
}

/// The real `codex` binary. npm installs a Node launcher; the native binary sits in the
/// platform package next to it, so spawn that directly.
fn find_codex() -> Option<PathBuf> {
    if let Some(p) = on_path(&exe("codex")) {
        return Some(p);
    }
    if !cfg!(windows) {
        // On Linux/macOS the npm `codex` script has a shebang and can be spawned as is.
        return on_path("codex");
    }
    let shim_dir = on_path("codex.cmd")?.parent()?.to_path_buf();
    let triple = "x86_64-pc-windows-msvc";
    let candidates = [
        shim_dir.join("node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor"),
        shim_dir.join("node_modules/@openai/codex-win32-x64/vendor"),
        shim_dir.join("node_modules/@openai/codex/vendor"),
    ];
    candidates
        .iter()
        .map(|v| v.join(triple).join("bin").join("codex.exe"))
        .find(|p| p.is_file())
}

/// A display form of a path with the home folder shortened to `~` (for logs and the UI).
pub fn tilde(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display());
        }
    }
    path.display().to_string()
}
