//! Phone notifications for Agentic OS.
//!
//! - [`apns`]: the APNs sender (token auth with the `.p8`, HTTP/2).
//! - [`store`]: push tokens per paired device, alarms each phone rings itself, the sent-once log.
//! - [`payload`]: what each push says (alarm, reminder, Nora calling, approval, important inbox item).
//! - [`http`]: `/api/push/*` for the app (register, local alarms, status, test).
//!
//! [`Push::watch`] turns live events into pushes: Nora's `alarm_fired` and `call_requested`
//! (the 21:00 close-the-day ring included), Work's `approval_requested`, and inbox items the
//! triage model marked important. Nothing else pushes (no start/stop noise), and each thing pushes
//! once. An alarm the phone already scheduled on itself is not pushed to that phone again.
//!
//! Android: tokens with `platform = "android"` are stored and skipped until an FCM sender exists
//! (`Sender` seam in [`Push::deliver`]).

pub mod apns;
pub mod http;
pub mod payload;
pub mod store;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::apns::{Apns, ApnsConfig, Outcome};
use crate::payload::{Kind, Notification};
use crate::store::{PushStore, TokenRow};

#[derive(Clone)]
pub struct Push(Arc<Inner>);

struct Inner {
    store: PushStore,
    apns: Option<Apns>,
    /// Why push is off, for the status route.
    off_reason: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Report {
    pub sent: usize,
    /// Devices skipped: the phone rings this alarm itself, or no sender for the platform yet.
    pub skipped: usize,
    pub failed: usize,
    /// Dead tokens removed.
    pub gone: usize,
    /// Already pushed once (dedupe).
    pub duplicate: bool,
}

impl Push {
    /// Opens the token tables and loads the APNs config (`~/.aos/secrets/apns.json`). A missing or
    /// broken config leaves push off; tokens are still stored.
    pub fn start(db_path: &Path) -> Result<Push> {
        let (apns, off) = match ApnsConfig::load() {
            Ok(Some(cfg)) => match Apns::open(cfg) {
                Ok(a) => (Some(a), None),
                Err(e) => (None, Some(format!("APNs key unusable: {e:#}"))),
            },
            Ok(None) => (None, Some("no APNs config (~/.aos/secrets/apns.json)".into())),
            Err(e) => (None, Some(format!("APNs config unreadable: {e:#}"))),
        };
        match &off {
            None => info!("push: APNs ready"),
            Some(why) => warn!("push off: {why}"),
        }
        Ok(Push(Arc::new(Inner { store: PushStore::open(db_path)?, apns, off_reason: off })))
    }

    /// For tests: tokens only, no sender.
    pub fn without_sender(db_path: &Path) -> Result<Push> {
        Ok(Push(Arc::new(Inner { store: PushStore::open(db_path)?, apns: None, off_reason: Some("test".into()) })))
    }

    pub fn store(&self) -> &PushStore {
        &self.0.store
    }

    pub fn enabled(&self) -> bool {
        self.0.apns.is_some()
    }

    pub fn off_reason(&self) -> Option<&str> {
        self.0.off_reason.as_deref()
    }

    /// Push `n` once to every paired device (or only `device`). The dedupe key is claimed first,
    /// so a restart or a repeated event never rings twice. Test pushes skip the dedupe.
    pub async fn notify(&self, n: &Notification, device: Option<&str>) -> Report {
        if n.kind != Kind::Test {
            match self.run_blocking({
                let key = n.key();
                move |s| s.claim(&key)
            })
            .await
            {
                Ok(true) => {}
                Ok(false) => return Report { duplicate: true, ..Default::default() },
                Err(e) => warn!("push dedupe: {e:#}"),
            }
        }
        let dev = device.map(str::to_string);
        let rows = match self.run_blocking(move |s| s.active(dev.as_deref())).await {
            Ok(r) => r,
            Err(e) => {
                warn!("push tokens: {e:#}");
                return Report::default();
            }
        };
        self.deliver(n, rows).await
    }

