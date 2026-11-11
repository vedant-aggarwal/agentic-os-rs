//! Child processes: one-shot commands and supervised long-running services.
//!
//! - Services run inside a **Job Object** on Windows (process group on Unix), so killing one
//!   takes its whole tree and nothing outlives the server. No console window flashes.
//! - [`supervise`] restarts a service with exponential backoff (1 s up to 60 s; the backoff
//!   resets once a run lasted a minute) and stops it when the cancel token fires.
//! - [`run_capture`] runs a command to completion with a timeout and returns its output.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use process_wrap::tokio::*;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const STDERR_KEEP: usize = 30;

/// What to run.
#[derive(Debug, Clone, Default)]
pub struct Spec {
    pub bin: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, OsString)>,
}

impl Spec {
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Spec { bin: bin.into(), ..Default::default() }
    }
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
    pub fn env(mut self, k: &str, v: impl Into<OsString>) -> Self {
        self.env.push((k.to_string(), v.into()));
        self
    }
}

/// Where a supervised service is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Running,
    /// Exited; waiting before the next start.
    Backoff,
    Stopped,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServiceStatus {
    pub phase: Phase,
    pub restarts: u32,
    pub last_error: Option<String>,
}

/// Shared view of a supervised service, readable from tools and tests.
#[derive(Clone)]
pub struct ServiceHandle(Arc<Mutex<ServiceStatus>>);

impl ServiceHandle {
    pub fn new() -> Self {
        ServiceHandle(Arc::new(Mutex::new(ServiceStatus {
            phase: Phase::Starting,
            restarts: 0,
            last_error: None,
        })))
    }
    pub fn status(&self) -> ServiceStatus {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn update(&self, f: impl FnOnce(&mut ServiceStatus)) {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

impl Default for ServiceHandle {
    fn default() -> Self {
        Self::new()
    }
}

fn start(spec: &Spec) -> std::io::Result<(Box<dyn ChildWrapper>, Arc<Mutex<VecDeque<String>>>)> {
    let mut cmd = CommandWrap::with_new(&spec.bin, |c| {
        c.args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        for (k, v) in &spec.env {
            c.env(k, v);
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
    let mut child = cmd.spawn()?;
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(stderr) = child.stderr().take() {
        let tail = tail.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                if t.len() == STDERR_KEEP {
                    t.pop_front();
                }
                t.push_back(line);
            }
        });
    }
    Ok((child, tail))
}

/// Keep `spec` running until `cancel` fires. `make` is called before every start so a service
/// can pick a fresh port or token each time. Returns when stopped.
pub async fn supervise(
    name: &'static str,
    cancel: CancellationToken,
    handle: ServiceHandle,
    mut make: impl FnMut() -> Option<Spec>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if cancel.is_cancelled() {
            break;
        }
        handle.update(|s| s.phase = Phase::Starting);
        let Some(spec) = make() else {
            // Not ready to start (for instance WhatsApp is not paired yet). Look again later.
            handle.update(|s| s.phase = Phase::Backoff);
            if sleep_or_cancel(&cancel, Duration::from_secs(15)).await {
                break;
            }
            continue;
        };
        let began = Instant::now();
        match start(&spec) {
            Ok((mut child, tail)) => {
                info!(service = name, "started");
                handle.update(|s| s.phase = Phase::Running);
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = child.start_kill();
                        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
                        break;
                    }
                    status = child.wait() => {
                        let err = {
                            let t = tail.lock().unwrap_or_else(|e| e.into_inner());
                            t.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" | ")
                        };
                        warn!(service = name, ?status, "exited; restarting");
                        handle.update(|s| {
                            s.restarts += 1;
                            s.last_error = Some(if err.is_empty() { format!("{status:?}") } else { err });
                        });
                    }
                }
            }
            Err(e) => {
                warn!(service = name, "could not start {}: {e}", spec.bin.display());
                handle.update(|s| {
                    s.restarts += 1;
                    s.last_error = Some(format!("could not start: {e}"));
                });
            }
        }
        if began.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        handle.update(|s| s.phase = Phase::Backoff);
        if sleep_or_cancel(&cancel, backoff).await {
            break;
        }
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
    handle.update(|s| s.phase = Phase::Stopped);
    info!(service = name, "stopped");
}

/// True when cancelled during the sleep.
pub async fn sleep_or_cancel(cancel: &CancellationToken, d: Duration) -> bool {
    tokio::select! {
        _ = cancel.cancelled() => true,
        _ = tokio::time::sleep(d) => false,
    }
}

/// The result of a finished command.
#[derive(Debug)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// Run to completion (killed on timeout or drop). No window on Windows.
pub async fn run_capture(spec: &Spec, timeout: Duration) -> std::io::Result<Output> {
    let mut cmd = tokio::process::Command::new(&spec.bin);
    cmd.args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = cmd.spawn()?;
    let out = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "command timed out"))??;
    Ok(Output {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command that exits at once with an error.
    fn crash() -> Spec {
        if cfg!(windows) {
            Spec::new("cmd").args(["/C", "exit 3"])
        } else {
            Spec::new("sh").args(["-c", "exit 3"])
        }
    }

    /// A command that keeps running until killed.
    fn sleeper() -> Spec {
        if cfg!(windows) {
            Spec::new("ping").args(["-n", "60", "127.0.0.1"])
        } else {
            Spec::new("sleep").args(["60"])
        }
    }

    #[tokio::test]
    async fn restarts_a_crashing_service_with_backoff() {
        let cancel = CancellationToken::new();
        let handle = ServiceHandle::new();
        let task = tokio::spawn(supervise("t-crash", cancel.clone(), handle.clone(), || Some(crash())));
        tokio::time::sleep(Duration::from_millis(3500)).await;
        let s = handle.status();
        assert!(s.restarts >= 2, "restarted {} times", s.restarts);
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
        assert_eq!(handle.status().phase, Phase::Stopped);
    }

    #[tokio::test]
    async fn cancel_stops_a_running_service_promptly() {
        let cancel = CancellationToken::new();
        let handle = ServiceHandle::new();
        let task = tokio::spawn(supervise("t-run", cancel.clone(), handle.clone(), || Some(sleeper())));
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(handle.status().phase, Phase::Running);
        let t = Instant::now();
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(8), task).await.unwrap().unwrap();
        assert!(t.elapsed() < Duration::from_secs(6));
        assert_eq!(handle.status().phase, Phase::Stopped);
    }

    #[tokio::test]
    async fn run_capture_returns_output_and_times_out() {
        let echo = if cfg!(windows) {
            Spec::new("cmd").args(["/C", "echo hi"])
        } else {
            Spec::new("sh").args(["-c", "echo hi"])
        };
        let out = run_capture(&echo, Duration::from_secs(10)).await.unwrap();
        assert!(out.ok() && out.stdout.contains("hi"));
        let e = run_capture(&sleeper(), Duration::from_millis(300)).await.unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
    }
}
