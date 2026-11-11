//! WhatsApp through the Go `wacli` (openclaw/wacli, built on whatsmeow).
//!
//! - **Incoming**: `wacli sync --follow --webhook http://127.0.0.1:<port>/hook/<token>` runs as a
//!   supervised child. wacli POSTs each live message as JSON to a loopback listener in this
//!   process, which turns it into an `inbox_item`.
//! - **Outgoing**: only confirmed replies (`inbox_reply` status `queued`) are sent, one
//!   `wacli send text` at a time. While `sync --follow` runs it owns the store lock and delegates
//!   the send to itself, so one connection serves both.
//! - **Pairing**: `aos inbox whatsapp pair` runs `wacli auth` in the terminal (QR code), once.
//!   Until the store is paired, the service reports `needs_pairing` and starts nothing.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use aos_core::Store;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::config::WhatsappConfig;
use crate::proc::{self, ServiceHandle, Spec};
use crate::store::{self, NewItem, Source};

/// Where the WhatsApp link stands, for `inbox_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Unknown,
    /// Not paired: run `aos inbox whatsapp pair`.
    NeedsPairing,
    /// Paired; the sync child is running or starting.
    Paired,
}

/// Shared state of the WhatsApp worker.
#[derive(Clone)]
pub struct WhatsappState {
    pub service: ServiceHandle,
    link: Arc<AtomicU8>,
}

impl WhatsappState {
    pub fn new() -> Self {
        WhatsappState { service: ServiceHandle::new(), link: Arc::new(AtomicU8::new(0)) }
    }
    pub fn link(&self) -> Link {
        match self.link.load(Ordering::Relaxed) {
            1 => Link::NeedsPairing,
            2 => Link::Paired,
            _ => Link::Unknown,
        }
    }
    fn set(&self, l: Link) {
        self.link.store(l as u8 + 0, Ordering::Relaxed);
    }
}

impl Default for WhatsappState {
    fn default() -> Self {
        Self::new()
    }
}

// Link discriminants: Unknown = 0, NeedsPairing = 1, Paired = 2 (declaration order).

// Parsing ---------------------------------------------------------------------------------

/// Turn one wacli webhook payload into an inbox row. `None` for things that are not a message
/// worth showing: reactions, revoked messages, status broadcasts, empty payloads.
pub fn parse_payload(account: &str, v: &Value) -> Option<NewItem> {
    let s = |k: &str| -> String {
        v.get(k)
            .or_else(|| v.get(&k.to_ascii_lowercase()))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let b = |k: &str| {
        v.get(k)
            .or_else(|| v.get(&k.to_ascii_lowercase()))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let chat = s("Chat");
    let id = s("ID");
    if chat.is_empty() || id.is_empty() || chat.starts_with("status@") {
        return None;
    }
    if b("Revoked") || !s("ReactionToID").is_empty() {
        return None;
    }
    let mut text = s("Text");
    if let Some(m) = v.get("Media").filter(|m| m.is_object()) {
        let kind = m.get("Type").and_then(Value::as_str).unwrap_or("media");
        let caption = m.get("Caption").and_then(Value::as_str).unwrap_or("");
        text = if text.is_empty() && caption.is_empty() {
            format!("[{kind}]")
        } else if text.is_empty() {
            format!("[{kind}] {caption}")
        } else {
            format!("[{kind}] {text}")
        };
    }
    if text.trim().is_empty() {
        return None;
    }
    let at = normalize_ts(&s("Timestamp"));
    Some(NewItem {
        source: Source::Whatsapp,
        account: account.to_string(),
        external_id: id,
        chat_name: s("ChatName"),
        sender: {
            let sj = s("SenderJID");
            if sj.is_empty() { chat.clone() } else { sj }
        },
        sender_name: s("PushName"),
        chat,
        subject: String::new(),
        text,
        at,
        from_me: b("FromMe"),
    })
}

/// wacli sends RFC 3339 with nanoseconds; store the same millisecond UTC form as everything else.
fn normalize_ts(raw: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|d| aos_core::time::ts(d.with_timezone(&chrono::Utc)))
        .unwrap_or_else(|_| store::now_ts())
}

// Webhook listener ------------------------------------------------------------------------

/// Read one HTTP request; returns `(path, body)`.
async fn read_request(s: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = s.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > 64 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let path = head.lines().next()?.split_whitespace().nth(1)?.to_string();
    let len = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0)
        .min(4 * 1024 * 1024);
    while buf.len() < head_end + len {
        let n = s.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Some((path, buf[head_end..].to_vec()))
}

/// Accept webhook posts until cancelled. Only the exact `/hook/<token>` path is accepted.
pub async fn serve_hook(
    listener: TcpListener,
    token: String,
    cancel: CancellationToken,
    mut on_payload: impl FnMut(Value) + Send + 'static,
) {
    let want = format!("/hook/{token}");
    loop {
        let (mut s, _) = tokio::select! {
            _ = cancel.cancelled() => return,
            r = listener.accept() => match r { Ok(x) => x, Err(_) => continue },
        };
        let ok = match tokio::time::timeout(Duration::from_secs(5), read_request(&mut s)).await {
            Ok(Some((path, body))) if path == want => {
                if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                    on_payload(v);
                }
                true
            }
            _ => false,
        };
        let reply: &[u8] = if ok {
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        } else {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        };
        let _ = s.write_all(reply).await;
    }
}

