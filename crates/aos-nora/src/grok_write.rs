//! Grok Bot write side: the queue runner.
//!
//! Everything Nora sends to Grok (his messages with the Grok engine, 1-3 line summaries of Claude
//! turns, `grok_ask` hand-offs) waits in `nora_grok_outbox`. [`run`] works through it in order:
//!
//! 1. dedupe: if the typed text is already in Nora's transcript (a retry after a missed
//!    confirmation), mark it sent and move on;
//! 2. skip while Grok is mid-reply (last transcript entry still streaming);
//! 3. type and send through the [`GrokWriter`] (UI Automation);
//! 4. confirm by watching the transcript file for the new `user` entry with the same text.
//!
//! Locked Windows, a closed app and a busy composer are waits, not failures. Real failures back
//! off (10 s, 20 s, 40 s ...) and the item is marked `failed` after [`MAX_ATTEMPTS`].
//! The state is kept in the kv table (`grok_write_state`) and served at `GET /api/nora/grok`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::events::NoraEvent;
use crate::grok::{GrokConfig, GrokEntry, GrokWriter, NotYetWriter, WriteError, parse_transcript, transcript_path};
use crate::nora::Nora;
use crate::store::{NoraStore, OutboxItem, now_ms};

pub const MAX_ATTEMPTS: u32 = 6;
const CONFIRM_SECS: u64 = 25;
const STREAM_STALE_MS: i64 = 5 * 60_000;

/// The writer for this machine: UI Automation on Windows when the Grok folder exists and
/// `AOS_GROK_WRITE` is not `0`; otherwise one that refuses (the outbox just keeps the items).
pub fn default_writer(cfg: &GrokConfig) -> std::sync::Arc<dyn GrokWriter> {
    #[cfg(windows)]
    if cfg.dir.is_some() && std::env::var("AOS_GROK_WRITE").map(|v| v != "0").unwrap_or(true) {
        return std::sync::Arc::new(crate::grok_uia::UiaWriter::new(&cfg.chat_name));
    }
    let _ = cfg;
    std::sync::Arc::new(NotYetWriter)
}

/// What is typed into the chat for an outbox item. One line: the composer sends on Enter.
pub fn typed_text(kind: &str, text: &str) -> String {
    let body = norm(text);
    match kind {
        "summary" => format!("[Claude Nora] {body}"),
        "ask" => format!("[Claude Nora to Grok] {body}"),
        _ => body,
    }
}

/// Collapse whitespace (newlines included) to single spaces.
pub fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn same(a: &str, b: &str) -> bool {
    let (a, b) = (norm(a).to_lowercase(), norm(b).to_lowercase());
    a == b || (a.len() > 60 && b.starts_with(&a.chars().take(60).collect::<String>()))
}

/// Is this `user` entry one of ours (typed by the write side) rather than something he typed in
/// Grok Bot himself? Each outbox item claims at most one entry. Idempotent per entry id.
pub fn is_own_echo(db: &NoraStore, e: &GrokEntry) -> bool {
    let key = format!("grok_echo:{}", e.id);
    if db.get_kv(&key).ok().flatten().is_some() {
        return true;
    }
    let Ok(items) = db.grok_outbox_since(e.at - 20 * 60_000) else { return false };
    for it in items {
        let claim = format!("grok_claim:{}", it.id);
        if db.get_kv(&claim).ok().flatten().is_some() || !same(&typed_text(&it.kind, &it.text), &e.text) {
            continue;
        }
        let _ = db.set_kv(&claim, &e.id);
        let _ = db.set_kv(&key, &it.id.to_string());
        return true;
    }
    false
}

fn user_entries(cfg: &GrokConfig, chat_id: &str) -> Vec<GrokEntry> {
    let Some(dir) = &cfg.dir else { return vec![] };
    let Some(path) = transcript_path(dir, chat_id) else { return vec![] };
    std::fs::read(path).ok().and_then(|b| parse_transcript(&b).ok()).unwrap_or_default()
}

/// Is Grok mid-reply? The newest transcript entry is a streaming partial (parse skips those, so
/// read the raw file).
fn grok_streaming(cfg: &GrokConfig, chat_id: &str) -> bool {
    let Some(path) = cfg.dir.as_ref().and_then(|d| transcript_path(d, chat_id)) else { return false };
    let Ok(body) = std::fs::read(path) else { return false };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) else { return false };
    v["value"]["entries"].as_array().is_some_and(|es| {
        es.iter().rev().take(3).any(|e| {
            e["isStreaming"].as_bool() == Some(true) && e["timestampMs"].as_i64().is_some_and(|t| now_ms() - t < STREAM_STALE_MS)
        })
    })
}

