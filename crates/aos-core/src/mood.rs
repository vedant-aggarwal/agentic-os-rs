//! Mood and energy check-ins.

use chrono::{DateTime, Local, NaiveDate, Utc};
use rusqlite::{OptionalExtension, Row, params};

use crate::error::{Result, invalid};
use crate::model::Mood;
use crate::store::{ChangeOp, Store};
use crate::{ids, time};

#[derive(Debug, Clone, Default)]
pub struct NewMood {
    /// -1.0..1.0
    pub valence: f64,
    /// 1..5
    pub energy: Option<i64>,
    pub labels: Vec<String>,
    pub reasons: Vec<String>,
    pub note: Option<String>,
    pub healthkit_id: Option<String>,
    /// When it was felt; defaults to now.
    pub at: Option<DateTime<Utc>>,
}

const MOOD_COLS: &str = "id, date, at, valence, energy, labels, reasons, note, healthkit_id";

fn mood_from_row(r: &Row<'_>) -> rusqlite::Result<Mood> {
    let list = |i: usize| -> rusqlite::Result<Vec<String>> {
        let raw: String = r.get(i)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    };
    Ok(Mood {
        id: r.get(0)?,
        date: r.get(1)?,
        at: r.get(2)?,
        valence: r.get(3)?,
        energy: r.get(4)?,
        labels: list(5)?,
        reasons: list(6)?,
        note: r.get(7)?,
        healthkit_id: r.get(8)?,
    })
}

impl Store {
    pub fn log_mood(&self, by: &str, m: NewMood) -> Result<Mood> {
        if !(-1.0..=1.0).contains(&m.valence) || m.valence.is_nan() {
            return Err(invalid("valence must be between -1.0 and 1.0"));
        }
        if let Some(e) = m.energy
            && !(1..=5).contains(&e)
        {
            return Err(invalid("energy must be 1..5"));
        }
        let at = m.at.unwrap_or_else(time::now);
        let date = at.with_timezone(&Local).date_naive();
        self.write(by, |tx| {
            let id = ids::new_id();
            tx.execute(
                "INSERT INTO mood (id, date, at, valence, energy, labels, reasons, note, healthkit_id,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![
                    id,
                    date,
                    time::ts(at),
                    m.valence,
                    m.energy,
                    serde_json::to_string(&m.labels)?,
                    serde_json::to_string(&m.reasons)?,
                    m.note,
                    m.healthkit_id,
                    tx.now()
                ],
            )?;
            tx.record("mood", &id, ChangeOp::Upsert)?;
            Ok(tx.query_row(&format!("SELECT {MOOD_COLS} FROM mood WHERE id = ?1"), [&id], mood_from_row)?)
        })
    }

    /// Check-ins on a day, oldest first.
    pub fn moods_on(&self, day: NaiveDate) -> Result<Vec<Mood>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {MOOD_COLS} FROM mood WHERE date = ?1 ORDER BY at"
            ))?;
            let rows = stmt.query_map([day], mood_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn latest_mood(&self) -> Result<Option<Mood>> {
        self.read(|c| {
            Ok(c.query_row(
                &format!("SELECT {MOOD_COLS} FROM mood ORDER BY at DESC LIMIT 1"),
                [],
                mood_from_row,
            )
            .optional()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_and_validates() {
        let store = Store::open_temporary().unwrap();
        assert!(
            store
                .log_mood(
                    "t",
                    NewMood {
                        valence: 1.5,
                        ..Default::default()
                    }
                )
                .is_err()
        );
        assert!(
            store
                .log_mood(
                    "t",
                    NewMood {
                        valence: 0.2,
                        energy: Some(9),
                        ..Default::default()
                    }
                )
                .is_err()
        );
        let m = store
            .log_mood(
                "t",
                NewMood {
                    valence: -0.4,
                    energy: Some(2),
                    labels: vec!["tired".into()],
                    reasons: vec!["sleep".into()],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(m.labels, vec!["tired"]);
        assert_eq!(store.latest_mood().unwrap().unwrap().id, m.id);
        assert_eq!(store.moods_on(m.date).unwrap().len(), 1);
    }
}