// wacli commands --------------------------------------------------------------------------

fn base(cfg: &WhatsappConfig) -> Spec {
    Spec::new(cfg.bin()).args(["--store".into(), cfg.store().into_os_string()])
}

/// True when the store holds a linked device.
pub async fn is_paired(cfg: &WhatsappConfig) -> bool {
    let spec = base(cfg).args(["--read-only", "--json", "auth", "status"]);
    match proc::run_capture(&spec, Duration::from_secs(20)).await {
        Ok(out) if out.ok() => serde_json::from_str::<Value>(&out.stdout)
            .ok()
            .and_then(|v| v.pointer("/data/authenticated").and_then(Value::as_bool))
            .unwrap_or(false),
        _ => false,
    }
}

/// Send one text. Only called for replies the owner confirmed.
pub async fn send_text(cfg: &WhatsappConfig, to: &str, text: &str) -> Result<(), String> {
    let spec = base(cfg).args(["--json", "--lock-wait", "30s", "send", "text", "--to", to, "--message", text]);
    match proc::run_capture(&spec, Duration::from_secs(90)).await {
        Ok(out) if out.ok() => Ok(()),
        Ok(out) => Err(first_line(&out.stderr, &out.stdout)),
        Err(e) => Err(e.to_string()),
    }
}

fn first_line(a: &str, b: &str) -> String {
    let t = if a.trim().is_empty() { b } else { a };
    t.lines().find(|l| !l.trim().is_empty()).unwrap_or("send failed").chars().take(200).collect()
}

/// What `aos inbox whatsapp pair` prints, then the pairing command to run with a real terminal.
pub fn pair_instructions(cfg: &WhatsappConfig) -> String {
    format!(
        "Pair WhatsApp (once):\n\
         1. On the phone: WhatsApp > Settings > Linked devices > Link a device.\n\
         2. Scan the QR code that appears below. Keep the phone online until it says it is linked.\n\
         3. Let it finish (it syncs recent chats, then exits). Then start the server as usual.\n\
         Session store: {}\n",
        cfg.store().display()
    )
}

/// Run the interactive pairing (`wacli auth`) with the terminal attached. Blocks until it ends.
pub fn pair_blocking(cfg: &WhatsappConfig) -> std::io::Result<std::process::ExitStatus> {
    std::fs::create_dir_all(cfg.store())?;
    let mut cmd = std::process::Command::new(cfg.bin());
    cmd.arg("--store").arg(cfg.store()).arg("auth");
    cmd.status()
}

// The worker ------------------------------------------------------------------------------

/// Send every queued, confirmed reply with `send`. Returns how many were attempted.
pub async fn process_outbox<F, Fut>(store: &Store, mut send: F) -> usize
where
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let queued = match store::queued_replies(store, Source::Whatsapp) {
        Ok(q) => q,
        Err(e) => {
            warn!("outbox: {e}");
            return 0;
        }
    };
    let n = queued.len();
    for (reply, item) in queued {
        let result = send(item.chat.clone(), reply.text.clone()).await;
        let _ = store::finish_reply(store, &reply.id, result.as_deref_err());
    }
    n
}

trait AsDerefErr {
    fn as_deref_err(&self) -> Option<&str>;
}
impl AsDerefErr for Result<(), String> {
    fn as_deref_err(&self) -> Option<&str> {
        self.as_ref().err().map(String::as_str)
    }
}

