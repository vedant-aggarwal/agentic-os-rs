//! Triage client: asks the local Laya service "should Nora read this?".
//!
//! One small HTTP/1.1 POST over a plain TCP connection to `127.0.0.1` (never `localhost`:
//! on Windows the IPv6 attempt costs about 2 s per call). Any failure or timeout falls back to
//! "yes, read it", so an important message is never silently dropped.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::config::TriageConfig;

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// Probability that Nora should read the message. `None` for the fallback.
    pub score: Option<f64>,
    pub yes: bool,
    /// `laya` or `fallback`.
    pub by: &'static str,
}

impl Verdict {
    pub fn fallback() -> Verdict {
        Verdict { score: None, yes: true, by: "fallback" }
    }
}

#[derive(Debug, Clone)]
pub struct Triage {
    cfg: TriageConfig,
}

impl Triage {
    pub fn new(cfg: TriageConfig) -> Self {
        Triage { cfg }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    pub fn addr(&self) -> String {
        format!("{}:{}", self.cfg.host, self.cfg.port)
    }

    /// Score one message. Never fails: the fallback is "yes".
    pub async fn classify(&self, message: &str, sender: &str, recent_chat: &[String]) -> Verdict {
        if !self.cfg.enabled {
            return Verdict::fallback();
        }
        let body = json!({ "message": message, "sender": sender, "recent_chat": recent_chat });
        let timeout = Duration::from_millis(self.cfg.timeout_ms);
        match tokio::time::timeout(timeout, post_json(&self.addr(), "/triage", &body)).await {
            Ok(Ok(v)) => match v.get("score").and_then(Value::as_f64) {
                Some(score) => Verdict {
                    score: Some(score),
                    yes: score >= self.cfg.threshold,
                    by: "laya",
                },
                None => Verdict::fallback(),
            },
            _ => Verdict::fallback(),
        }
    }

    /// True when the service answers `GET /health`.
    pub async fn healthy(&self) -> bool {
        let t = Duration::from_millis(self.cfg.timeout_ms.max(500));
        matches!(
            tokio::time::timeout(t, request(&self.addr(), "GET", "/health", None)).await,
            Ok(Ok(_))
        )
    }
}

async fn post_json(addr: &str, path: &str, body: &Value) -> std::io::Result<Value> {
    let text = request(addr, "POST", path, Some(body.to_string())).await?;
    serde_json::from_str(&text).map_err(std::io::Error::other)
}

/// Returns the response body for a 2xx answer.
async fn request(addr: &str, method: &str, path: &str, body: Option<String>) -> std::io::Result<String> {
    let mut s = TcpStream::connect(addr).await?;
    let body = body.unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await?;
    let raw = String::from_utf8_lossy(&raw);
    let (head, rest) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| std::io::Error::other("bad response"))?;
    let status_ok = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .is_some_and(|c| c.starts_with('2'));
    if !status_ok {
        return Err(std::io::Error::other(format!("http error: {}", head.lines().next().unwrap_or(""))));
    }
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        return Ok(dechunk(rest));
    }
    Ok(rest.to_string())
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((len, tail)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(len.trim(), 16).unwrap_or(0);
        if n == 0 || tail.len() < n {
            break;
        }
        out.push_str(&tail[..n]);
        rest = tail[n..].trim_start_matches("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// A one-shot fake service answering with `reply`.
    async fn fake(reply: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut s, _)) = l.accept().await {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf).await;
                let _ = s.write_all(reply.as_bytes()).await;
            }
        });
        port
    }

    fn cfg(port: u16) -> TriageConfig {
        TriageConfig { port, timeout_ms: 300, ..Default::default() }
    }

    #[tokio::test]
    async fn scores_above_threshold_are_yes() {
        let port = fake("HTTP/1.1 200 OK\r\nContent-Length: 14\r\n\r\n{\"score\": 0.9}").await;
        let v = Triage::new(cfg(port)).classify("hello", "sam", &[]).await;
        assert_eq!(v, Verdict { score: Some(0.9), yes: true, by: "laya" });
    }

    #[tokio::test]
    async fn low_score_is_no() {
        let port = fake("HTTP/1.1 200 OK\r\nContent-Length: 14\r\n\r\n{\"score\": 0.1}").await;
        let v = Triage::new(cfg(port)).classify("sale!", "shop", &[]).await;
        assert!(!v.yes && v.by == "laya");
    }

    #[tokio::test]
    async fn service_down_falls_back_to_yes() {
        // Nothing listens on this port.
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let v = Triage::new(cfg(port)).classify("x", "y", &[]).await;
        assert_eq!(v, Verdict::fallback());
    }

    #[tokio::test]
    async fn hung_service_times_out_to_yes() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _keep = l.accept().await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let v = Triage::new(cfg(port)).classify("x", "y", &[]).await;
        assert_eq!(v, Verdict::fallback());
    }
}
