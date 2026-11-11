//! Phone notifications on the client: permission, the push token, notification taps, and alarms
//! the phone rings by itself.
//!
//! - iOS: a Swift plugin (`src/ios/push`, loaded through `#[manganis::ffi]`, see [`ios`]).
//! - Android: not built yet. The seam is [`Native`]: an FCM version returns the same JSON shape
//!   (token, status, taps, local) from a Kotlin plugin, and the server already stores
//!   `platform = "android"` tokens.
//! - Desktop and web: nothing (no-op), so the hook below costs nothing there.
//!
//! [`use_native_push`] runs in the app shell: once paired it asks for permission, sends the token
//! to `/api/push/register`, keeps the phone's local alarms in step with `alarm_list`, tells the
//! server which alarms the phone rings itself (`/api/push/local`), and routes taps.

#[cfg(target_os = "ios")]
mod ios;

use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::Route;
use crate::live::{Live, net};
use crate::state::AppState;

/// What the native side reports on each poll.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub status: String,
    pub token: Option<String>,
    pub token_error: Option<String>,
    #[serde(default)]
    pub alarm_kit: bool,
    #[serde(default)]
    pub taps: Vec<Tap>,
    /// Set once after each alarm sync: the alarm ids now scheduled on the phone.
    pub local: Option<Vec<String>>,
    pub local_error: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct Tap {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub url: String,
}

/// The platform plugin behind one interface. Each platform module answers `platform()` and
/// `call(what, json)` with `what` = `start` / `poll` / `sync`. Android adds a `cfg(target_os = "android")`
/// module here.
#[cfg(target_os = "ios")]
mod imp {
    pub fn platform() -> Option<&'static str> {
        super::ios::available().then_some("ios")
    }
    pub fn call(what: &str, arg: &str) -> Option<String> {
        super::ios::call(what, arg)
    }
}

#[cfg(not(target_os = "ios"))]
mod imp {
    pub fn platform() -> Option<&'static str> {
        None
    }
    pub fn call(_what: &str, _arg: &str) -> Option<String> {
        None
    }
}

pub struct Native;

impl Native {
    /// `ios` where a plugin exists (and loaded), else `None`.
    pub fn platform() -> Option<&'static str> {
        imp::platform()
    }

    /// Installs the hooks. Call once from `main`, before the app launches, so a notification tap
    /// that launched the app is not lost.
    #[cfg_attr(feature = "server", allow(dead_code))]
    pub fn install() {
        imp::call("start", r#"{"ask":false}"#);
    }

    /// Shows the permission prompt once (later calls only re-read the answer).
    pub fn ask() {
        imp::call("start", r#"{"ask":true}"#);
    }

    pub fn poll() -> Option<Snapshot> {
        imp::call("poll", "").and_then(|s| serde_json::from_str(&s).ok())
    }

    /// The full list of upcoming alarms; the phone cancels the ones not in it.
    pub fn sync_alarms(list: &Value) {
        imp::call("sync", &list.to_string());
    }

    /// `production` for TestFlight / App Store builds, `sandbox` for debug installs from Xcode.
    pub fn apns_env() -> &'static str {
        if cfg!(debug_assertions) { "sandbox" } else { "production" }
    }
}

/// An in-app link from a push (`aos://...`) as a place to go.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Today,
    Nora,
    /// A ring; `answer` = he pressed Answer.
    Ring { id: i64, answer: bool },
    DeclineRing(i64),
    Work(String),
    Inbox(String),
}

pub fn target(tap: &Tap) -> Option<Target> {
    if tap.action == "dismiss" || tap.action == "stop" || tap.action == "snooze" {
        return None;
    }
    let rest = tap.url.strip_prefix("aos://")?;
    let mut parts = rest.splitn(3, '/');
    Some(match (parts.next()?, parts.next(), parts.next()) {
        ("call", Some("ring"), Some(id)) => {
            let id = id.parse().ok()?;
            match tap.action.as_str() {
                "decline" => Target::DeclineRing(id),
                a => Target::Ring { id, answer: a == "answer" },
            }
        }
        ("nora", _, _) => Target::Nora,
        ("work", thread, _) => Target::Work(thread.unwrap_or("").to_string()),
        ("inbox", item, _) => Target::Inbox(item.unwrap_or("").to_string()),
        _ => Target::Today,
    })
}