/// Run the WhatsApp worker until `cancel` fires: pairing check, supervised `wacli sync`, the
/// webhook listener and the outbox. `on_new` is called with every newly stored item.
pub async fn run(
    cfg: WhatsappConfig,
    store: Arc<Store>,
    state: WhatsappState,
    cancel: CancellationToken,
    on_new: impl Fn(store::Item) + Send + Sync + 'static,
) {
    let on_new = Arc::new(on_new);
    // Wait until the store is paired. `wacli sync` never shows a QR, so there is nothing to start.
    loop {
        if is_paired(&cfg).await {
            state.set(Link::Paired);
            break;
        }
        state.set(Link::NeedsPairing);
        info!("whatsapp: not paired; run `aos inbox whatsapp pair`");
        if proc::sleep_or_cancel(&cancel, Duration::from_secs(30)).await {
            return;
        }
    }

    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            warn!("whatsapp: cannot open the webhook listener: {e}");
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let token = aos_core::ids::new_token();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(serve_hook(listener, token.clone(), cancel.clone(), move |v| {
        let _ = tx.send(v);
    }));

    // Ingest payloads one by one (the store write is quick; triage runs elsewhere).
    {
        let (store, account, on_new, cancel) = (store.clone(), cfg.account.clone(), on_new.clone(), cancel.clone());
        tokio::spawn(async move {
            loop {
                let v = tokio::select! {
                    _ = cancel.cancelled() => return,
                    v = rx.recv() => match v { Some(v) => v, None => return },
                };
                let Some(item) = parse_payload(&account, &v) else { continue };
                let store = store.clone();
                match tokio::task::spawn_blocking(move || store::insert_item(&store, "whatsapp", &item)).await {
                    Ok(Ok(Some(row))) => on_new(row),
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => warn!("whatsapp: could not store a message: {e}"),
                    Err(e) => warn!("whatsapp: {e}"),
                }
            }
        });
    }

    // Outbox: confirmed replies only.
    {
        let (store, cfg, cancel) = (store.clone(), cfg.clone(), cancel.clone());
        tokio::spawn(async move {
            let every = Duration::from_secs(cfg.outbox_secs.max(1));
            while !proc::sleep_or_cancel(&cancel, every).await {
                let s = store.clone();
                let c = cfg.clone();
                let _ = process_outbox(&s, |to, text| {
                    let c = c.clone();
                    async move { send_text(&c, &to, &text).await }
                })
                .await;
            }
        });
    }

    let url = format!("http://127.0.0.1:{port}/hook/{token}");
    let make_cfg = cfg.clone();
    proc::supervise("wacli-sync", cancel, state.service.clone(), move || {
        Some(base(&make_cfg).args([
            "--events",
            "sync",
            "--follow",
            "--webhook",
            url.as_str(),
            "--webhook-allow-private",
        ]))
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "Chat": "910000000001@s.whatsapp.net",
            "ID": "ABC123",
            "SenderJID": "910000000001@s.whatsapp.net",
            "Timestamp": "2026-01-02T03:04:05.123456789Z",
            "FromMe": false,
            "Text": "kal 5 baje aana",
            "PushName": "Sam",
            "ChatName": "Sam"
        })
    }

    #[test]
    fn parses_a_text_message() {
        let it = parse_payload("main", &sample()).unwrap();
        assert_eq!(it.external_id, "ABC123");
        assert_eq!(it.text, "kal 5 baje aana");
        assert_eq!(it.at, "2026-01-02T03:04:05.123Z");
        assert_eq!(it.sender_name, "Sam");
        assert!(!it.from_me);
    }

    #[test]
    fn media_and_noise() {
        let mut m = sample();
        m["Text"] = json!("");
        m["Media"] = json!({ "Type": "image", "Caption": "the bill" });
        assert_eq!(parse_payload("main", &m).unwrap().text, "[image] the bill");
        m["Media"] = json!({ "Type": "audio" });
        assert_eq!(parse_payload("main", &m).unwrap().text, "[audio]");

        let mut r = sample();
        r["ReactionToID"] = json!("X1");
        assert!(parse_payload("main", &r).is_none(), "reactions are skipped");
        let mut st = sample();
        st["Chat"] = json!("status@broadcast");
        assert!(parse_payload("main", &st).is_none());
        let mut e = sample();
        e["Text"] = json!("   ");
        assert!(parse_payload("main", &e).is_none());
    }

    #[test]
    fn own_messages_come_in_handled() {
        let mut m = sample();
        m["FromMe"] = json!(true);
        let s = Store::open_temporary().unwrap();
        let it = parse_payload("main", &m).unwrap();
        assert!(store::insert_item(&s, "t", &it).unwrap().unwrap().handled);
    }

    #[tokio::test]
    async fn hook_listener_checks_the_token() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let cancel = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(serve_hook(l, "tok".into(), cancel.clone(), move |v| {
            let _ = tx.send(v);
        }));
        let post = |path: &'static str| async move {
            let body = sample().to_string();
            let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let req = format!(
                "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            s.write_all(req.as_bytes()).await.unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        };
        assert!(post("/hook/wrong").await.starts_with("HTTP/1.1 404"));
        assert!(rx.try_recv().is_err());
        assert!(post("/hook/tok").await.starts_with("HTTP/1.1 200"));
        assert_eq!(rx.recv().await.unwrap()["ID"], "ABC123");
        cancel.cancel();
    }

    #[tokio::test]
    async fn outbox_sends_only_queued_replies() {
        let s = Store::open_temporary().unwrap();
        let it = store::insert_item(&s, "t", &parse_payload("main", &sample()).unwrap()).unwrap().unwrap();
        let (_, draft) = store::save_draft(&s, "t", &it.id, "ok, aa raha hu").unwrap();
        let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
        let rec = sent.clone();
        let send = move |to: String, text: String| {
            let rec = rec.clone();
            async move {
                rec.lock().unwrap().push((to, text));
                Ok::<(), String>(())
            }
        };
        assert_eq!(process_outbox(&s, send.clone()).await, 0, "a draft is never sent");
        store::queue_reply(&s, "t", &draft.id).unwrap();
        assert_eq!(process_outbox(&s, send).await, 1);
        assert_eq!(sent.lock().unwrap()[0].0, "910000000001@s.whatsapp.net");
        assert!(store::get_item(&s, &it.id).unwrap().handled);
    }
}
