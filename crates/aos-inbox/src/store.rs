//! `inbox_item` and `inbox_reply` tables.
//!
//! The tables live in the main Agentic OS database. They are created on first use (so no change
//! to foundation's migrations) and every write goes through `Store::write`, which lets us add a
//! `change_log` row: the server's event bus then pushes `inbox_item` changes to clients.

use std::sync::Mutex;

use aos_core::store::ChangeOp;
use aos_core::{Error, Result, Store, ids, time};
use rusqlite::{Connection, OptionalExtension, Row, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const ENTITY: &str = "inbox_item";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS inbox_item (
    id           TEXT PRIMARY KEY,
    source       TEXT NOT NULL CHECK (source IN ('whatsapp', 'gmail', 'workmail', 'app')),
    account      TEXT NOT NULL DEFAULT '',
    external_id  TEXT NOT NULL,
    chat         TEXT NOT NULL DEFAULT '',
    chat_name    TEXT NOT NULL DEFAULT '',
    sender       TEXT NOT NULL DEFAULT '',
    sender_name  TEXT NOT NULL DEFAULT '',
    subject      TEXT NOT NULL DEFAULT '',
    text         TEXT NOT NULL DEFAULT '',
    at           TEXT NOT NULL,
    from_me      INTEGER NOT NULL DEFAULT 0,
    triage_score REAL,
    triage_yes   INTEGER,
    triage_by    TEXT,
    handled      INTEGER NOT NULL DEFAULT 0,
    handled_at   TEXT,
    note         TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    UNIQUE (source, account, external_id)
);
CREATE INDEX IF NOT EXISTS inbox_item_open ON inbox_item(handled, at);
CREATE INDEX IF NOT EXISTS inbox_item_chat ON inbox_item(source, account, chat, at);

CREATE TABLE IF NOT EXISTS inbox_reply (
    id         TEXT PRIMARY KEY,
    item_id    TEXT NOT NULL REFERENCES inbox_item(id) ON DELETE CASCADE,
    text       TEXT NOT NULL,
    status     TEXT NOT NULL CHECK (status IN ('draft', 'queued', 'sent', 'failed')),
    error      TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS inbox_reply_status ON inbox_reply(status);
"#;

/// Database files already prepared in this process.
static READY: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

/// Create the tables if they are missing. Cheap after the first call per database file.
pub fn ensure_schema(store: &Store) -> Result<()> {
    {
        let ready = READY.lock().unwrap_or_else(|e| e.into_inner());
        if ready.iter().any(|p| p == store.path()) {
            return Ok(());
        }
    }
    store.write("inbox", |tx| {
        tx.execute_batch(SCHEMA)?;
        Ok(())
    })?;
    READY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(store.path().to_path_buf());
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Whatsapp,
    Gmail,
    Workmail,
    App,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Whatsapp => "whatsapp",
            Source::Gmail => "gmail",
            Source::Workmail => "workmail",
            Source::App => "app",
        }
    }
}

/// A message about to enter the inbox.
#[derive(Debug, Clone)]
pub struct NewItem {
    pub source: Source,
    pub account: String,
    /// The provider's message id; with source and account it makes the row unique.
    pub external_id: String,
    pub chat: String,
    pub chat_name: String,
    pub sender: String,
    pub sender_name: String,
    pub subject: String,
    pub text: String,
    /// RFC 3339 UTC.
    pub at: String,
    pub from_me: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub source: String,
    pub account: String,
    pub chat: String,
    pub chat_name: String,
    pub sender: String,
    pub sender_name: String,
    pub subject: String,
    pub text: String,
    pub at: String,
    pub from_me: bool,
    /// Probability that Nora should read it (0..1). Missing when triage has not run.
    pub triage_score: Option<f64>,
    pub triage_yes: Option<bool>,
    /// `laya`, or `fallback` when the model was down.
    pub triage_by: Option<String>,
    pub handled: bool,
    pub note: Option<String>,
}

const COLS: &str = "id, source, account, chat, chat_name, sender, sender_name, subject, text, at, \
                    from_me, triage_score, triage_yes, triage_by, handled, note";

fn row_item(r: &Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        source: r.get(1)?,
        account: r.get(2)?,
        chat: r.get(3)?,
        chat_name: r.get(4)?,
        sender: r.get(5)?,
        sender_name: r.get(6)?,
        subject: r.get(7)?,
        text: r.get(8)?,
        at: r.get(9)?,
        from_me: r.get::<_, i64>(10)? != 0,
        triage_score: r.get(11)?,
        triage_yes: r.get::<_, Option<i64>>(12)?.map(|v| v != 0),
        triage_by: r.get(13)?,
        handled: r.get::<_, i64>(14)? != 0,
        note: r.get(15)?,
    })
}

