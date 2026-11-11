//! Server functions for the Work tab. Each one calls the process-wide `aos_work::Work`
//! supervisor (started in `src/server`). The live stream is a typed SSE server function, so the
//! same code works on web, desktop and mobile.

use aos_work::types::*;
use dioxus::fullstack::ServerEvents;
use dioxus::prelude::*;

#[cfg(feature = "server")]
fn work() -> Result<aos_work::Work, ServerFnError> {
    aos_work::Work::global().ok_or_else(|| ServerFnError::new("The Work supervisor is not running on this server."))
}

#[cfg(feature = "server")]
fn e(err: aos_work::WorkError) -> ServerFnError {
    ServerFnError::new(err.to_string())
}

#[server]
pub async fn work_projects() -> Result<Vec<Project>, ServerFnError> {
    work()?.projects().map_err(e)
}

#[server]
pub async fn work_add_project(folder: String) -> Result<Project, ServerFnError> {
    work()?.add_project("", &folder).map_err(e)
}

#[server]
pub async fn work_threads() -> Result<Vec<ThreadSummary>, ServerFnError> {
    work()?.threads(None).map_err(e)
}

#[server]
pub async fn work_thread(id: String) -> Result<ThreadDetail, ServerFnError> {
    work()?.thread(&id).map_err(e)
}

#[server]
pub async fn work_create(req: NewThread) -> Result<ThreadSummary, ServerFnError> {
    work()?.create_thread(req).map_err(e)
}

#[server]
pub async fn work_send(id: String, text: String) -> Result<ThreadSummary, ServerFnError> {
    work()?.send(&id, &text).map_err(e)
}

#[server]
pub async fn work_decide(id: String, decision: Decision) -> Result<Approval, ServerFnError> {
    work()?.decide(&id, decision).map_err(e)
}

#[server]
pub async fn work_interrupt(id: String) -> Result<ThreadSummary, ServerFnError> {
    work()?.interrupt(&id).map_err(e)
}

/// Returns the started threads and the error text of any task that failed.
#[server]
pub async fn work_launch(req: LaunchMany) -> Result<(Vec<ThreadSummary>, Vec<String>), ServerFnError> {
    let results = work()?.launch(req).map_err(e)?;
    let mut ok = Vec::new();
    let mut failed = Vec::new();
    for r in results {
        match r {
            Ok(t) => ok.push(t),
            Err(err) => failed.push(err),
        }
    }
    Ok((ok, failed))
}

#[server]
pub async fn work_diff(id: String) -> Result<ThreadDiff, ServerFnError> {
    let w = work()?;
    tokio::task::spawn_blocking(move || w.diff(&id))
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))?
        .map_err(e)
}

/// Every Work event, live. Starts with `Hello`; `Resync` means refetch.
#[get("/api/work_stream")]
pub async fn work_stream() -> Result<ServerEvents<WorkEvent>, ServerFnError> {
    let w = work()?;
    Ok(ServerEvents::from_stream(aos_work::http::event_results(&w)))
}
