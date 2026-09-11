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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Folder {
    pub id: i64,
    pub watched_id: i64,
    pub parent_id: Option<i64>,
    pub path: String,
    pub name: String,
}

impl Library {
    /// Inserts a folder, or refreshes its parent and scan marker if it already exists.
    pub fn upsert_folder(
        &self,
        watched_id: i64,
        parent_id: Option<i64>,
        path: &str,
        scan_id: i64,
    ) -> Result<i64> {
        let name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();
        let id = self.writer().query_row(
            "INSERT INTO folders (watched_id, parent_id, path, name, sort_key, seen_scan)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET parent_id = excluded.parent_id, seen_scan = excluded.seen_scan
             RETURNING id",
            params![watched_id, parent_id, path, name, sort_key(path), scan_id],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Deletes folders not seen in scan `scan_id` that hold no items and no subfolders.
    /// Folders still holding soft-deleted items survive until those items are purged.
    pub fn prune_folders(&self, watched_id: i64, scan_id: i64) -> Result<usize> {
        let conn = self.writer();
        let mut total = 0;
        loop {
            let removed = conn.execute(
                "DELETE FROM folders
                 WHERE watched_id = ?1 AND seen_scan < ?2
                   AND NOT EXISTS (SELECT 1 FROM items WHERE items.folder_id = folders.id)
                   AND NOT EXISTS (SELECT 1 FROM folders c WHERE c.parent_id = folders.id)",
                params![watched_id, scan_id],
            )?;
            if removed == 0 {
                return Ok(total);
            }
            total += removed;
        }
    }

    /// All folders in tree order (parents before children, siblings alphabetical).
    pub fn folders(&self) -> Result<Vec<Folder>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(
            "SELECT id, watched_id, parent_id, path, name FROM folders ORDER BY sort_key",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Folder {
                    id: r.get(0)?,
                    watched_id: r.get(1)?,
                    parent_id: r.get(2)?,
                    path: r.get(3)?,
                    name: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// Case-insensitive key whose byte order is depth-first tree order: separators map to
/// \u{1}, which sorts below every printable character, so "/p/a/z" < "/p/a b".
pub(crate) fn sort_key(path: &str) -> String {
    path.to_lowercase().replace(['/', '\\'], "\u{1}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{new_item, temp_library};

    #[test]
    fn upsert_folder_is_idempotent_and_tracks_parent() {
        let (_dir, lib) = temp_library();
        let w = lib.add_watched_folder(Path::new("/photos")).unwrap();
        let root = lib.upsert_folder(w.id, None, "/photos", 1).unwrap();
        let child = lib
            .upsert_folder(w.id, Some(root), "/photos/2024", 1)
            .unwrap();
        assert_eq!(
            lib.upsert_folder(w.id, Some(root), "/photos/2024", 2)
                .unwrap(),
            child
        );

        let folders = lib.folders().unwrap();
        assert_eq!(folders.len(), 2);
        assert_eq!(
            folders[1],
            Folder {
                id: child,
                watched_id: w.id,
                parent_id: Some(root),
                path: "/photos/2024".into(),
                name: "2024".into(),
            }
        );
    }

    #[test]
    fn folders_are_listed_in_tree_order() {
        let (_dir, lib) = temp_library();
        let w = lib.add_watched_folder(Path::new("/p")).unwrap();
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/a b", 1).unwrap();
        let a = lib.upsert_folder(w.id, Some(root), "/p/a", 1).unwrap();
        lib.upsert_folder(w.id, Some(a), "/p/a/z", 1).unwrap();

        let names: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["p", "a", "z", "a b"]);
    }

    #[test]
    fn prune_removes_only_unseen_empty_leaf_folders() {
        let (_dir, lib) = temp_library();
        let w = lib.add_watched_folder(Path::new("/p")).unwrap();
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/gone", 1).unwrap();
        let kept = lib.upsert_folder(w.id, Some(root), "/p/kept", 1).unwrap();
        let parent = lib.upsert_folder(w.id, Some(root), "/p/parent", 1).unwrap();
        let child = lib
            .upsert_folder(w.id, Some(parent), "/p/parent/child", 1)
            .unwrap();
        lib.insert_items(&[
            new_item(kept, "/p/kept/a.jpg", 0),
            new_item(child, "/p/parent/child/b.jpg", 0),
        ])
        .unwrap();

        // Second scan only saw the root.
        lib.upsert_folder(w.id, None, "/p", 2).unwrap();
        assert_eq!(lib.prune_folders(w.id, 2).unwrap(), 1);

        let paths: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["/p", "/p/kept", "/p/parent", "/p/parent/child"]);
    }
}