/// Insert a message. Returns the new row, or `None` when it was already stored. Messages sent by
/// the owner are stored already handled.
pub fn insert_item(store: &Store, actor: &str, n: &NewItem) -> Result<Option<Item>> {
    ensure_schema(store)?;
    store.write(actor, |tx| {
        let id = ids::new_id();
        let now = tx.now().to_string();
        let changed = tx.execute(
            "INSERT OR IGNORE INTO inbox_item
               (id, source, account, external_id, chat, chat_name, sender, sender_name, subject,
                text, at, from_me, handled, handled_at, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?15)",
            params![
                id,
                n.source.as_str(),
                n.account,
                n.external_id,
                n.chat,
                n.chat_name,
                n.sender,
                n.sender_name,
                n.subject,
                n.text,
                n.at,
                n.from_me,
                n.from_me,
                n.from_me.then(|| now.clone()),
                now
            ],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        tx.record(ENTITY, &id, ChangeOp::Upsert)?;
        Ok(Some(get(tx, &id)?))
    })
}

fn get(c: &Connection, id: &str) -> Result<Item> {
    c.query_row(
        &format!("SELECT {COLS} FROM inbox_item WHERE id = ?1"),
        [id],
        row_item,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound {
        kind: "inbox item",
        id: id.to_string(),
    })
}

/// Resolve an id or a unique prefix of 4+ characters.
pub fn resolve(c: &Connection, key: &str) -> Result<String> {
    let key = key.trim();
    if key.len() < 4 {
        return Err(Error::Invalid(
            "inbox id needs at least 4 characters".into(),
        ));
    }
    let like = format!("{}%", key.replace(['%', '_'], ""));
    let mut stmt = c.prepare("SELECT id FROM inbox_item WHERE id LIKE ?1 LIMIT 3")?;
    let ids: Vec<String> = stmt
        .query_map([like], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    match ids.len() {
        0 => Err(Error::NotFound {
            kind: "inbox item",
            id: key.to_string(),
        }),
        1 => Ok(ids.into_iter().next().unwrap_or_default()),
        n => Err(Error::Ambiguous {
            kind: "inbox item",
            id: key.to_string(),
            count: n,
        }),
    }
}

pub fn get_item(store: &Store, key: &str) -> Result<Item> {
    ensure_schema(store)?;
    store.read(|c| {
        let id = resolve(c, key)?;
        get(c, &id)
    })
}

#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub source: Option<String>,
    /// `Some(true)`: only handled; `Some(false)`: only open; `None`: both.
    pub handled: Option<bool>,
    /// Only items triage said Nora should read (or that were never triaged).
    pub only_read: bool,
    pub chat: Option<String>,
    pub limit: usize,
}

/// Newest first.
pub fn list_items(store: &Store, f: &Filter) -> Result<Vec<Item>> {
    ensure_schema(store)?;
    store.read(|c| {
        let mut sql = format!("SELECT {COLS} FROM inbox_item WHERE 1=1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(s) = &f.source {
            args.push(s.clone().into());
            sql.push_str(&format!(" AND source = ?{}", args.len()));
        }
        if let Some(h) = f.handled {
            sql.push_str(if h {
                " AND handled = 1"
            } else {
                " AND handled = 0"
            });
        }
        if f.only_read {
            sql.push_str(" AND (triage_yes IS NULL OR triage_yes = 1)");
        }
        if let Some(ch) = &f.chat {
            args.push(ch.clone().into());
            sql.push_str(&format!(" AND chat = ?{}", args.len()));
        }
        sql.push_str(&format!(
            " ORDER BY at DESC, id DESC LIMIT {}",
            f.limit.clamp(1, 500)
        ));
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), row_item)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
}

/// `(open items, open items Nora should read)`.
pub fn open_counts(store: &Store) -> Result<(i64, i64)> {
    ensure_schema(store)?;
    store.read(|c| {
        Ok(c.query_row(
            "SELECT COUNT(*), COALESCE(SUM(triage_yes IS NULL OR triage_yes = 1), 0)
             FROM inbox_item WHERE handled = 0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })
}

/// The last `n` texts of the same chat before `item`, oldest first (context for triage).
pub fn recent_chat(store: &Store, item: &Item, n: usize) -> Result<Vec<String>> {
    store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT text, from_me FROM inbox_item
             WHERE source = ?1 AND account = ?2 AND chat = ?3 AND at < ?4 AND chat <> ''
             ORDER BY at DESC LIMIT ?5",
        )?;
        let mut rows: Vec<String> = stmt
            .query_map(
                params![item.source, item.account, item.chat, item.at, n as i64],
                |r| {
                    let text: String = r.get(0)?;
                    let me: i64 = r.get(1)?;
                    Ok(format!("{}: {}", if me != 0 { "me" } else { "them" }, text))
                },
            )?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    })
}

