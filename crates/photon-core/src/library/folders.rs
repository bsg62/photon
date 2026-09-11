use super::Library;
use crate::{Error, Result};
use rusqlite::{Row, params};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WatchedFolder {
    pub id: i64,
    pub path: String,
    pub online: bool,
}

impl Library {
    /// Registers a folder to watch. Adding the same path twice returns the existing entry.
    pub fn add_watched_folder(&self, path: &Path) -> Result<WatchedFolder> {
        let path_str = path
            .to_str()
            .ok_or_else(|| Error::NonUtf8Path(path.to_path_buf()))?;
        let conn = self.writer();
        conn.execute(
            "INSERT OR IGNORE INTO watched_folders (path) VALUES (?1)",
            params![path_str],
        )?;
        let watched = conn.query_row(
            "SELECT id, path, online FROM watched_folders WHERE path = ?1",
            params![path_str],
            row_to_watched,
        )?;
        Ok(watched)
    }

    pub fn watched_folders(&self) -> Result<Vec<WatchedFolder>> {
        let conn = self.reader();
        let mut stmt =
            conn.prepare("SELECT id, path, online FROM watched_folders ORDER BY path")?;
        let rows = stmt
            .query_map([], row_to_watched)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_watched_online(&self, id: i64, online: bool) -> Result<()> {
        self.writer().execute(
            "UPDATE watched_folders SET online = ?2 WHERE id = ?1",
            params![id, online],
        )?;
        Ok(())
    }

    /// Forgets a watched folder and everything indexed under it. Files on disk are untouched.
    pub fn remove_watched_folder(&self, id: i64) -> Result<()> {
        self.writer()
            .execute("DELETE FROM watched_folders WHERE id = ?1", params![id])?;
        Ok(())
    }
}

fn row_to_watched(row: &Row<'_>) -> rusqlite::Result<WatchedFolder> {
    Ok(WatchedFolder {
        id: row.get(0)?,
        path: row.get(1)?,
        online: row.get(2)?,
    })
}
