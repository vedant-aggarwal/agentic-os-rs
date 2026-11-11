//! What each push says, and the APNs JSON for it.
//!
//! The rules (SYSTEM.md, NORA-BUILD.md "ring once"): only things he has to act on reach the phone.
//! No push for a session starting or stopping, a turn finishing, a call ending or a sync. Each
//! event pushes once (deduped by key in `push_sent`). Alarms and Nora's calls are time-sensitive
//! (they break through Focus); the rest are normal. Inbox pushes carry the sender, never the text.
//!
//! Every push has an `aos` object for the app: `{id, kind, url}`. `url` is an in-app link the app
//! routes on tap: `aos://today`, `aos://nora`, `aos://call/ring/<id>`, `aos://work/<thread>`,
//! `aos://inbox/<item>`.

use serde_json::{Value, json};

use crate::apns::ApnsRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Alarm,
    Reminder,
    Call,
    Approval,
    Inbox,
    Test,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Alarm => "alarm",
            Kind::Reminder => "reminder",
            Kind::Call => "call",
            Kind::Approval => "approval",
            Kind::Inbox => "inbox",
            Kind::Test => "test",
        }
    }

    /// The notification category the app registers (buttons).
    pub fn category(self) -> &'static str {
        match self {
            Kind::Alarm => "AOS_ALARM",
            Kind::Reminder => "AOS_REMINDER",
            Kind::Call => "AOS_CALL",
            Kind::Approval => "AOS_APPROVAL",
            Kind::Inbox => "AOS_INBOX",
            Kind::Test => "AOS_PLAIN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Active,
    TimeSensitive,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub kind: Kind,
    /// Stable id of the thing (alarm id, ring id, approval id, inbox item id).
    pub id: String,
    pub title: String,
    pub body: String,
    pub url: String,
    pub level: Level,
    /// Groups notifications in Notification Center.
    pub thread: String,
    /// Drop it if Apple can't deliver it in this many seconds (a ring is useless after it stops).
    pub ttl_secs: Option<u64>,
}

impl Notification {
    pub fn alarm(id: &str, label: &str, reminder: bool, late_secs: i64) -> Notification {
        let late = if late_secs > 300 { format!(" (late by {} min: the server was off)", late_secs / 60) } else { String::new() };
        let (kind, title) = if reminder { (Kind::Reminder, "Reminder") } else { (Kind::Alarm, "Alarm") };
        Notification {
            kind,
            id: id.into(),
            title: title.into(),
            body: format!("{}{late}", label.trim()),
            url: "aos://today".into(),
            // A late alarm already missed its moment: tell him, don't break through Focus for it.
            level: if !reminder && late_secs <= 300 { Level::TimeSensitive } else { Level::Active },
            thread: format!("aos-{}", kind.as_str()),
            ttl_secs: None,
        }
    }

    pub fn call(ring_id: i64, reason: &str, ring_secs: u64) -> Notification {
        let body = if reason.trim().is_empty() { "Tap to answer".to_string() } else { reason.trim().to_string() };
        Notification {
            kind: Kind::Call,
            id: ring_id.to_string(),
            title: "Nora is calling".into(),
            body,
            url: format!("aos://call/ring/{ring_id}"),
            level: Level::TimeSensitive,
            thread: "aos-call".into(),
            ttl_secs: Some(ring_secs),
        }
    }

    pub fn approval(id: &str, thread_id: &str, title: &str) -> Notification {
        Notification {
            kind: Kind::Approval,
            id: id.into(),
            title: "Work needs your OK".into(),
            body: clip(title, 140),
            url: format!("aos://work/{thread_id}"),
            level: Level::Active,
            thread: format!("aos-work-{thread_id}"),
            ttl_secs: None,
        }
    }

    /// The sender and, for mail, the subject. Never the message text.
    pub fn inbox(id: &str, source: &str, sender: &str, subject: &str) -> Notification {
        let from = if sender.trim().is_empty() { "someone" } else { sender.trim() };
        let body = match source {
            "whatsapp" => "New WhatsApp message".to_string(),
            _ if !subject.trim().is_empty() => clip(subject, 120),
            _ => "New message".to_string(),
        };
        Notification {
            kind: Kind::Inbox,
            id: id.into(),
            title: format!("Important: {}", clip(from, 60)),
            body,
            url: format!("aos://inbox/{id}"),
            level: Level::Active,
            thread: "aos-inbox".into(),
            ttl_secs: None,
        }
    }

