//! Gmail through the existing `gws` CLI. **Read only**: it lists and reads messages, nothing is
//! ever modified, labelled or sent from here.
//!
//! Each poll lists recent inbox messages (`users.messages.list`), skips the ones already stored,
//! and reads the rest with `format=metadata` (headers and snippet, no body download). More
//! accounts are more `[[gmail]]` blocks in the inbox config, each with its own
//! `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` (one `gws auth login` per account).

use std::sync::Arc;
use std::time::Duration;

use aos_core::Store;
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::config::{GmailAccount, find_gws};
use crate::proc::{self, Spec};
use crate::store::{self, NewItem, Source};

/// New messages read per poll, so a first run on a busy mailbox stays gentle.
const MAX_NEW_PER_POLL: usize = 20;

fn gws(acct: &GmailAccount, args: [&str; 6]) -> Spec {
    let mut spec = Spec::new(find_gws()).args(args);
    if let Some(dir) = &acct.config_dir {
        spec = spec.env("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", dir.as_os_str());
    }
    spec
}

async fn call(acct: &GmailAccount, resource: &str, method: &str, params: Value) -> Result<Value, String> {
    let p = params.to_string();
    let spec = gws(acct, ["gmail", "users", resource, method, "--params", p.as_str()]);
    let out = proc::run_capture(&spec, Duration::from_secs(45))
        .await
        .map_err(|e| format!("gws: {e}"))?;
    let v: Value = serde_json::from_str(&out.stdout).map_err(|_| {
        format!("gws gave no JSON (exit {:?}): {}", out.code, out.stderr.lines().next().unwrap_or(""))
    })?;
    if let Some(e) = v.get("error") {
        return Err(format!(
            "gmail error {}: {}",
            e.get("code").and_then(Value::as_i64).unwrap_or(0),
            e.get("message").and_then(Value::as_str).unwrap_or("")
        ));
    }
    Ok(v)
}

/// Message ids from a `messages.list` answer, newest first.
pub fn parse_list(v: &Value) -> Vec<String> {
    v.get("messages")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Turn a `messages.get?format=metadata` answer into an inbox row.
pub fn parse_message(account: &str, v: &Value) -> Option<NewItem> {
    let id = v.get("id")?.as_str()?.to_string();
    let header = |name: &str| -> String {
        v.pointer("/payload/headers")
            .and_then(Value::as_array)
            .and_then(|hs| {
                hs.iter().find(|h| {
                    h.get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
                })
            })
            .and_then(|h| h.get("value").and_then(Value::as_str))
            .unwrap_or("")
            .to_string()
    };
    let labels: Vec<&str> = v
        .get("labelIds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let ms: i64 = v
        .get("internalDate")
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let at = Utc
        .timestamp_millis_opt(ms)
        .single()
        .map(aos_core::time::ts)
        .unwrap_or_else(store::now_ts);
    let (name, addr) = split_from(&header("From"));
    Some(NewItem {
        source: Source::Gmail,
        account: account.to_string(),
        external_id: id,
        chat: v.get("threadId").and_then(Value::as_str).unwrap_or("").to_string(),
        chat_name: header("Subject"),
        sender: addr,
        sender_name: name,
        subject: header("Subject"),
        text: v.get("snippet").and_then(Value::as_str).unwrap_or("").to_string(),
        at,
        from_me: labels.contains(&"SENT"),
    })
}

/// `"Sam Lee" <sam@example.test>` into `("Sam Lee", "sam@example.test")`.
pub fn split_from(raw: &str) -> (String, String) {
    let raw = raw.trim();
    match (raw.rfind('<'), raw.rfind('>')) {
        (Some(a), Some(b)) if a < b => (
            raw[..a].trim().trim_matches('"').to_string(),
            raw[a + 1..b].trim().to_string(),
        ),
        _ => (String::new(), raw.to_string()),
    }
}

fn exists(store: &Store, account: &str, id: &str) -> bool {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT 1 FROM inbox_item WHERE source = 'gmail' AND account = ?1 AND external_id = ?2",
                [account, id],
                |_| Ok(()),
            )
            .is_ok())
        })
        .unwrap_or(false)
}

