//! Push tokens per paired device, the alarms each phone rings locally, and the sent-once log.
//!
//! Own tables in the main database (created if missing, like Nora's). A token belongs to the
//! device that registered it; a revoked device gets no pushes (joined at send time), and a token
//! Apple calls dead is deleted.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS push_token (
    token       TEXT PRIMARY KEY,
    device_id   TEXT NOT NULL,
    platform    TEXT NOT NULL,              -- ios | android (FCM later)
    env         TEXT NOT NULL DEFAULT 'production',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    last_ok_at  INTEGER,
    last_error  TEXT
);
CREATE INDEX IF NOT EXISTS push_token_device ON push_token(device_id);
-- Alarms a phone has scheduled on itself, so the server doesn't push the same alarm again.
CREATE TABLE IF NOT EXISTS push_local (
    device_id   TEXT NOT NULL,
    alarm_id    TEXT NOT NULL,
    at          INTEGER NOT NULL,
    PRIMARY KEY (device_id, alarm_id)
);
-- One push per thing (ring once), across restarts.
CREATE TABLE IF NOT EXISTS push_sent (
    key         TEXT PRIMARY KEY,
    at          INTEGER NOT NULL
);
";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TokenRow {
    #[serde(skip)]
    pub token: String,
    /// The last 6 characters, for status screens.
    pub tail: String,
    pub device_id: String,
    pub platform: String,
    pub env: String,
    pub last_ok_at: Option<i64>,
    pub last_error: Option<String>,
}

pub struct PushStore {
    conn: Mutex<Connection>,
}

pub fn now_ms() -> i64 {
    (crate::apns::unix_now() as i64) * 1000
}

