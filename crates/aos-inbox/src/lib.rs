//! Agentic OS inbox backend.
//!
//! - [`store`]: `inbox_item` and `inbox_reply` in the main database; every write lands in
//!   `change_log`, so the server's event bus pushes inbox changes to clients.
//! - [`whatsapp`]: Go `wacli` as a supervised child (live messages in by webhook, confirmed
//!   replies out).
//! - [`gmail`]: read-only polling through the `gws` CLI, one block per account.
//! - [`triage`]: asks the local Laya service whether Nora should read a message; falls back to
//!   "yes" when it is down.
//! - [`tools`]: `inbox_list`, `inbox_mark`, `inbox_reply` (risk `ask`), `inbox_status`.
//!
//! [`Inbox::start`] runs it all under one cancel token; `stop` (or dropping the token's owner)
//! ends the children, killing their process trees.

pub mod config;
pub mod gmail;
pub mod proc;
pub mod store;
pub mod tools;
pub mod triage;
pub mod whatsapp;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aos_core::Store;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub use config::InboxConfig;
pub use store::{Item, NewItem, Source};

/// A running inbox: the workers and the handles tools use to report on them.
#[derive(Clone)]
pub struct Inbox {
    pub store: Arc<Store>,
    pub config: Arc<InboxConfig>,
    pub triage: triage::Triage,
    pub whatsapp: whatsapp::WhatsappState,
    pub laya: proc::ServiceHandle,
    cancel: CancellationToken,
}

impl Inbox {
    /// Open the database at `db_path` and start every enabled worker on the current Tokio runtime.
    pub fn start(db_path: &Path, config: InboxConfig) -> aos_core::Result<Inbox> {
        let store = Arc::new(Store::open(db_path)?);
        store::ensure_schema(&store)?;
        let inbox = Inbox {
            triage: triage::Triage::new(config.triage.clone()),
            config: Arc::new(config),
            store,
            whatsapp: whatsapp::WhatsappState::new(),
            laya: proc::ServiceHandle::new(),
            cancel: CancellationToken::new(),
        };
        inbox.spawn_workers();
        Ok(inbox)
    }

    /// End every worker and child process. Safe to call twice.
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Triage a stored item in the background and record the verdict.
    pub fn triage_in_background(&self, item: Item) {
        if item.from_me {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move { this.triage_item(&item).await });
    }

    async fn triage_item(&self, item: &Item) {
        let store = self.store.clone();
        let it = item.clone();
        let recent = tokio::task::spawn_blocking(move || store::recent_chat(&store, &it, 5))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let who = if item.sender_name.is_empty() { &item.sender } else { &item.sender_name };
        let text = if item.subject.is_empty() {
            item.text.clone()
        } else {
            format!("{}: {}", item.subject, item.text)
        };
        let v = self.triage.classify(&text, who, &recent).await;
        let store = self.store.clone();
        let id = item.id.clone();
        let _ = tokio::task::spawn_blocking(move || store::set_triage(&store, &id, v.score, v.yes, v.by)).await;
    }

    fn spawn_workers(&self) {
        let on_new = {
            let this = self.clone();
            Arc::new(move |item: Item| this.triage_in_background(item)) as Arc<dyn Fn(Item) + Send + Sync>
        };

        if self.config.whatsapp.enabled {
            let (cfg, store, state, cancel) = (
                self.config.whatsapp.clone(),
                self.store.clone(),
                self.whatsapp.clone(),
                self.cancel.clone(),
            );
            let cb = on_new.clone();
            tokio::spawn(async move { whatsapp::run(cfg, store, state, cancel, move |i| cb(i)).await });
        }

        for acct in self.config.gmail.clone() {
            tokio::spawn(gmail::run(acct, self.store.clone(), self.cancel.clone(), on_new.clone()));
        }

        if self.config.triage.enabled && self.config.laya.spawn {
            let cfg = self.config.clone();
            let handle = self.laya.clone();
            let cancel = self.cancel.clone();
            tokio::spawn(async move {
                let py = cfg.laya.python_bin();
                if !py.exists() {
                    warn!("laya: {} not found; triage will fall back to 'read everything'", py.display());
                    return;
                }
                proc::supervise("laya", cancel, handle, move || {
                    let mut spec = proc::Spec::new(&py)
                        .args(["-u".into(), cfg.laya.script_path().into_os_string()])
                        .args(["--port".to_string(), cfg.triage.port.to_string()])
                        .env("AOS_LAYA_THRESHOLD", cfg.triage.threshold.to_string())
                        .env("PYTHONIOENCODING", "utf-8");
                    if let Some(hf) = cfg.laya.hf() {
                        spec = spec.env("HF_HOME", hf.into_os_string());
                    }
                    Some(spec)
                })
                .await;
            });
        }
    }