pub fn set_triage(store: &Store, id: &str, score: Option<f64>, yes: bool, by: &str) -> Result<()> {
    store.write("triage", |tx| {
        let n = tx.execute(
            "UPDATE inbox_item SET triage_score = ?2, triage_yes = ?3, triage_by = ?4, updated_at = ?5
             WHERE id = ?1",
            params![id, score, yes, by, tx.now()],
        )?;
        if n > 0 {
            tx.record(ENTITY, id, ChangeOp::Upsert)?;
        }
        Ok(())
    })
}

/// Set handled / unhandled, with an optional note.
pub fn mark(
    store: &Store,
    actor: &str,
    key: &str,
    handled: bool,
    note: Option<&str>,
) -> Result<Item> {
    ensure_schema(store)?;
    store.write(actor, |tx| {
        let id = resolve(tx, key)?;
        tx.execute(
            "UPDATE inbox_item SET handled = ?2, handled_at = ?3, note = COALESCE(?4, note),
                    updated_at = ?5 WHERE id = ?1",
            params![
                id,
                handled,
                handled.then(|| tx.now().to_string()),
                note,
                tx.now()
            ],
        )?;
        tx.record(ENTITY, &id, ChangeOp::Upsert)?;
        get(tx, &id)
    })
}

// Replies ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Reply {
    pub id: String,
    pub item_id: String,
    pub text: String,
    pub status: String,
    pub error: Option<String>,
}

fn row_reply(r: &Row<'_>) -> rusqlite::Result<Reply> {
    Ok(Reply {
        id: r.get(0)?,
        item_id: r.get(1)?,
        text: r.get(2)?,
        status: r.get(3)?,
        error: r.get(4)?,
    })
}

const REPLY_COLS: &str = "id, item_id, text, status, error";

/// Store a reply as a draft. Nothing is sent.
pub fn save_draft(
    store: &Store,
    actor: &str,
    item_key: &str,
    text: &str,
) -> Result<(Item, Reply)> {
    ensure_schema(store)?;
    if text.trim().is_empty() {
        return Err(Error::Invalid("reply text is empty".into()));
    }
    store.write(actor, |tx| {
        let item_id = resolve(tx, item_key)?;
        let item = get(tx, &item_id)?;
        // One open draft per item: a new text replaces the old draft.
        tx.execute(
            "DELETE FROM inbox_reply WHERE item_id = ?1 AND status = 'draft'",
            [&item_id],
        )?;
        let id = ids::new_id();
        tx.execute(
            "INSERT INTO inbox_reply (id, item_id, text, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'draft', ?4, ?4)",
            params![id, item_id, text, tx.now()],
        )?;
        let reply = tx.query_row(
            &format!("SELECT {REPLY_COLS} FROM inbox_reply WHERE id = ?1"),
            [&id],
            row_reply,
        )?;
        Ok((item, reply))
    })
}

/// Turn a draft into a queued send. Only the explicit confirm path of `inbox_reply` calls this.
pub fn queue_reply(store: &Store, actor: &str, reply_id: &str) -> Result<Reply> {
    store.write(actor, |tx| {
        let n = tx.execute(
            "UPDATE inbox_reply SET status = 'queued', updated_at = ?2 WHERE id = ?1 AND status = 'draft'",
            params![reply_id, tx.now()],
        )?;
        if n == 0 {
            return Err(Error::Invalid("reply is not a draft".into()));
        }
        Ok(tx.query_row(
            &format!("SELECT {REPLY_COLS} FROM inbox_reply WHERE id = ?1"),
            [reply_id],
            row_reply,
        )?)
    })
}

