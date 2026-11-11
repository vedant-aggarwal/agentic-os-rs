//! Seed data.
//!
//! - [`SeedFile::generic`]: the public default (three pillars, nine areas), applied once to a
//!   new database.
//! - A personal file (`%USERPROFILE%\.aos\seed.local.toml`, never in the repo) with the same
//!   shape adds or renames pillars and areas and adds habits and goals. See `seed.example.toml`.
//!
//! Applying a seed is an upsert by id, so running it again is safe.

use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::areas::{upsert_area, upsert_pillar, upsert_project};
use crate::error::{Result, invalid};
use crate::goals::{NewGoal, upsert_goal};
use crate::habits::{NewHabit, upsert_habit};
use crate::model::{Area, Project};
use crate::store::Store;
use crate::streak::Cadence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedFile {
    #[serde(default)]
    pub pillar: Vec<SeedPillar>,
    #[serde(default)]
    pub area: Vec<SeedArea>,
    #[serde(default)]
    pub project: Vec<SeedProject>,
    #[serde(default)]
    pub habit: Vec<SeedHabit>,
    #[serde(default)]
    pub goal: Vec<SeedGoal>,
    /// Who the app is for and how the hero shows his goals. Read by the `profile` tool.
    pub profile: Option<SeedProfile>,
}

/// The personal profile: the name on the top bar and one hero slot per pillar.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedProfile {
    pub name: Option<String>,
    /// Avatar look (character art set). Free text the UI maps to its art.
    pub avatar: Option<String>,
    /// Show the companion in private mode.
    #[serde(default)]
    pub companion: bool,
    /// Habit ids shown only in private mode.
    #[serde(default)]
    pub private_habits: Vec<String>,
    #[serde(default)]
    pub hero: Vec<SeedHero>,
}