    /// Wait (up to `grace`) for the workers to wind down after [`Inbox::stop`].
    pub async fn wait_stopped(&self, grace: Duration) {
        let deadline = tokio::time::Instant::now() + grace;
        loop {
            let all = [self.laya.status().phase, self.whatsapp.service.status().phase];
            if all.iter().all(|p| *p != proc::Phase::Running) || tokio::time::Instant::now() >= deadline {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn inbox_with(port: u16, store: Arc<Store>) -> Inbox {
        let config = InboxConfig {
            triage: config::TriageConfig { port, timeout_ms: 400, ..Default::default() },
            ..Default::default()
        };
        Inbox {
            triage: triage::Triage::new(config.triage.clone()),
            config: Arc::new(config),
            store,
            whatsapp: whatsapp::WhatsappState::new(),
            laya: proc::ServiceHandle::new(),
            cancel: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn new_item_gets_a_triage_verdict_or_the_yes_fallback() {
        // A fake Laya that scores everything 0.05.
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 14\r\n\r\n{\"score\": 0.05}")
                .await;
        });
        let store = Arc::new(Store::open_temporary().unwrap());
        let inbox = inbox_with(port, store.clone());
        let mk = |ext: &str| NewItem {
            source: Source::Gmail,
            account: "main".into(),
            external_id: ext.into(),
            chat: "t".into(),
            chat_name: String::new(),
            sender: "shop@example.test".into(),
            sender_name: String::new(),
            subject: "Sale".into(),
            text: "50% off".into(),
            at: "2026-01-01T00:00:00.000Z".into(),
            from_me: false,
        };
        let a = store::insert_item(&store, "t", &mk("a")).unwrap().unwrap();
        inbox.triage_item(&a).await;
        let a = store::get_item(&store, &a.id).unwrap();
        assert_eq!((a.triage_yes, a.triage_by.as_deref()), (Some(false), Some("laya")));

        // The fake served one connection; the service is now "down", so the next one falls back.
        let b = store::insert_item(&store, "t", &mk("b")).unwrap().unwrap();
        inbox.triage_item(&b).await;
        let b = store::get_item(&store, &b.id).unwrap();
        assert_eq!((b.triage_yes, b.triage_by.as_deref()), (Some(true), Some("fallback")));
    }

    /// Live: start the real Laya service under the supervisor, score through the Rust client,
    /// then stop it. Needs the venv and weights. Run:
    /// `cargo test -p aos-inbox live_laya -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_laya_supervised() {
        let cfg = InboxConfig::default();
        let port = 8798; // not the default, so it cannot clash with a running service
        let cancel = CancellationToken::new();
        let handle = proc::ServiceHandle::new();
        let (python, laya) = (cfg.laya.python_bin(), cfg.laya.clone());
        let task = tokio::spawn(proc::supervise("laya", cancel.clone(), handle.clone(), move || {
            let mut spec = proc::Spec::new(&python)
                .args(["-u".into(), laya.script_path().into_os_string()])
                .args(["--port".to_string(), port.to_string()]);
            if let Some(hf) = laya.hf() {
                spec = spec.env("HF_HOME", hf.into_os_string());
            }
            Some(spec)
        }));
        let t = triage::Triage::new(config::TriageConfig { port, timeout_ms: 5000, ..Default::default() });
        let began = std::time::Instant::now();
        while !t.healthy().await {
            assert!(began.elapsed() < Duration::from_secs(600), "laya did not come up");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        println!("laya up after {:?}", began.elapsed());
        let otp = t.classify("Your OTP is 123456. Do not share it.", "VM-BANKXX", &[]).await;
        let real = t.classify("Beta kal shop pe aana, maal aaya hai", "Family", &[]).await;
        println!("otp {otp:?}
real {real:?}");
        assert!(!otp.yes && real.yes && real.by == "laya");
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(10), task).await.unwrap().unwrap();
        assert!(!t.healthy().await, "the Laya child must be gone after cancel");
    }
}
