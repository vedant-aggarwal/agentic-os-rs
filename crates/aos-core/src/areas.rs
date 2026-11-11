//! Pillars, areas and projects. Everything (tasks, habits, XP) belongs to an area; areas sit
//! under one of the pillars.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::error::{Result, not_found};
use crate::model::{Area, Pillar, Project};
use crate::store::{ChangeOp, Store, Tx};

/// A pillar with its areas, for listings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PillarTree {
    #[serde(flatten)]
    pub pillar: Pillar,
    pub areas: Vec<Area>,
}

/// All areas grouped by pillar, plus areas not under any pillar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaTree {
    pub pillars: Vec<PillarTree>,
    pub unfiled: Vec<Area>,
}

pub(crate) const AREA_COLS: &str = "id, pillar_id, name, color, sort, archived";

pub(crate) fn area_from_row(r: &Row<'_>) -> rusqlite::Result<Area> {
    Ok(Area {
        id: r.get("id")?,
        pillar_id: r.get("pillar_id")?,
        name: r.get("name")?,
        color: r.get("color")?,
        sort: r.get("sort")?,
        archived: r.get("archived")?,
    })
}

impl Store {
    pub fn list_pillars(&self) -> Result<Vec<Pillar>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT id, name, sort FROM pillar ORDER BY sort, name")?;
            let rows = stmt.query_map([], |r| {
                Ok(Pillar {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    sort: r.get(2)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn list_areas(&self, include_archived: bool) -> Result<Vec<Area>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {AREA_COLS} FROM area WHERE archived = 0 OR ?1 ORDER BY sort, name"
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map([include_archived], area_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn area_tree(&self) -> Result<AreaTree> {
        let areas = self.list_areas(false)?;
        let pillars = self
            .list_pillars()?
            .into_iter()
            .map(|p| PillarTree {
                areas: areas
                    .iter()
                    .filter(|a| a.pillar_id.as_deref() == Some(&p.id))
                    .cloned()
                    .collect(),
                pillar: p,
            })
            .collect::<Vec<_>>();
        let unfiled = areas
            .into_iter()
            .filter(|a| {
                !pillars
                    .iter()
                    .any(|p| a.pillar_id.as_deref() == Some(&p.pillar.id))
            })
            .collect();
        Ok(AreaTree { pillars, unfiled })
    }

    /// Find an area by id or by name (case-insensitive).
    pub fn resolve_area(&self, key: &str) -> Result<Area> {
        self.read(|c| resolve_area(c, key))
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        self.read(|c| {
            let mut stmt =
                c.prepare("SELECT id, area_id, name, status FROM project ORDER BY name")?;
            let rows = stmt.query_map([], project_from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Find a project by id or name (case-insensitive).
    pub fn resolve_project(&self, key: &str) -> Result<Project> {
        self.read(|c| {
            c.query_row(
                "SELECT id, area_id, name, status FROM project
                 WHERE id = ?1 OR lower(name) = lower(?1) LIMIT 1",
                [key.trim()],
                project_from_row,
            )
            .optional()?
            .ok_or_else(|| not_found("project", key))
        })
    }
}

fn project_from_row(r: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: r.get(0)?,
        area_id: r.get(1)?,
        name: r.get(2)?,
        status: r.get(3)?,
    })
}

pub(crate) fn resolve_area(c: &Connection, key: &str) -> Result<Area> {
    let sql = format!(
        "SELECT {AREA_COLS} FROM area WHERE id = ?1 OR lower(name) = lower(?1)
         ORDER BY id = ?1 DESC LIMIT 1"
    );
    c.query_row(&sql, [key.trim()], area_from_row)
        .optional()?
        .ok_or_else(|| not_found("area", key))
}

pub(crate) fn upsert_pillar(tx: &Tx<'_>, id: &str, name: &str, sort: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO pillar (id, name, sort, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, sort = excluded.sort,
             updated_at = excluded.updated_at",
        params![id, name, sort, tx.now()],
    )?;
    tx.record("pillar", id, ChangeOp::Upsert)
}

pub(crate) fn upsert_area(tx: &Tx<'_>, area: &Area) -> Result<()> {
    tx.execute(
        "INSERT INTO area (id, pillar_id, name, color, sort, archived, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(id) DO UPDATE SET pillar_id = COALESCE(excluded.pillar_id, area.pillar_id),
             name = excluded.name, color = COALESCE(excluded.color, area.color),
             sort = excluded.sort, archived = excluded.archived, updated_at = excluded.updated_at",
        params![
            area.id,
            area.pillar_id,
            area.name,
            area.color,
            area.sort,
            area.archived,
            tx.now()
        ],
    )?;
    tx.record("area", &area.id, ChangeOp::Upsert)
}

pub(crate) fn upsert_project(tx: &Tx<'_>, p: &Project) -> Result<()> {
    tx.execute(
        "INSERT INTO project (id, area_id, name, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(id) DO UPDATE SET area_id = excluded.area_id, name = excluded.name,
             status = excluded.status, updated_at = excluded.updated_at",
        params![p.id, p.area_id, p.name, p.status, tx.now()],
    )?;
    tx.record("project", &p.id, ChangeOp::Upsert)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_seed_tree() {
        let store = Store::open_temporary().unwrap();
        let tree = store.area_tree().unwrap();
        assert_eq!(tree.pillars.len(), 3);
        assert!(tree.pillars.iter().all(|p| p.areas.len() == 3));
        assert!(tree.unfiled.is_empty());
    }

    #[test]
    fn resolve_by_id_or_name() {
        let store = Store::open_temporary().unwrap();
        assert_eq!(store.resolve_area("fitness").unwrap().id, "fitness");
        assert_eq!(store.resolve_area("READING").unwrap().id, "reading");
        assert!(matches!(
            store.resolve_area("nope"),
            Err(crate::Error::NotFound { .. })
        ));
    }
}