impl PushStore {
    pub fn open(path: &Path) -> Result<PushStore> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(PushStore { conn: Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Save (or move) a token to this device. APNs tokens are hex; FCM tokens are longer base64-ish.
    pub fn register(&self, device_id: &str, token: &str, platform: &str, env: &str) -> Result<()> {
        let token = token.trim();
        if token.len() < 32 || token.len() > 4096 || !token.chars().all(|c| c.is_ascii_alphanumeric() || "-_:".contains(c)) {
            bail!("that is not a push token");
        }
        if !matches!(platform, "ios" | "android") {
            bail!("platform is ios or android");
        }
        let env = if matches!(env, "sandbox" | "development") { "sandbox" } else { "production" };
        let now = now_ms();
        self.c().execute(
            "INSERT INTO push_token (token, device_id, platform, env, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(token) DO UPDATE SET device_id = ?2, platform = ?3, env = ?4, updated_at = ?5, last_error = NULL",
            params![token, device_id, platform, env, now],
        )?;
        Ok(())
    }

    pub fn unregister(&self, device_id: &str, token: &str) -> Result<usize> {
        Ok(self.c().execute("DELETE FROM push_token WHERE token = ?1 AND device_id = ?2", params![token.trim(), device_id])?)
    }

    pub fn forget(&self, token: &str) -> Result<()> {
        self.c().execute("DELETE FROM push_token WHERE token = ?1", [token])?;
        Ok(())
    }

    /// Tokens of devices that are still paired. `device` limits it to one device.
    pub fn active(&self, device: Option<&str>) -> Result<Vec<TokenRow>> {
        let c = self.c();
        // The device table belongs to aos-core; without it (tests of this crate alone) every token counts.
        let has_devices: bool = c
            .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'device'", [], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        let sql = if has_devices {
            "SELECT t.token, t.device_id, t.platform, t.env, t.last_ok_at, t.last_error FROM push_token t
             JOIN device d ON d.id = t.device_id WHERE d.revoked_at IS NULL AND (?1 IS NULL OR t.device_id = ?1)
             ORDER BY t.updated_at DESC"
        } else {
            "SELECT token, device_id, platform, env, last_ok_at, last_error FROM push_token
             WHERE (?1 IS NULL OR device_id = ?1) ORDER BY updated_at DESC"
        };
        let mut stmt = c.prepare(sql)?;
        let rows = stmt.query_map([device], |r| {
            let token: String = r.get(0)?;
            Ok(TokenRow {
                tail: token.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect(),
                token,
                device_id: r.get(1)?,
                platform: r.get(2)?,
                env: r.get(3)?,
                last_ok_at: r.get(4)?,
                last_error: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn mark(&self, token: &str, error: Option<&str>) -> Result<()> {
        match error {
            None => self.c().execute("UPDATE push_token SET last_ok_at = ?2, last_error = NULL WHERE token = ?1", params![token, now_ms()])?,
            Some(e) => self.c().execute("UPDATE push_token SET last_error = ?2 WHERE token = ?1", params![token, e])?,
        };
        Ok(())
    }

    /// The phone's full list of alarms it has scheduled itself (replaces the old list).
    pub fn set_local(&self, device_id: &str, alarm_ids: &[String]) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM push_local WHERE device_id = ?1", [device_id])?;
        for id in alarm_ids.iter().take(500) {
            tx.execute("INSERT OR IGNORE INTO push_local (device_id, alarm_id, at) VALUES (?1, ?2, ?3)", params![device_id, id, now_ms()])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn has_local(&self, device_id: &str, alarm_id: &str) -> Result<bool> {
        Ok(self
            .c()
            .query_row("SELECT 1 FROM push_local WHERE device_id = ?1 AND alarm_id = ?2", params![device_id, alarm_id], |_| Ok(true))
            .optional()?
            .unwrap_or(false))
    }

    /// True the first time `key` is claimed; false ever after. Keeps rings and alarms to one push.
    pub fn claim(&self, key: &str) -> Result<bool> {
        let n = self.c().execute("INSERT OR IGNORE INTO push_sent (key, at) VALUES (?1, ?2)", params![key, now_ms()])?;
        Ok(n == 1)
    }

    /// Drop sent-log rows older than `days` (the log only needs to outlive retries and restarts).
    pub fn prune(&self, days: i64) -> Result<usize> {
        Ok(self.c().execute("DELETE FROM push_sent WHERE at < ?1", [now_ms() - days * 86_400_000])?)
    }
}

#[cfg(test)]
pub(crate) fn temp_db(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("aos-push-{name}-{}-{}.db", std::process::id(), now_ms()));
    let _ = std::fs::remove_file(&p);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOK: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

    #[test]
    fn register_moves_and_revoked_devices_get_nothing() {
        let path = temp_db("reg");
        let core = aos_core::Store::open(&path).unwrap();
        let phone = core.create_device_token("iPhone", Some("ios")).unwrap().device;
        let other = core.create_device_token("iPad", Some("ios")).unwrap().device;
        let s = PushStore::open(&path).unwrap();
        s.register(&phone.id, TOK, "ios", "production").unwrap();
        assert_eq!(s.active(None).unwrap().len(), 1);
        assert_eq!(s.active(None).unwrap()[0].tail, "7e8f90");
        // Same token from another device (restore from backup): it moves.
        s.register(&other.id, TOK, "ios", "sandbox").unwrap();
        let rows = s.active(None).unwrap();
        assert_eq!((rows.len(), rows[0].device_id.as_str(), rows[0].env.as_str()), (1, other.id.as_str(), "sandbox"));
        core.revoke_device(&other.id).unwrap();
        assert!(s.active(None).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_junk_tokens() {
        let s = PushStore::open(&temp_db("junk")).unwrap();
        assert!(s.register("d", "short", "ios", "production").is_err());
        assert!(s.register("d", &format!("{TOK} ; DROP"), "ios", "production").is_err());
        assert!(s.register("d", TOK, "windows", "production").is_err());
    }

    #[test]
    fn claim_is_once() {
        let s = PushStore::open(&temp_db("claim")).unwrap();
        assert!(s.claim("call:1").unwrap());
        assert!(!s.claim("call:1").unwrap());
        assert!(s.claim("call:2").unwrap());
    }

    #[test]
    fn local_alarm_list_replaces() {
        let s = PushStore::open(&temp_db("local")).unwrap();
        s.set_local("d", &["a".into(), "b".into()]).unwrap();
        assert!(s.has_local("d", "a").unwrap());
        s.set_local("d", &["b".into()]).unwrap();
        assert!(!s.has_local("d", "a").unwrap());
        assert!(s.has_local("d", "b").unwrap());
        assert!(!s.has_local("other", "b").unwrap());
    }
}
