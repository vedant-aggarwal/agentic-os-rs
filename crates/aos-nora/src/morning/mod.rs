//! The morning print ritual (SYSTEM.md section 3).
//!
//! At the set time (05:00) the PC makes a 3-page A4 sheet (schedule, habits, tasks by area, inbox
//! items, Nora's analysis of yesterday, a news brief) and prints it silently. He marks it with a pen,
//! photographs it and sends the photo to Nora, who applies the marks ([`intake`]).
//!
//! - [`sheet`]: the data, the HTML and the PDF (headless Edge or Chrome).
//! - [`print`]: SumatraPDF `-print-to`, the printer list and the choice of printer.
//! - [`intake`]: photo to changes to tools.
//! - This file: settings, the scheduled run, the `print_morning`, `morning_settings` and
//!   `plan_photo` tools, and the HTTP routes.

pub mod intake;
pub mod print;
pub mod sheet;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use aos_tools::{Risk, Tool, ToolCtx, ToolError};
use chrono::{Local, NaiveDate, Timelike};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::engine::{TurnRequest, ask};
use crate::nora::Nora;
use crate::store::NewMsg;
use sheet::{Analysis, Sheet};

/// A missed 05:00 (PC off) is still printed on boot until this hour.
pub const CATCH_UP_UNTIL_HOUR: u32 = 11;

/// Where sheets, PDFs and photos are kept (`~/.aos/print`, or `AOS_PRINT_DIR`).
pub fn print_dir() -> PathBuf {
    std::env::var_os("AOS_PRINT_DIR").map(PathBuf::from).unwrap_or_else(|| aos_core::paths::aos_home().join("print"))
}

// ---------------------------------------------------------------------------------------------
// Settings

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MorningSettings {
    /// The daily 05:00 job. Off switch.
    pub enabled: bool,
    /// `HH:MM`, local.
    pub time: String,
    /// Printer name; empty = the Windows default if it is a real printer, else the first real one.
    pub printer: String,
    /// Include Nora's analysis of yesterday (one cheap model turn).
    pub analysis: bool,
    /// Include the news brief (one model turn with web search).
    pub news: bool,
}

impl Default for MorningSettings {
    fn default() -> Self {
        // Off until the owner turns it on: the first real print is their call.
        MorningSettings { enabled: false, time: "05:00".into(), printer: String::new(), analysis: true, news: true }
    }
}