    async fn deliver(&self, n: &Notification, rows: Vec<TokenRow>) -> Report {
        let mut report = Report::default();
        let now = apns::unix_now();
        for row in rows {
            // The phone already rings this alarm on its own (works with no signal): don't double it.
            if matches!(n.kind, Kind::Alarm | Kind::Reminder) {
                let (d, id) = (row.device_id.clone(), n.id.clone());
                if self.run_blocking(move |s| s.has_local(&d, &id)).await.unwrap_or(false) {
                    report.skipped += 1;
                    continue;
                }
            }
            let outcome = match (row.platform.as_str(), &self.0.apns) {
                ("ios", Some(apns)) => apns.send(&row.token, &row.env, &n.apns(now)).await,
                ("ios", None) => Outcome::Failed("push off".into()),
                // Android seam: an FCM HTTP v1 sender goes here.
                _ => {
                    report.skipped += 1;
                    continue;
                }
            };
            let token = row.token.clone();
            match outcome {
                Outcome::Sent => {
                    report.sent += 1;
                    let _ = self.run_blocking(move |s| s.mark(&token, None)).await;
                }
                Outcome::Gone(why) => {
                    report.gone += 1;
                    info!("push: token …{} is gone ({why}); removed", row.tail);
                    let _ = self.run_blocking(move |s| s.forget(&token)).await;
                }
                Outcome::Failed(why) => {
                    report.failed += 1;
                    warn!("push {} to …{} failed: {why}", n.kind.as_str(), row.tail);
                    let _ = self.run_blocking(move |s| s.mark(&token, Some(&why))).await;
                }
            }
        }
        report
    }

    async fn run_blocking<T: Send + 'static>(&self, f: impl FnOnce(&PushStore) -> Result<T> + Send + 'static) -> Result<T> {
        let me = self.clone();
        tokio::task::spawn_blocking(move || f(&me.0.store)).await?
    }

    /// Listen to Nora, Work and the inbox until the channels close. Spawns one task per source.
    pub fn watch(
        &self,
        nora: broadcast::Receiver<aos_nora::NoraEvent>,
        work: broadcast::Receiver<aos_work::types::WorkEvent>,
        inbox: Option<broadcast::Receiver<aos_inbox::Item>>,
    ) {
        spawn_listener(self.clone(), nora, |e| from_nora(&e));
        spawn_listener(self.clone(), work, |e| from_work(&e));
        if let Some(rx) = inbox {
            let me = self.clone();
            spawn_listener(self.clone(), rx, move |item| {
                // One push per sender per 10 minutes: a burst of messages is one buzz.
                let bucket = apns::unix_now() / 600;
                let sender = if item.sender.is_empty() { item.chat.clone() } else { item.sender.clone() };
                match me.store().claim(&format!("inbox-from:{sender}:{bucket}")) {
                    Ok(true) => from_inbox(&item),
                    _ => None,
                }
            });
        }
        let me = self.clone();
        tokio::spawn(async move {
            let _ = me.run_blocking(|s| s.prune(30)).await;
        });
    }
}

