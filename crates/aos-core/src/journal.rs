//! Journal entries. The text is stored exactly as given: no trimming, no rewriting.
//! Writing on a day earns [`crate::xp::JOURNAL_XP`] once for that day.

use chrono::{Local, NaiveDate};
use rusqlite::{Row, params};
use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::model::JournalEntry;
use crate::store::{ChangeOp, Store};
use crate::{ids, time, xp};

#[derive(Debug, Clone, Default)]
pub struct NewJournal {
    /// Verbatim. Only an all-whitespace text is refused.
    pub text: String,
    /// Defaults to `journal`.
    pub kind: Option<String>,
    /// Defaults to today.
    pub date: Option<NaiveDate>,
    /// `app`, `call`, `cli`, `mcp`...
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalAdded {
    pub entry: JournalEntry,
    pub xp_delta: i64,
}

const COLS: &str = "id, date, at, kind, text, nora_read, source";

fn entry_from_row(r: &Row<'_>) -> rusqlite::Result<JournalEntry> {
    Ok(JournalEntry {
        id: r.get(0)?,
        date: r.get(1)?,
        at: r.get(2)?,
        kind: r.get(3)?,
        text: r.get(4)?,
        nora_read: r.get(5)?,
        source: r.get(6)?,
    })
}

impl Store {
    pub fn add_journal(&self, by: &str, j: NewJournal) -> Result<JournalAdded> {
        if j.text.trim().is_empty() {
            return Err(invalid("journal text is empty"));
        }
        let now = time::now();
        let date = j
            .date
            .unwrap_or_else(|| now.with_timezone(&Local).date_naive());
        let kind = j.kind.unwrap_or_else(|| "journal".into());
        self.write(by, |tx| {
            let id = ids::new_id();
            tx.execute(
                "INSERT INTO journal_entry (id, date, at, kind, text, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![id, date, time::ts(now), kind, j.text, j.source.as_deref().unwrap_or("app"), tx.now()],
            )?;
            tx.record("journal_entry", &id, ChangeOp::Upsert)?;
            let day_key = format!("journal:{}", date.format("%Y-%m-%d"));
            // Journal XP goes to the anchor habit's area (journaling is the anchor), if set.
            let area: Option<String> = tx
                .query_row(
                    "SELECT area_id FROM habit WHERE anchor = 1 AND archived = 0 ORDER BY sort LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .ok()
                .flatten();
            let xp_delta = xp::set_award(tx, "journal", &day_key, area.as_deref(), xp::JOURNAL_XP, date)?;
            let entry =
                tx.query_row(&format!("SELECT {COLS} FROM journal_entry WHERE id = ?1"), [&id], entry_from_row)?;
            Ok(JournalAdded { entry, xp_delta })
        })
    }

    /// Entries from `from` to `to` (inclusive), newest first, at most `limit` (0 = all).
    pub fn read_journal(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        limit: usize,
    ) -> Result<Vec<JournalEntry>> {
        self.read(|c| {
            let mut sql = format!(
                "SELECT {COLS} FROM journal_entry WHERE date >= ?1 AND date <= ?2 ORDER BY at DESC"
            );
            if limit > 0 {
                sql += &format!(" LIMIT {limit}");
            }
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params![from, to], entry_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn journal_written_on(&self, day: NaiveDate) -> Result<bool> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM journal_entry WHERE date = ?1)",
                [day],
                |r| r.get(0),
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_kept_verbatim() {
        let store = Store::open_temporary().unwrap();
        let raw = "  Aaj ka din... tough tha.\n\n  Line two with trailing spaces   \n";
        let day = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let added = store
            .add_journal(
                "t",
                NewJournal {
                    text: raw.into(),
                    date: Some(day),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(added.entry.text, raw);
        let back = store.read_journal(day, day, 0).unwrap();
        assert_eq!(back[0].text, raw);
    }

    #[test]
    fn xp_once_per_day() {
        let store = Store::open_temporary().unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let entry = |t: &str| NewJournal {
            text: t.into(),
            date: Some(day),
            ..Default::default()
        };
        assert_eq!(
            store.add_journal("t", entry("one")).unwrap().xp_delta,
            xp::JOURNAL_XP
        );
        assert_eq!(store.add_journal("t", entry("two")).unwrap().xp_delta, 0);
        assert!(store.journal_written_on(day).unwrap());
        assert!(store.add_journal("t", entry("   ")).is_err());
    }
}