/// One goal in the hero: which UI slot (`physical`, `money`, `mind`) shows which goal.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedHero {
    pub slot: String,
    /// Goal id from `[[goal]]`.
    pub goal: Option<String>,
    /// One line under the name: variant and colour.
    pub variant: Option<String>,
    pub price_note: Option<String>,
    /// What the pillar covers.
    pub about: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedPillar {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub sort: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedArea {
    pub id: String,
    pub name: String,
    pub pillar: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub sort: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedProject {
    pub id: String,
    pub name: String,
    pub area: Option<String>,
    /// `active` (default), `paused` or `done`.
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedHabit {
    pub id: String,
    pub name: String,
    pub area: Option<String>,
    pub cue: Option<String>,
    pub normal: Option<String>,
    pub floor: Option<String>,
    /// `daily` (default) or `mon,wed,fri`.
    pub cadence: Option<String>,
    pub xp: Option<i64>,
    #[serde(default)]
    pub anchor: bool,
    /// Defaults to today.
    pub start: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedGoal {
    pub id: String,
    pub name: String,
    pub pillar: Option<String>,
    pub area: Option<String>,
    #[serde(default)]
    pub specs: Vec<String>,
    pub why: Option<String>,
    #[serde(default)]
    pub images: Vec<String>,
    /// Price in paise (₹1 = 100 paise).
    pub target_paise: Option<i64>,
    /// Starting amount in paise; only used the first time the goal is created.
    #[serde(default)]
    pub earned_paise: i64,
    /// Manual 0..1 progress for goals without a price.
    pub progress: Option<f64>,
    #[serde(default)]
    pub sort: i64,
}

/// What a seed run touched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SeedReport {
    pub pillars: usize,
    pub areas: usize,
    pub habits: usize,
    pub goals: usize,
}

impl SeedFile {
    /// The public default: three pillars (body, wealth, mind) with three areas each.
    pub fn generic() -> SeedFile {
        let pillar = |id: &str, name: &str, sort| SeedPillar {
            id: id.into(),
            name: name.into(),
            sort,
        };
        let area = |id: &str, name: &str, pillar: &str, sort| SeedArea {
            id: id.into(),
            name: name.into(),
            pillar: Some(pillar.into()),
            color: None,
            sort,
        };
        SeedFile {
            pillar: vec![
                pillar("body", "Body", 1),
                pillar("wealth", "Wealth", 2),
                pillar("mind", "Mind", 3),
            ],
            area: vec![
                area("fitness", "Fitness", "body", 1),
                area("health", "Health and sleep", "body", 2),
                area("care", "Looks and care", "body", 3),
                area("work", "Work", "wealth", 4),
                area("business", "Business", "wealth", 5),
                area("skills", "Skills that earn", "wealth", 6),
                area("learning", "Learning", "mind", 7),
                area("reading", "Reading", "mind", 8),
                area("mindset", "Mind and discipline", "mind", 9),
            ],
            project: vec![],
            habit: vec![],
            goal: vec![],
            profile: None,
        }
    }

    pub fn from_toml(text: &str) -> Result<SeedFile> {
        toml::from_str(text).map_err(|e| invalid(format!("seed file: {e}")))
    }

    pub fn load(path: &Path) -> Result<SeedFile> {
        Self::from_toml(&std::fs::read_to_string(path)?)
    }
}

/// Upsert everything in `seed` in one transaction.
pub fn apply(store: &Store, by: &str, seed: &SeedFile) -> Result<SeedReport> {
    let today = crate::time::local_today();
    store.write(by, |tx| {
        for p in &seed.pillar {
            upsert_pillar(tx, &p.id, &p.name, p.sort)?;
        }
        for a in &seed.area {
            upsert_area(
                tx,
                &Area {
                    id: a.id.clone(),
                    pillar_id: a.pillar.clone(),
                    name: a.name.clone(),
                    color: a.color.clone(),
                    sort: a.sort,
                    archived: false,
                },
            )?;
        }
        for p in &seed.project {
            upsert_project(
                tx,
                &Project {
                    id: p.id.clone(),
                    area_id: p.area.clone(),
                    name: p.name.clone(),
                    status: p.status.clone().unwrap_or_else(|| "active".into()),
                },
            )?;
        }
        for h in &seed.habit {
            let cadence: Cadence = h.cadence.as_deref().unwrap_or("daily").parse()?;
            upsert_habit(
                tx,
                &NewHabit {
                    id: Some(h.id.clone()),
                    name: h.name.clone(),
                    area_id: h.area.clone(),
                    cue: h.cue.clone(),
                    normal: h.normal.clone(),
                    floor: h.floor.clone(),
                    cadence,
                    xp: h.xp.unwrap_or(5),
                    anchor: h.anchor,
                    start_date: h.start.unwrap_or(today),
                },
            )?;
        }
        for g in &seed.goal {
            upsert_goal(
                tx,
                &NewGoal {
                    id: Some(g.id.clone()),
                    pillar_id: g.pillar.clone(),
                    area_id: g.area.clone(),
                    name: g.name.clone(),
                    specs: g.specs.clone(),
                    why: g.why.clone(),
                    images: g.images.clone(),
                    target_paise: g.target_paise,
                    earned_paise: g.earned_paise,
                    progress: g.progress,
                    sort: g.sort,
                },
            )?;
        }
        Ok(SeedReport {
            pillars: seed.pillar.len(),
            areas: seed.area.len(),
            habits: seed.habit.len(),
            goals: seed.goal.len(),
        })
    })
}

/// The `[profile]` of the personal seed file, if the file and the section exist.
pub fn local_profile(path: &Path) -> Option<SeedProfile> {
    SeedFile::load(path).ok()?.profile
}

/// Apply the personal seed file if it exists. Returns `None` when there is no file.
pub fn apply_local(store: &Store, path: &Path) -> Result<Option<SeedReport>> {
    if !path.exists() {
        return Ok(None);
    }
    apply(store, "seed", &SeedFile::load(path)?).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_file_parses_and_applies() {
        let seed = SeedFile::from_toml(include_str!("../seed.example.toml")).unwrap();
        let store = Store::open_temporary().unwrap();
        let report = apply(&store, "test", &seed).unwrap();
        assert!(report.habits > 0);
        assert_eq!(store.list_goals(false).unwrap().len(), report.goals);
        let today = crate::time::local_today();
        let habits = store.list_habits(today).unwrap();
        assert!(habits[0].habit.anchor, "the anchor habit is listed first");
        // Applying twice is fine.
        apply(&store, "test", &seed).unwrap();
        assert_eq!(store.list_habits(today).unwrap().len(), habits.len());
    }

    #[test]
    fn profile_section_parses() {
        let seed = SeedFile::from_toml(
            "[profile]
name = 'Sam'
private_habits = ['x']
[[profile.hero]]
slot = 'money'
goal = 'laptop'
",
        )
        .unwrap();
        let p = seed.profile.unwrap();
        assert_eq!(p.name.as_deref(), Some("Sam"));
        assert_eq!(p.hero[0].goal.as_deref(), Some("laptop"));
        assert_eq!(p.private_habits, vec!["x".to_string()]);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(SeedFile::from_toml("[[pillar]]\nid='x'\nname='X'\ncolour='red'\n").is_err());
    }
}
