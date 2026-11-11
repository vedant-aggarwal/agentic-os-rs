//! Device pairing and tokens.
//!
//! 1. On the PC: `aos pair new` makes a 6-digit code (valid 10 minutes, stored only as a hash).
//! 2. The phone sends the code once and gets a long-lived device token (stored only as a hash).
//! 3. Every request carries `Authorization: Bearer <token>`.
//!
//! **A failed request never logs a device out.** [`Store::authenticate`] only reads; a wrong or
//! unknown token changes nothing. Only [`Store::revoke_device`] ends a session.
//!
//! Brute force: each wrong code adds an attempt to every open code; a code dies after
//! [`MAX_CODE_ATTEMPTS`] wrong tries or when it expires, whichever comes first.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, invalid, not_found};
use crate::store::{Store, Tx, resolve_prefix};
use crate::{ids, time};

pub const MAX_CODE_ATTEMPTS: i64 = 5;
pub const DEFAULT_CODE_TTL_MINUTES: i64 = 10;
/// Only bump `last_seen_at` when it is older than this, so reads stay cheap.
const SEEN_RESOLUTION_SECS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: Option<String>,
    pub created_at: String,
    pub last_seen_at: Option<String>,
    pub revoked_at: Option<String>,
}

/// A fresh pairing code. The plain code is shown once and never stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairingCode {
    pub code: String,
    pub expires_at: String,
}

/// A paired device and its token. The plain token is returned once and never stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paired {
    pub device: Device,
    pub token: String,
}

const COLS: &str = "id, name, platform, created_at, last_seen_at, revoked_at";

fn device_from_row(r: &Row<'_>) -> rusqlite::Result<Device> {
    Ok(Device {
        id: r.get(0)?,
        name: r.get(1)?,
        platform: r.get(2)?,
        created_at: r.get(3)?,
        last_seen_at: r.get(4)?,
        revoked_at: r.get(5)?,
    })
}

fn insert_device(tx: &Tx<'_>, name: &str, platform: Option<&str>) -> Result<Paired> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid("device name is empty"));
    }
    let id = ids::new_id();
    let token = ids::new_token();
    tx.execute(
        "INSERT INTO device (id, name, platform, token_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, name, platform, ids::sha256_hex(&token), tx.now()],
    )?;
    let device = tx.query_row(
        &format!("SELECT {COLS} FROM device WHERE id = ?1"),
        [&id],
        device_from_row,
    )?;
    Ok(Paired { device, token })
}

impl Store {
    pub fn create_pairing_code(&self, ttl_minutes: i64) -> Result<PairingCode> {
        let ttl = ttl_minutes.clamp(1, 60);
        let code = ids::new_pairing_code();
        let expires = time::now() + Duration::minutes(ttl);
        self.write("pairing", |tx| {
            tx.execute(
                "INSERT INTO pairing_code (id, code_hash, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
                params![ids::new_id(), ids::sha256_hex(&code), tx.now(), time::ts(expires)],
            )?;
            Ok(())
        })?;
        Ok(PairingCode {
            code,
            expires_at: time::ts(expires),
        })
    }

    /// Trade a pairing code for a device token. Wrong codes return [`Error::Unauthorized`].
    pub fn claim_pairing_code(
        &self,
        code: &str,
        name: &str,
        platform: Option<&str>,
    ) -> Result<Paired> {
        let now = time::ts(time::now());
        let hash = ids::sha256_hex(code.trim());
        let outcome = self.write("pairing", |tx| {
            let found: Option<String> = tx
                .query_row(
                    "SELECT id FROM pairing_code
                     WHERE code_hash = ?1 AND used_at IS NULL AND expires_at > ?2 AND attempts < ?3",
                    params![hash, now, MAX_CODE_ATTEMPTS],
                    |r| r.get(0),
                )
                .optional()?;
            match found {
                Some(code_id) => {
                    let paired = insert_device(tx, name, platform)?;
                    tx.execute(
                        "UPDATE pairing_code SET used_at = ?2, device_id = ?3 WHERE id = ?1",
                        params![code_id, tx.now(), paired.device.id],
                    )?;
                    Ok(Some(paired))
                }
                None => {
                    // Count the wrong guess against every open code. This commits.
                    tx.execute(
                        "UPDATE pairing_code SET attempts = attempts + 1
                         WHERE used_at IS NULL AND expires_at > ?1",
                        [&now],
                    )?;
                    Ok(None)
                }
            }
        })?;
        outcome.ok_or(Error::Unauthorized)
    }

