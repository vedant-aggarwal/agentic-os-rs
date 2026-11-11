//! Talking to the running server (for tools that only exist there: Work, Nora).
//!
//! `AOS_URL` (default `http://127.0.0.1:8787`) and `AOS_TOKEN` (a device token from
//! `aos pair token --name cli`). Plain HTTP/1.1 over a local socket, so the CLI needs no HTTP
//! client crate. Any failure returns `None` and the caller falls back to local tools.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::Value;

/// `GET /api/tools` from the running server.
pub fn tools() -> Option<Vec<Value>> {
    let token = std::env::var("AOS_TOKEN").ok().filter(|t| !t.trim().is_empty())?;
    let base = std::env::var("AOS_URL").unwrap_or_else(|_| "http://127.0.0.1:8787".into());
    let body = get(&base, "/api/tools", token.trim())?;
    let v: Value = serde_json::from_str(&body).ok()?;
    v["tools"].as_array().cloned()
}

fn get(base: &str, path: &str, token: &str) -> Option<String> {
    let hostport = base.strip_prefix("http://")?.trim_end_matches('/');
    let addr = hostport.to_socket_addrs().ok()?.next()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {hostport}\r\nAuthorization: Bearer {token}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") {
        return None;
    }
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        return Some(dechunk(body));
    }
    Some(body.to_string())
}

fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some((size, after)) = rest.split_once("\r\n") {
        let Ok(n) = usize::from_str_radix(size.trim(), 16) else { break };
        if n == 0 || after.len() < n {
            break;
        }
        out.push_str(&after[..n]);
        rest = after[n..].trim_start_matches("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn dechunks() {
        assert_eq!(super::dechunk("4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"), "Wikipedia");
    }
}
