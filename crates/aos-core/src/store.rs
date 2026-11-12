//! The SQLite store.
//!
//! Rules (research/tech-stack.md, risk R13):
//! - WAL mode, so readers never block the writer.
//! - Exactly one writer connection behind a mutex, so writes never fight (`database is locked`).
//! - Every write runs in a `BEGIN IMMEDIATE` transaction and records what it touched in
//!   `change_log`, which drives sync and the live event bus.
//! - A few reader connections for queries.
//!
//! Other processes (the `aos` CLI while the server runs) may write the same file. SQLite's file
//! lock plus `busy_timeout` serialises them, and the server notices their rows in `change_log`.

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

use crate::error::Result;
use crate::{ids, seed, time};

/// Schema migrations, applied in order. `PRAGMA user_version` stores how many have run.
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql")];

const READERS: usize = 4;

/// Handle to the database. Share it behind an `Arc`; all methods take `&self`.
pub struct Store {
    path: PathBuf,
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    next_reader: AtomicUsize,
    created: bool,
    delete_on_drop: bool,
}

/// A write transaction. Derefs to [`Connection`] for queries; call [`Tx::record`] for every
/// row you insert, update or delete in a synced table.
pub struct Tx<'c> {
    tx: rusqlite::Transaction<'c>,
    actor: &'c str,
    now: String,
}

impl Deref for Tx<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.tx
    }
}

impl Tx<'_> {
    /// The transaction's timestamp, used for `created_at` / `updated_at`.
    pub fn now(&self) -> &str {
        &self.now
    }

    /// Who is writing (a device id, `cli`, `mcp`, `seed`).
    pub fn actor(&self) -> &str {
        self.actor
    }

    /// Add a `change_log` row for a synced table.
    pub fn record(&self, entity: &str, row_id: &str, op: ChangeOp) -> Result<()> {
        self.tx.execute(
            "INSERT INTO change_log (entity, row_id, op, at, actor) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![entity, row_id, op.as_str(), self.now, self.actor],
        )?;
        Ok(())
    }
}

/// What happened to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOp {
    Upsert,
    Delete,
}

impl ChangeOp {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeOp::Upsert => "upsert",
            ChangeOp::Delete => "delete",
        }
    }
}

impl Store {
    /// Open (or create) the database at `path`, run migrations, and seed the generic pillars
    /// and areas the first time.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)?;
        }
        let mut writer = open_conn(&path, false)?;
        writer.pragma_update(None, "journal_mode", "WAL")?;
        let fresh = migrate(&mut writer)?;
        crate::mission::ensure_schema(&writer)?;

        let readers = (0..READERS)
            .map(|_| open_conn(&path, true).map(Mutex::new))
            .collect::<Result<Vec<_>>>()?;

        let store = Store {
            path,
            writer: Mutex::new(writer),
            readers,
            next_reader: AtomicUsize::new(0),
            created: fresh,
            delete_on_drop: false,
        };
        if fresh {
            seed::apply(&store, "seed", &seed::SeedFile::generic())?;
        }
        Ok(store)
    }

    /// A store in a new file under the system temp folder, deleted on drop. For tests and
    /// quick checks.
    pub fn open_temporary() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("aos-test-{}.db", ids::new_id()));
        let mut store = Self::open(&path)?;
        store.delete_on_drop = true;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True when this `open` created the database (callers then apply the personal seed).
    pub fn was_created(&self) -> bool {
        self.created
    }

    /// Run read-only queries on a reader connection.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.reader();
        f(&conn)
    }

    /// Run `f` inside one `BEGIN IMMEDIATE` transaction on the single writer. Commits when `f`
    /// returns `Ok`, rolls back on `Err`.
    pub fn write<T>(&self, actor: &str, f: impl FnOnce(&Tx<'_>) -> Result<T>) -> Result<T> {
        let mut conn = lock(&self.writer);
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let w = Tx {
            tx,
            actor,
            now: time::ts(time::now()),
        };
        let out = f(&w)?;
        w.tx.commit()?;
        Ok(out)
    }

    fn reader(&self) -> MutexGuard<'_, Connection> {
        // Take the first free reader; if all are busy, wait on the next one in turn.
        for r in &self.readers {
            if let Ok(guard) = r.try_lock() {
                return guard;
            }
        }
        let i = self.next_reader.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        lock(&self.readers[i])
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if !self.delete_on_drop {
            return;
        }
        // Close every connection first so Windows lets us delete the files.
        self.readers.clear();
        if let Ok(mem) = Connection::open_in_memory() {
            drop(std::mem::replace(&mut self.writer, Mutex::new(mem)));
        }
        for suffix in ["", "-wal", "-shm"] {
            let mut p = self.path.clone().into_os_string();
            p.push(suffix);
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Resolve a row id from an exact id or a unique prefix (at least 4 characters), so people and
/// agents can type `aos task done 7k2m` instead of the whole id.
pub(crate) fn resolve_prefix(c: &Connection, table: &'static str, key: &str) -> Result<String> {
    let key = key.trim();
    let exact: Option<String> = c
        .query_row(
            &format!("SELECT id FROM {table} WHERE id = ?1"),
            [key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = exact {
        return Ok(id);
    }
    if key.len() < 4
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(crate::error::not_found(table, key));
    }
    let mut stmt = c.prepare(&format!(
        "SELECT id FROM {table} WHERE id LIKE ?1 || '%' LIMIT 5"
    ))?;
    let ids = stmt
        .query_map([key], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    match ids.len() {
        0 => Err(crate::error::not_found(table, key)),
        1 => Ok(ids.into_iter().next().expect("one id")),
        n => Err(crate::Error::Ambiguous {
            kind: table,
            id: key.to_string(),
            count: n,
        }),
    }
}

/// A poisoned mutex only means another thread panicked mid-call; the connection is still fine
/// (its transaction rolled back when it dropped), so carry on.
fn lock(m: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn open_conn(path: &Path, read_only: bool) -> Result<Connection> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// Apply pending migrations. Returns true when the database was brand new.
fn migrate(conn: &mut Connection) -> Result<bool> {
    let current = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    if current >= MIGRATIONS.len() {
        return Ok(false);
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for sql in &MIGRATIONS[current..] {
        tx.execute_batch(sql)?;
    }
    tx.pragma_update(None, "user_version", MIGRATIONS.len() as i64)?;
    tx.commit()?;
    Ok(current == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(store: &Store, sql: &str) -> i64 {
        store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    #[test]
    fn opens_migrates_and_seeds() {
        let store = Store::open_temporary().unwrap();
        let mode: String = store
            .read(|c| Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        assert_eq!(count(&store, "SELECT count(*) FROM pillar"), 3);
    }

    #[test]
    fn failed_write_rolls_back() {
        let store = Store::open_temporary().unwrap();
        let before = store.latest_version().unwrap();
        let res: Result<()> = store.write("test", |tx| {
            tx.record("task", "x", ChangeOp::Upsert)?;
            Err(crate::error::invalid("boom"))
        });
        assert!(res.is_err());
        assert_eq!(store.latest_version().unwrap(), before);
    }

    #[test]
    fn reopen_keeps_data_and_skips_seed() {
        let path = std::env::temp_dir().join(format!("aos-reopen-{}.db", ids::new_id()));
        {
            let s = Store::open(&path).unwrap();
            s.write("t", |tx| {
                tx.execute("DELETE FROM area WHERE id = 'reading'", [])?;
                Ok(())
            })
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(
            count(&s, "SELECT count(*) FROM area WHERE id = 'reading'"),
            0,
            "seed must not run again on an existing database"
        );
        drop(s);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