/// Queued replies with the item they answer, oldest first.
pub fn queued_replies(store: &Store, source: Source) -> Result<Vec<(Reply, Item)>> {
    ensure_schema(store)?;
    store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT r.id, r.item_id, r.text, r.status, r.error FROM inbox_reply r
             JOIN inbox_item i ON i.id = r.item_id
             WHERE r.status = 'queued' AND i.source = ?1 ORDER BY r.created_at",
        )?;
        let replies: Vec<Reply> = stmt
            .query_map([source.as_str()], row_reply)?
            .collect::<rusqlite::Result<_>>()?;
        replies
            .into_iter()
            .map(|r| {
                let item = get(c, &r.item_id)?;
                Ok((r, item))
            })
            .collect()
    })
}

/// Record the outcome of a send. A successful send also marks the item handled.
pub fn finish_reply(store: &Store, reply_id: &str, error: Option<&str>) -> Result<()> {
    store.write("inbox", |tx| {
        tx.execute(
            "UPDATE inbox_reply SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
            params![
                reply_id,
                if error.is_none() { "sent" } else { "failed" },
                error,
                tx.now()
            ],
        )?;
        if error.is_none() {
            let item_id: String = tx.query_row(
                "SELECT item_id FROM inbox_reply WHERE id = ?1",
                [reply_id],
                |r| r.get(0),
            )?;
            tx.execute(
                "UPDATE inbox_item SET handled = 1, handled_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND handled = 0",
                params![item_id, tx.now()],
            )?;
            tx.record(ENTITY, &item_id, ChangeOp::Upsert)?;
        }
        Ok(())
    })
}

pub fn now_ts() -> String {
    time::ts(time::now())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn sample(ext: &str, chat: &str, text: &str, at: &str) -> NewItem {
        NewItem {
            source: Source::Whatsapp,
            account: "main".into(),
            external_id: ext.into(),
            chat: chat.into(),
            chat_name: "Test Chat".into(),
            sender: "910000000001@s.whatsapp.net".into(),
            sender_name: "Sam".into(),
            subject: String::new(),
            text: text.into(),
            at: at.into(),
            from_me: false,
        }
    }

    #[test]
    fn insert_dedupes_and_logs_changes() {
        let s = Store::open_temporary().unwrap();
        let a = insert_item(&s, "t", &sample("m1", "c1", "hello", "2026-01-01T10:00:00.000Z")).unwrap();
        assert!(a.is_some());
        let b = insert_item(&s, "t", &sample("m1", "c1", "hello", "2026-01-01T10:00:00.000Z")).unwrap();
        assert!(b.is_none(), "same external id is stored once");
        let notices = s.change_notices(0, 100).unwrap();
        assert!(notices.iter().any(|n| n.entity == ENTITY && n.op == "upsert"));
    }

    #[test]
    fn mark_triage_and_context() {
        let s = Store::open_temporary().unwrap();
        let a = insert_item(&s, "t", &sample("m1", "c1", "first", "2026-01-01T10:00:00.000Z"))
            .unwrap()
            .unwrap();
        let b = insert_item(&s, "t", &sample("m2", "c1", "second", "2026-01-01T10:01:00.000Z"))
            .unwrap()
            .unwrap();
        assert_eq!(recent_chat(&s, &b, 5).unwrap(), vec!["them: first".to_string()]);
        set_triage(&s, &a.id, Some(0.1), false, "laya").unwrap();
        let open = list_items(
            &s,
            &Filter { handled: Some(false), only_read: true, limit: 10, ..Default::default() },
        )
        .unwrap();
        assert_eq!(open.len(), 1, "an item triaged 'no' is hidden from the read list");
        let m = mark(&s, "t", &b.id[..6], true, Some("done")).unwrap();
        assert!(m.handled && m.note.as_deref() == Some("done"));
        assert_eq!(open_counts(&s).unwrap().0, 1);
    }

    #[test]
    fn reply_is_draft_until_queued() {
        let s = Store::open_temporary().unwrap();
        let a = insert_item(&s, "t", &sample("m1", "c1", "hi", "2026-01-01T10:00:00.000Z"))
            .unwrap()
            .unwrap();
        let (_, d) = save_draft(&s, "t", &a.id, "ok").unwrap();
        assert_eq!(d.status, "draft");
        assert!(queued_replies(&s, Source::Whatsapp).unwrap().is_empty());
        queue_reply(&s, "t", &d.id).unwrap();
        assert_eq!(queued_replies(&s, Source::Whatsapp).unwrap().len(), 1);
        finish_reply(&s, &d.id, None).unwrap();
        assert!(get_item(&s, &a.id).unwrap().handled);
    }
}