pub fn parse_time(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

pub fn settings(nora: &Nora) -> Result<MorningSettings> {
    let d = MorningSettings::default();
    let db = nora.db();
    let flag = |k: &str, dflt: bool| -> Result<bool> { Ok(db.get_kv(k)?.map(|v| v == "true").unwrap_or(dflt)) };
    Ok(MorningSettings {
        enabled: flag("morning_enabled", d.enabled)?,
        time: db.get_kv("morning_time")?.filter(|t| parse_time(t).is_some()).unwrap_or(d.time),
        printer: db.get_kv("morning_printer")?.unwrap_or_default(),
        analysis: flag("morning_analysis", d.analysis)?,
        news: flag("morning_news", d.news)?,
    })
}

pub fn set_settings(nora: &Nora, s: &MorningSettings) -> Result<MorningSettings> {
    let (h, m) = parse_time(&s.time).context("time must look like 05:00")?;
    let db = nora.db();
    db.set_kv("morning_enabled", &s.enabled.to_string())?;
    db.set_kv("morning_time", &format!("{h:02}:{m:02}"))?;
    db.set_kv("morning_printer", s.printer.trim())?;
    db.set_kv("morning_analysis", &s.analysis.to_string())?;
    db.set_kv("morning_news", &s.news.to_string())?;
    settings(nora)
}

// ---------------------------------------------------------------------------------------------
// Stored sheets (the photo intake matches marks against these)

pub fn save_sheet(nora: &Nora, sheet: &Sheet) -> Result<()> {
    let text = serde_json::to_string(sheet)?;
    nora.db().set_kv(&format!("morning_sheet:{}", sheet.id), &text)?;
    nora.db().set_kv("morning_sheet_latest", &sheet.id)?;
    Ok(())
}

pub fn load_sheet(nora: &Nora, id: Option<&str>) -> Result<Option<Sheet>> {
    let id = match id.filter(|s| !s.trim().is_empty()) {
        Some(i) => i.trim().to_string(),
        None => match nora.db().get_kv("morning_sheet_latest")? {
            Some(i) => i,
            None => return Ok(None),
        },
    };
    Ok(nora.db().get_kv(&format!("morning_sheet:{id}"))?.and_then(|t| serde_json::from_str(&t).ok()))
}

// ---------------------------------------------------------------------------------------------
// The model turns

const ANALYSIS_SYSTEM: &str = "You are Nora, a direct and warm assistant writing the analysis of yesterday that is printed at the top of \
her owner's morning sheet. Use only the facts you are given; never invent a number, a reason or a feeling. \
A miss gets a reason, never blame. If no reason was logged, say so plainly. Plain words, no markdown, no emoji.";

fn analysis_prompt(facts: &str) -> String {
    format!(
        "{facts}\nWrite the analysis in exactly these five labelled lines, each one or two short sentences (about 90 words in total):\n\
DONE: what got done.\nMISSED: what was missed and the reason he gave for each (or that none was given).\n\
MOOD: mood and energy if logged, otherwise say it was not logged.\nPATTERN: one pattern worth noticing, from the facts only.\n\
TODAY: the one thing that matters most this morning."
    )
}

/// One cheap Claude turn over yesterday's facts. Falls back to a facts-only summary.
pub async fn analyse(nora: &Nora, day: NaiveDate) -> (Analysis, bool) {
    let yesterday = day - chrono::Duration::days(1);
    let n2 = nora.clone();
    let facts = tokio::task::spawn_blocking(move || sheet::day_facts(&n2, yesterday)).await.ok().and_then(|r| r.ok()).unwrap_or_default();
    let req = TurnRequest {
        system: ANALYSIS_SYSTEM.into(),
        prompt: analysis_prompt(&facts),
        model: Some(nora.config().cheap_model.clone()),
        timeout: Some(Duration::from_secs(120)),
        ..Default::default()
    };
    match ask(nora.engine().as_ref(), req).await {
        Ok(text) => (sheet::parse_analysis(&text), true),
        Err(e) => {
            warn!("morning analysis: {e}");
            let n3 = nora.clone();
            (sheet::fallback_analysis(&n3, yesterday), false)
        }
    }
}

/// One Claude turn with web search for a short news brief. `None` when it is unavailable.
pub async fn news(nora: &Nora, day: NaiveDate) -> Option<String> {
    let req = TurnRequest {
        system: "You write a short morning news brief for a student in Delhi, India. Use web search for today's news. \
Output only the bullets, one per line, each starting with '- '. No intro, no sources, no markdown, no emoji."
            .into(),
        prompt: format!(
            "Today is {}. Give 5 bullets: 2 top India headlines, 1 Indian markets/economy, 1 technology or AI, 1 world. Each bullet at most 20 words, factual, no opinion.",
            day.format("%A %-d %B %Y")
        ),
        model: Some(nora.config().cheap_model.clone()),
        web: true,
        timeout: Some(Duration::from_secs(150)),
        ..Default::default()
    };
    match ask(nora.engine().as_ref(), req).await {
        Ok(t) => {
            let lines: Vec<String> = t.lines().map(|l| l.trim()).filter(|l| l.starts_with(['-', '*', '•'])).map(String::from).collect();
            (!lines.is_empty()).then(|| lines.join("\n"))
        }
        Err(e) => {
            warn!("morning news skipped: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The run

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Day to print (default today).
    pub day: Option<NaiveDate>,
    /// Make the PDF but do not print it.
    pub no_print: bool,
    /// Printer override.
    pub printer: Option<String>,
    /// Override the settings for analysis and news (used by tests and the dry run).
    pub analysis: Option<bool>,
    pub news: Option<bool>,
}

/// Build, render and (unless told not to) print the sheet. Posts the outcome to the thread.
pub async fn run(nora: &Nora, opts: RunOptions) -> Result<Value> {
    let r = run_inner(nora, &opts).await;
    if let Err(e) = &r {
        let _ = nora.post(NewMsg::new("system", "morning", "error", format!("Morning sheet failed: {}", crate::prompt::clip(&format!("{e:#}"), 300))));
    }
    r
}

async fn run_inner(nora: &Nora, opts: &RunOptions) -> Result<Value> {
    let cfg = settings(nora)?;
    let day = opts.day.unwrap_or_else(aos_core::time::local_today);
    let (want_analysis, want_news) = (opts.analysis.unwrap_or(cfg.analysis), opts.news.unwrap_or(cfg.news));

    let n2 = nora.clone();
    let mut sheet = tokio::task::spawn_blocking(move || sheet::gather(&n2, day)).await??;
    let (a, n) = tokio::join!(
        async {
            if want_analysis { Some(analyse(nora, day).await) } else { None }
        },
        async {
            if want_news { news(nora, day).await } else { None }
        }
    );
    let analysis_ok = a.as_ref().map(|x| x.1).unwrap_or(false);
    sheet.analysis = a.map(|x| x.0);
    sheet.news = n;
    save_sheet(nora, &sheet)?;

    let dir = print_dir();
    std::fs::create_dir_all(&dir)?;
    let html = dir.join(format!("morning-{}.html", sheet.id));
    let pdf = dir.join(format!("morning-{}.pdf", sheet.id));
    std::fs::write(&html, sheet::render_html(&sheet))?;
    let pages = sheet::html_to_pdf(&html, &pdf).await?;

    let mut printed_on = None;
    if !opts.no_print {
        let printers = print::list_printers().await;
        let want = opts.printer.clone().unwrap_or(cfg.printer.clone());
        let printer = print::choose(&printers, &want).context("no printer found. Set one in the Print plan sheet.")?;
        anyhow::ensure!(printer.physical || !want.is_empty(), "only virtual printers were found; choose a real printer");
        print::print_pdf(&pdf, &printer.name).await.with_context(|| format!("printing to {}", printer.name))?;
        printed_on = Some(printer);
    }
    let pages_txt = pages.map(|p| format!("{p} pages")).unwrap_or_else(|| "the sheet".into());
    let text = match &printed_on {
        Some(p) => format!("Printed your morning sheet ({pages_txt}) on {}{}.", p.name, if p.offline { ". The printer looks offline; the job waits in its queue" } else { "" }),
        None => format!("Made your morning sheet ({pages_txt}); not printed."),
    };
    nora.post(NewMsg::new("claude", "morning", "message", text.clone()))?;
    info!(sheet = %sheet.id, "morning sheet done");
    Ok(json!({
        "sheet": sheet.id,
        "pdf": pdf.to_string_lossy(),
        "pages": pages,
        "printed": printed_on.is_some(),
        "printer": printed_on.map(|p| p.name),
        "analysis": if sheet.analysis.is_none() { "off" } else if analysis_ok { "model" } else { "facts only" },
        "news": sheet.news.is_some(),
        "message": text,
    }))
}

// ---------------------------------------------------------------------------------------------
// Schedule

/// Called by the scheduler loop every few seconds. Starts the job once per day at the set time
/// (or on boot, if the PC was off then, until [`CATCH_UP_UNTIL_HOUR`]). The run is spawned so a
/// slow model turn never delays alarms.
pub fn tick(nora: &Nora) -> Result<bool> {
    if !claim_run(nora)? {
        return Ok(false);
    }
    let n = nora.clone();
    tokio::spawn(async move {
        if let Err(e) = run(&n, RunOptions::default()).await {
            warn!("morning run: {e:#}");
        }
    });
    Ok(true)
}

/// True once per day when the job is switched on and its time has come. Records the day first, so
/// a crash mid-run never prints twice.
fn claim_run(nora: &Nora) -> Result<bool> {
    let s = settings(nora)?;
    if !s.enabled {
        return Ok(false);
    }
    let Some((h, m)) = parse_time(&s.time) else { return Ok(false) };
    let now = Local::now();
    let due = (now.hour(), now.minute()) >= (h, m) && now.hour() < CATCH_UP_UNTIL_HOUR.max(h + 1);
    let key = now.format("%Y-%m-%d").to_string();
    if !due || nora.db().get_kv("morning_last_run")?.as_deref() == Some(&key) {
        return Ok(false);
    }
    nora.db().set_kv("morning_last_run", &key)?;
    Ok(true)
}

// ---------------------------------------------------------------------------------------------
// Tools

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PrintArgs {
    /// Wait until it has printed and return the result (default: start it and return at once; a
    /// message in Nora's thread says when it is done).
    #[serde(default)]
    pub wait: bool,
    /// Only make the PDF, do not print.
    #[serde(default)]
    pub no_print: bool,
    /// Printer name (default: the setting).
    #[serde(default)]
    pub printer: Option<String>,
    /// Day to print: today (default), tomorrow, 2026-11-13.
    #[serde(default)]
    pub date: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SettingsArgs {
    /// Turn the daily job on or off.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Time of day, 05:00.
    #[serde(default)]
    pub time: Option<String>,
    /// Printer name; empty for automatic.
    #[serde(default)]
    pub printer: Option<String>,
    /// Include Nora's analysis of yesterday.
    #[serde(default)]
    pub analysis: Option<bool>,
    /// Include the news brief.
    #[serde(default)]
    pub news: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PhotoArgs {
    /// Path of the photo on the server's PC (JPEG, PNG or WebP).
    pub path: String,
    /// Sheet id (the date on the sheet, 2026-11-13); default the newest.
    #[serde(default)]
    pub sheet: Option<String>,
}

fn err(e: anyhow::Error) -> ToolError {
    ToolError::InvalidArgs(format!("{e:#}"))
}

/// Run a future from a synchronous tool handler. Handlers run on blocking threads, but a fresh
/// thread keeps this safe from any context.
fn block_on<T: Send>(handle: &tokio::runtime::Handle, fut: impl std::future::Future<Output = T> + Send) -> T {
    std::thread::scope(|s| s.spawn(|| handle.block_on(fut)).join().expect("worker thread panicked"))
}

pub fn tools(nora: &Nora) -> Vec<Tool> {
    let handle = tokio::runtime::Handle::current();
    let (n1, n2, n3) = (nora.clone(), nora.clone(), nora.clone());
    let h1 = handle.clone();
    vec![
        Tool::new(
            "print_morning",
            "Make today's morning sheet (schedule, habits, tasks by area, messages, Nora's analysis of yesterday, news) and print it on the configured printer. Returns at once; a message in Nora's thread says when it printed. Pass wait=true to wait, no_print=true for the PDF only.",
            Risk::Tell,
            move |_ctx: &ToolCtx<'_>, a: PrintArgs| {
                let day = a.date.as_deref().map(|d| aos_core::time::parse_day(d, aos_core::time::local_today())).transpose()?;
                let opts = RunOptions { day, no_print: a.no_print, printer: a.printer.filter(|p| !p.trim().is_empty()), ..Default::default() };
                if a.wait {
                    return block_on(&h1, run(&n1, opts)).map_err(err);
                }
                let n = n1.clone();
                h1.spawn(async move {
                    if let Err(e) = run(&n, opts).await {
                        warn!("print_morning: {e:#}");
                    }
                });
                Ok(json!({ "status": "started", "note": "Nora posts in the thread when the sheet is printed." }))
            },
        ),
        Tool::new(
            "morning_settings",
            "Show or change the morning print: on/off, time, printer, whether to include the analysis and the news. Call with no arguments to read.",
            Risk::Tell,
            move |_ctx: &ToolCtx<'_>, a: SettingsArgs| {
                let mut s = settings(&n2).map_err(err)?;
                if let Some(v) = a.enabled {
                    s.enabled = v;
                }
                if let Some(v) = a.time {
                    s.time = v;
                }
                if let Some(v) = a.printer {
                    s.printer = v;
                }
                if let Some(v) = a.analysis {
                    s.analysis = v;
                }
                if let Some(v) = a.news {
                    s.news = v;
                }
                Ok(json!(set_settings(&n2, &s).map_err(err)?))
            },
        ),
        Tool::new(
            "plan_photo",
            "Read a photo of the marked-up morning sheet (a file on the server's PC), apply the ticks, new tasks, moves and notes through the tools, and post a summary in Nora's thread.",
            Risk::Tell,
            move |_ctx: &ToolCtx<'_>, a: PhotoArgs| {
                let bytes = std::fs::read(&a.path).map_err(|e| ToolError::InvalidArgs(format!("cannot read {}: {e}", a.path)))?;
                block_on(&handle, intake::process(&n3, bytes, a.sheet.as_deref())).map_err(err)
            },
        )
        .positional(&["path"]),
    ]
}

// ---------------------------------------------------------------------------------------------
// HTTP

pub mod http {
    //! | Method | Path | |
    //! |---|---|---|
    //! | POST | `/api/nora/photo?sheet=` | the image as the request body (JPEG, PNG, WebP); up to 25 MB |
    //! | POST | `/api/nora/print-morning` | `{wait?, no_print?, printer?}`: same as the tool |
    //! | GET/POST | `/api/nora/morning-settings` | [`super::MorningSettings`] |
    //! | GET | `/api/nora/printers` | printers on the PC and which one would be used |

    use axum::body::Bytes;
    use axum::extract::{DefaultBodyLimit, Query, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use serde::Deserialize;
    use serde_json::json;

    use super::{MorningSettings, RunOptions};
    use crate::nora::Nora;

    pub fn router(nora: Nora) -> Router {
        Router::new()
            .route("/api/nora/photo", post(photo).layer(DefaultBodyLimit::max(25 * 1024 * 1024)))
            .route("/api/nora/print-morning", post(print_now))
            .route("/api/nora/morning-settings", get(get_settings).post(set_settings))
            .route("/api/nora/printers", get(printers))
            .with_state(nora)
    }

    fn fail(code: StatusCode, e: anyhow::Error) -> Response {
        (code, Json(json!({ "error": { "code": "morning", "message": format!("{e:#}") } }))).into_response()
    }

    #[derive(Deserialize)]
    struct PhotoQ {
        sheet: Option<String>,
    }

    async fn photo(State(n): State<Nora>, Query(q): Query<PhotoQ>, body: Bytes) -> Response {
        match super::intake::process(&n, body.to_vec(), q.sheet.as_deref()).await {
            Ok(v) => Json(v).into_response(),
            Err(e) => fail(StatusCode::BAD_REQUEST, e),
        }
    }

    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct PrintBody {
        wait: bool,
        no_print: bool,
        printer: Option<String>,
    }

    async fn print_now(State(n): State<Nora>, body: Option<Json<PrintBody>>) -> Response {
        let b = body.map(|j| j.0).unwrap_or_default();
        let opts = RunOptions { no_print: b.no_print, printer: b.printer.filter(|p| !p.trim().is_empty()), ..Default::default() };
        if b.wait {
            return match super::run(&n, opts).await {
                Ok(v) => Json(v).into_response(),
                Err(e) => fail(StatusCode::BAD_REQUEST, e),
            };
        }
        tokio::spawn(async move {
            let _ = super::run(&n, opts).await;
        });
        Json(json!({ "status": "started" })).into_response()
    }

    async fn get_settings(State(n): State<Nora>) -> Response {
        match super::settings(&n) {
            Ok(s) => Json(s).into_response(),
            Err(e) => fail(StatusCode::BAD_REQUEST, e),
        }
    }

    async fn set_settings(State(n): State<Nora>, Json(s): Json<MorningSettings>) -> Response {
        match super::set_settings(&n, &s) {
            Ok(s) => Json(s).into_response(),
            Err(e) => fail(StatusCode::BAD_REQUEST, e),
        }
    }

    async fn printers(State(n): State<Nora>) -> Response {
        let list = super::print::list_printers().await;
        let configured = super::settings(&n).map(|s| s.printer).unwrap_or_default();
        let chosen = super::print::choose(&list, &configured).map(|p| p.name);
        Json(json!({ "printers": list, "chosen": chosen })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nora::testing::nora_with;

    #[test]
    fn settings_default_and_round_trip() {
        let (nora, _e, _p) = nora_with(&[], None);
        let s = settings(&nora).unwrap();
        assert!(!s.enabled && s.time == "05:00" && s.analysis && s.news);
        let s = set_settings(&nora, &MorningSettings { enabled: false, time: "6:5".into(), printer: " Home ".into(), analysis: false, news: true }).unwrap();
        assert!(!s.enabled);
        assert_eq!((s.time.as_str(), s.printer.as_str(), s.analysis), ("06:05", "Home", false));
        assert!(set_settings(&nora, &MorningSettings { time: "25:00".into(), ..s }).is_err());
    }

    #[test]
    fn claim_runs_once_per_day_and_respects_the_switch() {
        let (nora, _e, _p) = nora_with(&[], None);
        let now = Local::now();
        // Set the time to the start of this hour so it is due now (unless the hour is past the window).
        let t = format!("{:02}:00", now.hour());
        set_settings(&nora, &MorningSettings { enabled: false, time: t.clone(), printer: "Test PDF".into(), analysis: false, news: false }).unwrap();
        assert!(!claim_run(&nora).unwrap(), "off switch");
        set_settings(&nora, &MorningSettings { enabled: true, time: t, printer: "Test PDF".into(), analysis: false, news: false }).unwrap();
        assert!(claim_run(&nora).unwrap());
        assert!(!claim_run(&nora).unwrap(), "only once per day");
        // A time later today is not due.
        nora.db().set_kv("morning_last_run", "").unwrap();
        let later = (now.hour() + 1).min(23);
        if later > now.hour() {
            set_settings(&nora, &MorningSettings { time: format!("{later:02}:30"), ..settings(&nora).unwrap() }).unwrap();
            assert!(!claim_run(&nora).unwrap());
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_makes_a_pdf_when_a_browser_exists() {
        if sheet::find_browser().is_none() {
            return;
        }
        let analysis = "DONE: Two tasks.\nMISSED: Gym (tired).\nMOOD: Not logged.\nPATTERN: Evenings slip.\nTODAY: Start with gym.";
        let (nora, engine, _p) = nora_with(&[analysis], None);
        let out = run(&nora, RunOptions { no_print: true, news: Some(false), ..Default::default() }).await.unwrap();
        assert_eq!(out["pages"], 3);
        assert_eq!(out["printed"], false);
        assert_eq!(out["analysis"], "model");
        assert!(std::path::Path::new(out["pdf"].as_str().unwrap()).is_file());
        assert!(engine.seen.lock().unwrap()[0].prompt.contains("Date:"));
        assert!(load_sheet(&nora, None).unwrap().unwrap().analysis.unwrap().missed.contains("tired"));
    }
}