    /// Make a device token directly (for local agents set up from the PC: `aos pair token`).
    pub fn create_device_token(&self, name: &str, platform: Option<&str>) -> Result<Paired> {
        self.write("pairing", |tx| insert_device(tx, name, platform))
    }

    /// The device for a token, or `None`. Never revokes or changes anything on failure.
    pub fn authenticate(&self, token: &str) -> Result<Option<Device>> {
        let hash = ids::sha256_hex(token.trim());
        let device = self.read(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM device WHERE token_hash = ?1 AND revoked_at IS NULL"),
                [&hash],
                device_from_row,
            )
            .optional()?)
        })?;
        let Some(device) = device else {
            return Ok(None);
        };

        let stale = device
            .last_seen_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .is_none_or(|t| {
                (time::now() - t.with_timezone(&Utc)).num_seconds() > SEEN_RESOLUTION_SECS
            });
        if stale {
            // Best effort: a busy database must not turn a good token into a failed request.
            let _ = self.write(&device.id, |tx| {
                tx.execute(
                    "UPDATE device SET last_seen_at = ?2 WHERE id = ?1",
                    params![device.id, tx.now()],
                )?;
                Ok(())
            });
        }
        Ok(Some(device))
    }

    pub fn list_devices(&self) -> Result<Vec<Device>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!("SELECT {COLS} FROM device ORDER BY created_at"))?;
            let rows = stmt.query_map([], device_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The only way a device loses access.
    pub fn revoke_device(&self, key: &str) -> Result<Device> {
        self.write("pairing", |tx| {
            let id = resolve_prefix(tx, "device", key)?;
            tx.execute(
                "UPDATE device SET revoked_at = COALESCE(revoked_at, ?2) WHERE id = ?1",
                params![id, tx.now()],
            )?;
            tx.query_row(
                &format!("SELECT {COLS} FROM device WHERE id = ?1"),
                [&id],
                device_from_row,
            )
            .optional()?
            .ok_or_else(|| not_found("device", key))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_then_authenticate() {
        let store = Store::open_temporary().unwrap();
        let code = store.create_pairing_code(10).unwrap();
        let paired = store
            .claim_pairing_code(&code.code, "iPhone", Some("ios"))
            .unwrap();
        assert!(paired.token.starts_with("aos_"));
        let dev = store.authenticate(&paired.token).unwrap().unwrap();
        assert_eq!(dev.name, "iPhone");
        // A code works once.
        assert!(matches!(
            store.claim_pairing_code(&code.code, "Other", None),
            Err(Error::Unauthorized)
        ));
    }

    #[test]
    fn failed_requests_never_revoke() {
        let store = Store::open_temporary().unwrap();
        let paired = store.create_device_token("desktop", None).unwrap();
        for _ in 0..50 {
            assert!(store.authenticate("aos_wrong").unwrap().is_none());
            assert!(store.authenticate("").unwrap().is_none());
        }
        assert!(store.authenticate(&paired.token).unwrap().is_some());
        assert!(store.list_devices().unwrap()[0].revoked_at.is_none());
    }

    #[test]
    fn explicit_revoke_ends_session() {
        let store = Store::open_temporary().unwrap();
        let paired = store.create_device_token("old phone", None).unwrap();
        store.revoke_device(&paired.device.id).unwrap();
        assert!(store.authenticate(&paired.token).unwrap().is_none());
    }

    #[test]
    fn wrong_guesses_burn_the_code() {
        let store = Store::open_temporary().unwrap();
        let code = store.create_pairing_code(10).unwrap();
        let wrong = if code.code == "000000" {
            "000001"
        } else {
            "000000"
        };
        for _ in 0..MAX_CODE_ATTEMPTS {
            assert!(store.claim_pairing_code(wrong, "x", None).is_err());
        }
        assert!(
            store.claim_pairing_code(&code.code, "x", None).is_err(),
            "after too many wrong guesses even the right code fails"
        );
    }
}
