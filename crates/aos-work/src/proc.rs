//! Child processes for the CLIs (tech-stack R12).
//!
//! - Spawned through `process-wrap`: on Windows inside a **Job Object** so killing it takes the
//!   whole tree (node, shells, MCP servers the CLI started) and nothing outlives the server; on
//!   Unix in its own process group. `KillOnDrop` covers panics.
//! - No console window flashes on Windows (`CREATE_NO_WINDOW`).
//! - Real executables only (no `.cmd`/`.bat` shims; see `config.rs`).
//! - stdout is read as lines (lossy UTF-8, any length) into a channel; stdin is written by one
//!   writer task fed by a channel, so nobody holds a lock across `.await`.
//! - The last stderr lines are kept for error messages.

use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use process_wrap::tokio::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::debug;

use crate::error::{Result, WorkError};

const STDERR_KEEP: usize = 40;

pub struct ChildProc {
    child: Box<dyn ChildWrapper>,
    stdin: mpsc::UnboundedSender<String>,
    /// stdout, one line per message. Closed when the process exits.
    pub lines: mpsc::UnboundedReceiver<String>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    pub pid: Option<u32>,
}

/// Start `bin args...` in `cwd`. `env_remove` drops variables that would confuse a nested CLI.
pub fn spawn(bin: &Path, args: &[String], cwd: &Path, env_remove: &[&str]) -> Result<ChildProc> {
    if !cwd.is_dir() {
        return Err(WorkError::Invalid(format!("folder does not exist: {}", cwd.display())));
    }
    let mut cmd = CommandWrap::with_new(bin, |c| {
        c.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for k in env_remove {
            c.env_remove(k);
        }
    });
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        cmd.wrap(CreationFlags(CREATE_NO_WINDOW));
        cmd.wrap(JobObject);
    }
    #[cfg(unix)]
    {
        cmd.wrap(ProcessGroup::leader());
    }
    cmd.wrap(KillOnDrop);

    let mut child = cmd
        .spawn()
        .map_err(|e| WorkError::Provider(format!("could not start {}: {e}", bin.display())))?;
    let pid = child.id();

    let mut stdin = child.stdin().take().ok_or_else(|| WorkError::Provider("no stdin".into()))?;
    let stdout = child.stdout().take().ok_or_else(|| WorkError::Provider("no stdout".into()))?;
    let stderr = child.stderr().take();

    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(mut line) = in_rx.recv().await {
            if !line.ends_with('\n') {
                line.push('\n');
            }
            if stdin.write_all(line.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
        }
        // Dropping stdin closes it: both CLIs exit on EOF.
    });

    let (out_tx, out_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut reader = BufReader::with_capacity(64 * 1024, stdout);
        let mut buf = Vec::with_capacity(8 * 1024);
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf);
                    let line = line.trim();
                    if !line.is_empty() && out_tx.send(line.to_string()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_KEEP)));
    if let Some(stderr) = stderr {
        let tail = tail.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                debug!(target: "aos_work::stderr", "{line}");
                let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                if t.len() == STDERR_KEEP {
                    t.pop_front();
                }
                t.push_back(line);
            }
        });
    }

    Ok(ChildProc { child, stdin: in_tx, lines: out_rx, stderr: tail, pid })
}

impl ChildProc {
    /// Queue one line for stdin. False when the writer is gone (process exited).
    pub fn send(&self, line: String) -> bool {
        self.stdin.send(line).is_ok()
    }

    pub fn send_json(&self, value: &serde_json::Value) -> bool {
        self.send(value.to_string())
    }

    /// The last stderr lines, joined (for error messages).
    pub fn stderr_tail(&self, max_lines: usize) -> String {
        let t = self.stderr.lock().unwrap_or_else(|e| e.into_inner());
        let skip = t.len().saturating_sub(max_lines);
        t.iter().skip(skip).cloned().collect::<Vec<_>>().join("\n")
    }

    /// Kill the whole process tree and reap it.
    pub async fn kill(&mut self) {
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }

    /// Ask nicely first (close stdin), then kill after `grace`.
    pub async fn shutdown(&mut self, grace: Duration) {
        let (dead_tx, _) = mpsc::unbounded_channel();
        self.stdin = dead_tx; // drops the writer's sender: stdin closes
        if tokio::time::timeout(grace, self.child.wait()).await.is_err() {
            self.kill().await;
        }
    }
}