fn spawn_listener<E: Clone + Send + 'static>(
    push: Push,
    mut rx: broadcast::Receiver<E>,
    map: impl Fn(E) -> Option<Notification> + Send + 'static,
) {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(e) => {
                    if let Some(n) = map(e) {
                        let r = push.notify(&n, None).await;
                        if !r.duplicate {
                            info!("push {} {}: sent {} skipped {} failed {}", n.kind.as_str(), n.id, r.sent, r.skipped, r.failed);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => warn!("push listener lagged by {n} events"),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Nora events that reach the phone: an alarm or reminder firing, and a ring (any `call_requested`,
/// including the 21:00 close-the-day call). Missed calls, call states and messages don't push.
pub fn from_nora(e: &aos_nora::NoraEvent) -> Option<Notification> {
    use aos_nora::NoraEvent;
    match e {
        NoraEvent::AlarmFired { alarm, late_secs } => Some(Notification::alarm(
            &alarm.id,
            &alarm.label,
            alarm.kind == aos_core::model::AlarmKind::Reminder,
            *late_secs,
        )),
        NoraEvent::CallRequested { call } => {
            Some(Notification::call(call.id, &call.reason, aos_nora::scheduler::RING_SECS as u64))
        }
        _ => None,
    }
}

/// Work events that reach the phone: a session waiting for his approval. Turn starts, ends and
/// diffs don't.
pub fn from_work(e: &aos_work::types::WorkEvent) -> Option<Notification> {
    match e {
        aos_work::types::WorkEvent::ApprovalRequested { approval } => {
            Some(Notification::approval(&approval.id, &approval.thread_id, &approval.title))
        }
        _ => None,
    }
}

pub fn from_inbox(item: &aos_inbox::Item) -> Option<Notification> {
    if item.from_me || item.handled {
        return None;
    }
    let who = if !item.sender_name.is_empty() {
        &item.sender_name
    } else if !item.chat_name.is_empty() {
        &item.chat_name
    } else {
        &item.sender
    };
    Some(Notification::inbox(&item.id, &item.source, who, &item.subject))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_alarms_and_rings_push_from_nora() {
        use aos_nora::NoraEvent;
        let alarm = aos_core::model::Alarm {
            id: "al1".into(),
            at: "2026-11-12T21:00:00Z".into(),
            label: "Sleep".into(),
            kind: aos_core::model::AlarmKind::Alarm,
            status: aos_core::model::AlarmStatus::Fired,
            task_id: None,
        };
        let n = from_nora(&NoraEvent::AlarmFired { alarm, late_secs: 0 }).unwrap();
        assert_eq!((n.kind, n.title.as_str(), n.body.as_str()), (Kind::Alarm, "Alarm", "Sleep"));

        let call = aos_nora::store::Call { id: 7, at: 0, reason: "Close the day: your journaling call".into(), source: "close_day".into(), status: "ringing".into() };
        let n = from_nora(&NoraEvent::CallRequested { call: call.clone() }).unwrap();
        assert_eq!((n.kind, n.url.as_str()), (Kind::Call, "aos://call/ring/7"));
        assert!(from_nora(&NoraEvent::CallMissed { call }).is_none(), "ring once: a missed ring doesn't push again");
        assert!(from_nora(&NoraEvent::TurnStarted { turn_id: "t".into() }).is_none());
    }

    #[test]
    fn inbox_skips_own_and_handled() {
        let mut item = aos_inbox::Item {
            id: "i1".into(),
            source: "gmail".into(),
            account: String::new(),
            chat: String::new(),
            chat_name: String::new(),
            sender: "a@b.c".into(),
            sender_name: "Bank".into(),
            subject: "Statement".into(),
            text: "secret body".into(),
            at: String::new(),
            from_me: false,
            triage_score: Some(0.9),
            triage_yes: Some(true),
            triage_by: Some("laya".into()),
            handled: false,
            note: None,
        };
        let n = from_inbox(&item).unwrap();
        assert!(!n.body.contains("secret"));
        item.handled = true;
        assert!(from_inbox(&item).is_none());
    }

    #[tokio::test]
    async fn notify_dedupes_and_skips_android_and_local_alarms() {
        let path = store::temp_db("notify");
        let push = Push::without_sender(&path).unwrap();
        let tok = "f".repeat(64);
        push.store().register("dev-a", &tok, "android", "production").unwrap();
        let n = Notification::call(1, "hi", 90);
        let r = push.notify(&n, None).await;
        assert_eq!((r.sent, r.skipped, r.duplicate), (0, 1, false));
        assert!(push.notify(&n, None).await.duplicate);

        push.store().register("dev-b", &"e".repeat(64), "ios", "production").unwrap();
        push.store().set_local("dev-b", &["al9".into()]).unwrap();
        let r = push.notify(&Notification::alarm("al9", "Gym", false, 0), None).await;
        assert_eq!((r.sent, r.skipped, r.failed), (0, 2, 0), "android skipped, iPhone rings it locally");
        // Without the local copy the iPhone would get it; with no sender here it counts as failed.
        let r = push.notify(&Notification::alarm("al10", "Gym", false, 0), None).await;
        assert_eq!((r.skipped, r.failed), (1, 1));
        let _ = std::fs::remove_file(&path);
    }
}