/// One poll. Returns the newly stored items.
pub async fn poll_once(
    acct: &GmailAccount,
    store: &Arc<Store>,
) -> Result<Vec<store::Item>, String> {
    store::ensure_schema(store).map_err(|e| e.to_string())?;
    let list = call(
        acct,
        "messages",
        "list",
        json!({ "userId": "me", "maxResults": acct.max, "q": acct.query }),
    )
    .await?;
    let mut stored = Vec::new();
    let mut fresh = 0;
    for id in parse_list(&list) {
        if exists(store, &acct.name, &id) {
            continue;
        }
        if fresh == MAX_NEW_PER_POLL {
            break;
        }
        fresh += 1;
        let msg = call(
            acct,
            "messages",
            "get",
            json!({ "userId": "me", "id": id, "format": "metadata" }),
        )
        .await?;
        if let Some(item) = parse_message(&acct.name, &msg) {
            let s = store.clone();
            let row = tokio::task::spawn_blocking(move || store::insert_item(&s, "gmail", &item))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            stored.extend(row);
        }
    }
    Ok(stored)
}

/// Poll one account until cancelled. Errors back off up to 10 minutes; `on_new` sees each new item.
pub async fn run(
    acct: GmailAccount,
    store: Arc<Store>,
    cancel: CancellationToken,
    on_new: Arc<dyn Fn(store::Item) + Send + Sync>,
) {
    let every = Duration::from_secs(acct.interval_secs.max(15));
    let mut wait = Duration::from_secs(2);
    loop {
        if proc::sleep_or_cancel(&cancel, wait).await {
            return;
        }
        wait = match poll_once(&acct, &store).await {
            Ok(items) => {
                debug!(account = %acct.name, new = items.len(), "gmail poll");
                items.into_iter().for_each(|i| on_new(i));
                every
            }
            Err(e) => {
                warn!(account = %acct.name, "gmail poll failed: {e}");
                (wait * 2).clamp(Duration::from_secs(30), Duration::from_secs(600))
            }
        };
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "id": "18f0a1b2c3d4e5f6",
            "threadId": "18f0a1b2c3d4e5f0",
            "labelIds": ["INBOX", "UNREAD"],
            "snippet": "Hi, the invoice for last month is still unpaid.",
            "internalDate": "1767323045000",
            "payload": { "headers": [
                { "name": "From", "value": "\"Sam Lee\" <sam@example.test>" },
                { "name": "subject", "value": "Invoice reminder" },
                { "name": "Date", "value": "Fri, 2 Jan 2026 03:04:05 +0000" }
            ]}
        })
    }

    #[test]
    fn parses_a_message() {
        let it = parse_message("main", &sample()).unwrap();
        assert_eq!(it.sender, "sam@example.test");
        assert_eq!(it.sender_name, "Sam Lee");
        assert_eq!(it.subject, "Invoice reminder");
        assert_eq!(it.chat, "18f0a1b2c3d4e5f0");
        assert_eq!(it.at, "2026-01-02T03:04:05.000Z");
        assert!(!it.from_me);
    }

    #[test]
    fn sent_label_means_from_me() {
        let mut m = sample();
        m["labelIds"] = json!(["SENT", "INBOX"]);
        assert!(parse_message("main", &m).unwrap().from_me);
    }

    #[test]
    fn list_and_from_parsing() {
        assert_eq!(parse_list(&json!({ "messages": [{ "id": "a" }, { "id": "b" }] })), vec!["a", "b"]);
        assert!(parse_list(&json!({ "resultSizeEstimate": 0 })).is_empty());
        assert_eq!(split_from("plain@example.test"), (String::new(), "plain@example.test".into()));
    }

    /// Live check against the real `gws` login. Read only; prints counts, never message text.
    /// Run: `cargo test -p aos-inbox live_poll -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_poll() {
        let store = Arc::new(Store::open_temporary().unwrap());
        let acct = GmailAccount { max: 5, ..Default::default() };
        let first = poll_once(&acct, &store).await.expect("poll");
        let again = poll_once(&acct, &store).await.expect("second poll");
        println!("gmail live: stored {} on first poll, {} new on second", first.len(), again.len());
        assert!(again.is_empty(), "second poll must not duplicate");
    }
}