/// Upcoming alarms from `alarm_list` as the plugin's list.
pub fn alarm_rows(v: &Value) -> Value {
    let rows: Vec<Value> = v["alarms"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| x["status"].as_str().is_none_or(|s| s == "scheduled"))
                .filter_map(|x| {
                    let at = chrono::DateTime::parse_from_rfc3339(x["at"].as_str()?).ok()?;
                    let kind = if x["kind"].as_str() == Some("reminder") { "reminder" } else { "alarm" };
                    let label = x["label"].as_str().unwrap_or("");
                    let title = if label.is_empty() { if kind == "reminder" { "Reminder" } else { "Alarm" } } else { label };
                    Some(json!({ "id": x["id"], "at_ms": at.timestamp_millis(), "title": title, "kind": kind }))
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(rows)
}

/// Poll every 2 s while the app runs (the webview timer pauses when the app is in the background,
/// which is fine: taps and the token wait in the plugin). Alarms resync every minute and when the
/// pairing changes.
pub fn use_native_push(live: Live, state: AppState) {
    let nav = use_navigator();
    use_future(move || async move {
        if Native::platform().is_none() {
            return;
        }
        let mut asked = false;
        let mut sent: Option<(String, String)> = None;
        let mut last_sync: Option<(f64, String)> = None;
        loop {
            let conn = live.conn.peek().clone().filter(|c| !c.token.is_empty());
            if let Some(conn) = conn {
                if !asked {
                    Native::ask();
                    asked = true;
                }
                if let Some(snap) = Native::poll() {
                    for tap in &snap.taps {
                        route(tap, &conn, state, nav).await;
                    }
                    if let Some(tok) = snap.token.clone() {
                        let key = (tok.clone(), format!("{}|{}", conn.url, conn.device_id));
                        if sent.as_ref() != Some(&key) {
                            let body = json!({ "token": tok, "platform": "ios", "env": Native::apns_env() });
                            if net::post(&conn, "/api/push/register", &body).await.is_ok() {
                                sent = Some(key);
                            }
                        }
                    }
                    if let Some(ids) = snap.local {
                        let _ = net::post(&conn, "/api/push/local", &json!({ "alarm_ids": ids })).await;
                    }
                }
                let now = js_now();
                let fresh = last_sync.as_ref().is_some_and(|(t, url)| now - t < 60_000.0 && *url == conn.url);
                if !fresh {
                    if let Ok(v) = net::tool(&conn, "alarm_list", json!({})).await {
                        Native::sync_alarms(&alarm_rows(&v));
                        last_sync = Some((now, conn.url.clone()));
                    }
                }
            }
            net::sleep_ms(2000).await;
        }
    });
}

fn js_now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64
}

async fn route(tap: &Tap, conn: &net::Conn, mut state: AppState, nav: dioxus::router::Navigator) {
    let Some(t) = target(tap) else { return };
    match t {
        Target::Today => {
            nav.push(Route::Today { lock: None, mode: None });
        }
        Target::Nora => state.nora.set(true),
        Target::Ring { id, answer } => {
            if answer {
                crate::call::answer_ring(id);
            } else {
                // The ring card shows if the ring is still live; the Nora page is behind it.
                state.nora.set(true);
            }
        }
        Target::DeclineRing(id) => {
            let _ = net::post(conn, "/api/nora/call/decline", &json!({ "ring_id": id })).await;
        }
        Target::Work(_) => {
            nav.push(Route::Work {});
        }
        Target::Inbox(_) => {
            nav.push(Route::Inbox {});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(action: &str, url: &str) -> Tap {
        Tap { action: action.into(), url: url.into(), ..Default::default() }
    }

    #[test]
    fn routes_push_links() {
        assert_eq!(target(&tap("open", "aos://call/ring/12")), Some(Target::Ring { id: 12, answer: false }));
        assert_eq!(target(&tap("answer", "aos://call/ring/12")), Some(Target::Ring { id: 12, answer: true }));
        assert_eq!(target(&tap("decline", "aos://call/ring/12")), Some(Target::DeclineRing(12)));
        assert_eq!(target(&tap("open", "aos://work/th1")), Some(Target::Work("th1".into())));
        assert_eq!(target(&tap("open", "aos://inbox/i1")), Some(Target::Inbox("i1".into())));
        assert_eq!(target(&tap("open", "aos://today")), Some(Target::Today));
        assert_eq!(target(&tap("stop", "aos://today")), None);
        assert_eq!(target(&tap("open", "https://example.com")), None);
    }

    #[test]
    fn alarm_list_becomes_plugin_rows() {
        let v = json!({ "alarms": [
            { "id": "a1", "at": "2026-11-13T01:30:00Z", "label": "Wake", "kind": "alarm", "status": "scheduled" },
            { "id": "a2", "at": "2026-11-13T02:00:00+05:30", "label": "", "kind": "reminder", "status": "scheduled" },
            { "id": "a3", "at": "2026-11-13T02:00:00Z", "label": "Old", "kind": "alarm", "status": "cancelled" },
            { "id": "a4", "at": "not a time", "label": "x", "kind": "alarm" }
        ]});
        let rows = alarm_rows(&v);
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["title"], "Wake");
        assert_eq!(rows[0]["at_ms"], 1_794_533_400_000i64);
        assert_eq!((rows[1]["title"].as_str(), rows[1]["kind"].as_str()), (Some("Reminder"), Some("reminder")));
    }
}