    pub fn test() -> Notification {
        Notification {
            kind: Kind::Test,
            id: format!("test-{}", crate::apns::unix_now()),
            title: "Agentic OS".into(),
            body: "Push works on this phone.".into(),
            url: "aos://today".into(),
            level: Level::Active,
            thread: "aos-test".into(),
            ttl_secs: None,
        }
    }

    /// The APNs request: alert push, priority 10, collapse id per thing so a retry replaces itself.
    pub fn apns(&self, now: u64) -> ApnsRequest {
        let aps = json!({
            "alert": { "title": self.title, "body": self.body },
            "sound": "default",
            "category": self.kind.category(),
            "thread-id": self.thread,
            "interruption-level": match self.level { Level::TimeSensitive => "time-sensitive", Level::Active => "active" },
        });
        let body: Value = json!({
            "aps": aps,
            "aos": { "id": self.id, "kind": self.kind.as_str(), "url": self.url },
        });
        ApnsRequest {
            push_type: "alert",
            priority: 10,
            expiration: Some(self.ttl_secs.map(|t| now + t).unwrap_or(0)).filter(|e| *e > 0),
            collapse_id: Some(format!("{}-{}", self.kind.as_str(), self.id)).map(|c| clip_bytes(&c, 64)),
            body,
        }
    }

    /// The dedupe key: one push per thing, ever.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.id)
    }
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// apns-collapse-id is at most 64 bytes.
fn clip_bytes(s: &str, max: usize) -> String {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alarm_is_time_sensitive_and_reminder_is_not() {
        let a = Notification::alarm("a1", "Wake up", false, 0).apns(1000);
        assert_eq!(a.body["aps"]["interruption-level"], "time-sensitive");
        assert_eq!(a.body["aps"]["category"], "AOS_ALARM");
        assert_eq!(a.body["aps"]["alert"]["body"], "Wake up");
        assert_eq!(a.body["aos"], json!({ "id": "a1", "kind": "alarm", "url": "aos://today" }));
        assert_eq!(a.push_type, "alert");
        assert_eq!(a.priority, 10);
        assert_eq!(a.expiration, None);
        assert_eq!(a.collapse_id.as_deref(), Some("alarm-a1"));

        let r = Notification::alarm("r1", "Call the bank", true, 0).apns(1000);
        assert_eq!(r.body["aps"]["interruption-level"], "active");
        assert_eq!(r.body["aps"]["alert"]["title"], "Reminder");
    }

    #[test]
    fn late_alarm_says_so_and_stays_quiet() {
        let n = Notification::alarm("a1", "Gym", false, 1800);
        assert_eq!(n.body, "Gym (late by 30 min: the server was off)");
        assert_eq!(n.level, Level::Active);
    }

    #[test]
    fn call_rings_once_and_expires_with_the_ring() {
        let n = Notification::call(12, "Close the day: your journaling call", 90);
        let r = n.apns(1_000);
        assert_eq!(r.body["aps"]["alert"]["title"], "Nora is calling");
        assert_eq!(r.body["aps"]["category"], "AOS_CALL");
        assert_eq!(r.body["aps"]["interruption-level"], "time-sensitive");
        assert_eq!(r.body["aos"]["url"], "aos://call/ring/12");
        assert_eq!(r.expiration, Some(1_090));
        assert_eq!(n.key(), "call:12");
    }

    #[test]
    fn inbox_never_carries_the_message_text() {
        let w = Notification::inbox("i1", "whatsapp", "Rahul", "");
        assert_eq!((w.title.as_str(), w.body.as_str()), ("Important: Rahul", "New WhatsApp message"));
        let m = Notification::inbox("i2", "gmail", "Bank", "Your statement is ready");
        assert_eq!(m.body, "Your statement is ready");
        assert_eq!(m.url, "aos://inbox/i2");
        assert_eq!(m.level, Level::Active);
    }

    #[test]
    fn approval_links_to_its_thread() {
        let n = Notification::approval("ap1", "th9", "Run: cargo test --workspace");
        let r = n.apns(0);
        assert_eq!(r.body["aos"]["url"], "aos://work/th9");
        assert_eq!(r.body["aps"]["thread-id"], "aos-work-th9");
        assert_eq!(r.body["aps"]["interruption-level"], "active");
    }

    #[test]
    fn long_text_is_clipped() {
        let long = "x".repeat(500);
        assert_eq!(Notification::approval("a", "t", &long).body.chars().count(), 140);
        let id = "é".repeat(60);
        let c = Notification::approval(&id, "t", "x").apns(0).collapse_id.unwrap();
        assert!(c.len() <= 64);
    }
}