fn confirmed(cfg: &GrokConfig, chat_id: &str, typed: &str, since_ms: i64) -> bool {
    user_entries(cfg, chat_id).iter().any(|e| e.role == "user" && e.at >= since_ms && same(&e.text, typed))
}

/// Outcome of one pass over the queue.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pass {
    pub sent: usize,
    pub waiting: Option<String>,
}

#[derive(Default)]
pub struct Backoff {
    attempts: HashMap<i64, u32>,
    not_before: HashMap<i64, Instant>,
}

fn set_state(nora: &Nora, state: &str, detail: &str) {
    let queued = nora.db().grok_queue(100).map(|q| q.len()).unwrap_or(0);
    let v = json!({ "state": state, "detail": detail, "queued": queued, "at": now_ms(), "writer": nora.grok_writer().describe() });
    let _ = nora.db().set_kv("grok_write_state", &v.to_string());
}

/// One pass over the queue. Blocking (UI Automation and file polling): run it on a blocking thread.
pub fn process_once(nora: &Nora, back: &mut Backoff) -> Result<Pass> {
    let cfg = nora.config().grok.clone();
    let writer = nora.grok_writer().clone();
    let mut pass = Pass::default();
    let items: Vec<OutboxItem> = nora.db().grok_queue(20)?;
    if items.is_empty() {
        if nora.db().get_kv("grok_write_state")?.is_none_or(|s| !s.contains("\"idle\"")) {
            set_state(nora, "idle", "");
        }
        return Ok(pass);
    }
    let chat_id = cfg.chat_id.clone().or_else(|| cfg.dir.as_deref().and_then(|d| crate::grok::chat_id_from_roster(d, &cfg.chat_name)));
    for item in items {
        if back.not_before.get(&item.id).is_some_and(|t| Instant::now() < *t) {
            pass.waiting = Some("backing off".into());
            break; // keep the order
        }
        let typed = typed_text(&item.kind, &item.text);
        if let Some(cid) = &chat_id {
            // A retry after a missed confirmation: it may have gone through after all.
            if confirmed(&cfg, cid, &typed, item.created_at - 1000) {
                nora.db().mark_grok(item.id, "sent", None)?;
                pass.sent += 1;
                continue;
            }
            if grok_streaming(&cfg, cid) {
                pass.waiting = Some("Grok is replying".into());
                set_state(nora, "busy", "Grok is replying");
                break;
            }
        }
        set_state(nora, "sending", &format!("item {}", item.id));
        let started = now_ms();
        match writer.send(&typed) {
            Ok(()) => {
                let ok = match &chat_id {
                    Some(cid) => {
                        let end = Instant::now() + Duration::from_secs(CONFIRM_SECS);
                        let mut hit = false;
                        while Instant::now() < end {
                            if confirmed(&cfg, cid, &typed, started - 2000) {
                                hit = true;
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(400));
                        }
                        hit
                    }
                    None => true, // no transcript to watch: trust the send
                };
                if ok {
                    nora.db().mark_grok(item.id, "sent", None)?;
                    back.attempts.remove(&item.id);
                    pass.sent += 1;
                    info!(id = item.id, "grok message confirmed");
                } else {
                    // Pressed send but nothing showed up. Retry later; the dedupe above catches a late arrival.
                    fail_once(nora, back, &item, "sent but not seen in the transcript")?;
                    pass.waiting = Some("unconfirmed".into());
                    break;
                }
            }
            Err(WriteError::Locked) => {
                back.not_before.insert(item.id, Instant::now() + Duration::from_secs(30));
                set_state(nora, "locked", "Windows is locked; queued until he unlocks");
                pass.waiting = Some("locked".into());
                break;
            }
            Err(WriteError::NotRunning(m)) => {
                back.not_before.insert(item.id, Instant::now() + Duration::from_secs(20));
                set_state(nora, "not_running", &m);
                pass.waiting = Some(m);
                break;
            }
            Err(WriteError::Busy(m)) => {
                back.not_before.insert(item.id, Instant::now() + Duration::from_secs(6));
                set_state(nora, "busy", &m);
                pass.waiting = Some(m);
                break;
            }
            Err(WriteError::Failed(m)) => {
                fail_once(nora, back, &item, &m)?;
                pass.waiting = Some(m);
                break;
            }
        }
    }
    if pass.waiting.is_none() {
        set_state(nora, "idle", "");
    }
    Ok(pass)
}

