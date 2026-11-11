//! Read T3 Code's project list (read only). T3 keeps its state in SQLite under `~/.t3/userdata/`:
//! `statev2.sqlite` (orchestration v2) or the older `state.sqlite`, table `projection_projects`.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct T3Project {
    pub id: String,
    pub title: String,
    pub folder: String,
}

pub fn read_projects(path: &Path) -> Result<Vec<T3Project>> {
    // Read only, and never create the file. T3 may be running (WAL), which is fine for readers.
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    let v2 = "SELECT project_id, title, workspace_root FROM projection_projects
              WHERE deleted_at IS NULL AND archived_at IS NULL ORDER BY updated_at DESC";
    let v1 = "SELECT project_id, title, workspace_root FROM projection_projects
              WHERE deleted_at IS NULL ORDER BY updated_at DESC";
    let mut st = match conn.prepare(v2) {
        Ok(st) => st,
        Err(_) => conn.prepare(v1)?,
    };
    let rows = st
        .query_map([], |r| Ok(T3Project { id: r.get(0)?, title: r.get(1)?, folder: r.get(2)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_v1_shape() {
        let dir = std::env::temp_dir().join(format!("aos-t3-{}", crate::store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("state.sqlite");
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(
                "CREATE TABLE projection_projects (project_id TEXT, title TEXT, workspace_root TEXT,
                   created_at TEXT, updated_at TEXT, deleted_at TEXT);
                 INSERT INTO projection_projects VALUES ('p1', 'One', '/a', '1', '2', NULL);
                 INSERT INTO projection_projects VALUES ('p2', 'Gone', '/b', '1', '1', 'yes');",
            )
            .unwrap();
        }
        let ps = read_projects(&db).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].title, "One");
        let _ = std::fs::remove_dir_all(dir);
    }
}