fn fail_once(nora: &Nora, back: &mut Backoff, item: &OutboxItem, msg: &str) -> Result<()> {
    let n = back.attempts.entry(item.id).or_insert(0);
    *n += 1;
    if *n >= MAX_ATTEMPTS {
        nora.db().mark_grok(item.id, "failed", Some(msg))?;
        back.attempts.remove(&item.id);
        set_state(nora, "error", &format!("gave up on item {}: {msg}", item.id));
        warn!(id = item.id, "grok send gave up: {msg}");
    } else {
        let secs = 10u64 << (*n - 1).min(5);
        back.not_before.insert(item.id, Instant::now() + Duration::from_secs(secs));
        set_state(nora, "error", &format!("retrying in {secs}s: {msg}"));
        debug!(id = item.id, "grok send failed ({n}): {msg}");
    }
    Ok(())
}

/// The runner: wakes every 3 s and whenever something is queued.
pub async fn run(nora: Nora) {
    let shutdown = nora.shutdown_token();
    let mut events = nora.bus().subscribe();
    let mut back = Backoff::default();
    loop {
        let n = nora.clone();
        let res = tokio::task::spawn_blocking(move || {
            let r = process_once(&n, &mut back);
            (r, back)
        })
        .await;
        match res {
            Ok((r, b)) => {
                back = b;
                if let Err(e) = r {
                    warn!("grok write: {e:#}");
                }
            }
            Err(e) => {
                warn!("grok write task: {e}");
                back = Backoff::default();
            }
        }
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
            Ok(NoraEvent::GrokQueued { .. }) = events.recv() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nora::testing::nora_with;
    use std::sync::Mutex;

    struct Fake {
        results: Mutex<Vec<Result<(), WriteError>>>,
        sent: Mutex<Vec<String>>,
    }
    impl GrokWriter for Fake {
        fn send(&self, text: &str) -> Result<(), WriteError> {
            self.sent.lock().unwrap().push(text.to_string());
            let mut r = self.results.lock().unwrap();
            if r.is_empty() { Ok(()) } else { r.remove(0) }
        }
    }

    fn with_writer(w: std::sync::Arc<dyn GrokWriter>) -> Nora {
        let (nora, _e, _p) = nora_with(&[], None);
        nora.set_grok_writer(w);
        nora
    }

    #[test]
    fn formats_typed_text() {
        assert_eq!(typed_text("message", "hello\n  there"), "hello there");
        assert_eq!(typed_text("summary", "Done."), "[Claude Nora] Done.");
        assert_eq!(typed_text("ask", "what?"), "[Claude Nora to Grok] what?");
    }

    #[test]
    fn sends_in_order_and_marks_sent_without_a_transcript() {
        let fake = std::sync::Arc::new(Fake { results: Mutex::new(vec![]), sent: Mutex::new(vec![]) });
        let nora = with_writer(fake.clone());
        nora.db().queue_grok("message", "one").unwrap();
        nora.db().queue_grok("summary", "two").unwrap();
        let p = process_once(&nora, &mut Backoff::default()).unwrap();
        assert_eq!(p.sent, 2);
        assert_eq!(*fake.sent.lock().unwrap(), vec!["one", "[Claude Nora] two"]);
        assert!(nora.db().grok_queue(10).unwrap().is_empty());
    }

    #[test]
    fn locked_waits_without_counting_and_keeps_the_queue() {
        let fake = std::sync::Arc::new(Fake { results: Mutex::new(vec![Err(WriteError::Locked)]), sent: Mutex::new(vec![]) });
        let nora = with_writer(fake.clone());
        nora.db().queue_grok("message", "later").unwrap();
        let mut back = Backoff::default();
        let p = process_once(&nora, &mut back).unwrap();
        assert_eq!(p.sent, 0);
        assert_eq!(p.waiting.as_deref(), Some("locked"));
        assert_eq!(nora.db().grok_queue(10).unwrap().len(), 1);
        assert!(nora.db().get_kv("grok_write_state").unwrap().unwrap().contains("locked"));
        // Inside the wait window nothing is tried again.
        process_once(&nora, &mut back).unwrap();
        assert_eq!(fake.sent.lock().unwrap().len(), 1);
    }

    #[test]
    fn real_failures_back_off_then_give_up() {
        let fails = (0..MAX_ATTEMPTS).map(|_| Err(WriteError::Failed("x".into()))).collect();
        let fake = std::sync::Arc::new(Fake { results: Mutex::new(fails), sent: Mutex::new(vec![]) });
        let nora = with_writer(fake);
        let item = nora.db().queue_grok("message", "m").unwrap();
        let mut back = Backoff::default();
        for _ in 0..MAX_ATTEMPTS {
            back.not_before.clear();
            process_once(&nora, &mut back).unwrap();
        }
        assert!(nora.db().grok_queue(10).unwrap().is_empty());
        assert_eq!(nora.db().grok_item(item.id).unwrap().status, "failed");
    }

    #[test]
    fn echo_of_our_message_is_not_imported_as_his_words() {
        let (nora, _e, _p) = nora_with(&[], None);
        let it = nora.db().queue_grok("summary", "Alarm set.").unwrap();
        let e = GrokEntry { id: "e1".into(), role: "user", text: "[Claude Nora] Alarm set.".into(), at: now_ms() };
        assert!(is_own_echo(nora.db(), &e));
        assert!(is_own_echo(nora.db(), &e), "idempotent for the same entry");
        // The item claimed one entry; the same words typed by him later are his.
        let again = GrokEntry { id: "e2".into(), ..e.clone() };
        assert!(!is_own_echo(nora.db(), &again));
        let other = GrokEntry { id: "e3".into(), role: "user", text: "something else".into(), at: now_ms() };
        assert!(!is_own_echo(nora.db(), &other));
        let _ = it;
    }

    #[test]
    fn confirms_from_a_transcript_file() {
        use data_encoding::BASE32_NOPAD;
        let dir = std::env::temp_dir().join(format!("aos-grokw-{}", aos_core::ids::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = "sand.client.slice.account.a.transcript.replicas.c2";
        let file = format!("{}.blob", BASE32_NOPAD.encode(name.as_bytes()).to_ascii_lowercase());
        let body = format!(
            r#"{{"value":{{"entries":[{{"kind":"message","id":"u1","role":"user","content":"hello   there","isStreaming":false,"timestampMs":{},"seq":1}}]}}}}"#,
            now_ms()
        );
        std::fs::write(dir.join(file), body).unwrap();
        let cfg = GrokConfig { dir: Some(dir.clone()), chat_id: Some("c2".into()), chat_name: "Nora".into(), backfill_hours: 1 };
        assert!(confirmed(&cfg, "c2", "hello there", now_ms() - 5000));
        assert!(!confirmed(&cfg, "c2", "nope", now_ms() - 5000));
        assert!(!grok_streaming(&cfg, "c2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Live check against the real Grok Bot (types into Nora's chat). Run by hand:
    /// `cargo test -p aos-nora live_roundtrip -- --ignored --nocapture`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_roundtrip() {
        use crate::nora::{Nora, NoraConfig};
        let path = std::env::temp_dir().join(format!("aos-nora-live-{}.db", aos_core::ids::new_id()));
        let cfg = NoraConfig::from_env(path);
        let engine = std::sync::Arc::new(crate::nora::testing::FakeEngine { texts: vec![], tool: None, seen: Mutex::new(vec![]) });
        let nora = Nora::new(cfg, engine).unwrap();
        let item = nora.db().queue_grok("summary", "[test from Agentic OS, ignore] queue check 2").unwrap();
        let p = process_once(&nora, &mut Backoff::default()).unwrap();
        println!("pass: {p:?}; state: {:?}", nora.db().get_kv("grok_write_state").unwrap());
        assert_eq!(nora.db().grok_item(item.id).unwrap().status, "sent");
        // The read side must not take our own line for his words.
        let cfgg = &nora.config().grok;
        let cid = crate::grok::chat_id_from_roster(cfgg.dir.as_deref().unwrap(), &cfgg.chat_name).unwrap();
        crate::grok::ingest(&nora, &cid, &transcript_path(cfgg.dir.as_deref().unwrap(), &cid).unwrap()).unwrap();
        let thread = nora.thread(50, None).unwrap();
        assert!(!thread.iter().any(|m| m.text.contains("queue check 2")), "echo leaked into the thread");
    }
}
