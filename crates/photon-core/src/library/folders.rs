use super::Library;
use crate::paths;
use crate::{Error, Result};
use rusqlite::{Row, params};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    pub id: i64,
    pub path: String,
    pub online: bool,
}

impl Library {
    /// Registers a folder to watch after validating it. The path is canonicalised, and adding
    /// an already watched folder returns the existing entry. A folder that contains or sits
    /// inside another watched folder is refused, as is anything equal to or inside
    /// photon's own directories in `excluded`.
    pub fn add_watched_folder(&self, path: &Path, excluded: &[PathBuf]) -> Result<WatchedFolder> {
        let canonical =
            paths::canonicalize(path).map_err(|_| Error::FolderNotFound(path.to_path_buf()))?;
        if !canonical.is_dir() {
            return Err(Error::FolderNotFound(path.to_path_buf()));
        }
        for ex in excluded {
            let ex = paths::canonicalize(ex).unwrap_or_else(|_| ex.clone());
            if paths::is_within(&canonical, &ex) {
                return Err(Error::FolderExcluded {
                    path: ex.display().to_string(),
                });
            }
        }
        for existing in self.watched_folders()? {
            let existing_path = Path::new(&existing.path);
            if paths::same_path(&canonical, existing_path) {
                return Ok(existing);
            }
            if paths::overlaps(&canonical, existing_path) {
                return Err(Error::FolderOverlap {
                    existing: existing.path,
                });
            }
        }
        let path_str = canonical
            .to_str()
            .ok_or_else(|| Error::NonUtf8Path(canonical.clone()))?;
        self.register_watched_folder(path_str)
    }

    /// Inserts a watched-folder row as given, without validation. Used by
    /// `add_watched_folder` and by tests that work with synthetic paths.
    pub(crate) fn register_watched_folder(&self, path: &str) -> Result<WatchedFolder> {
        let conn = self.writer();
        conn.execute(
            "INSERT OR IGNORE INTO watched_folders (path) VALUES (?1)",
            params![path],
        )?;
        let watched = conn.query_row(
            "SELECT id, path, online FROM watched_folders WHERE path = ?1",
            params![path],
            row_to_watched,
        )?;
        Ok(watched)
    }

    pub fn watched_folders(&self) -> Result<Vec<WatchedFolder>> {
        let conn = self.reader()?;
        let mut stmt =
            conn.prepare("SELECT id, path, online FROM watched_folders ORDER BY path")?;
        let rows = stmt
            .query_map([], row_to_watched)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Photos per watched folder, keyed by watched id. A root with no photos is absent.
    ///
    /// Soft-deleted items are left out so the figure agrees with the All view; an offline
    /// root's items are not soft-deleted, so its count survives the drive going away.
    pub fn watched_photo_counts(&self) -> Result<Vec<(i64, i64)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT f.watched_id, count(*) FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.missing_since IS NULL
             GROUP BY f.watched_id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
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
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        // The cascade takes every item under it, and their thumbnails with no item are
        // garbage the next collection has to know to look for.
        super::settings::bump_thumb_gc_epoch(&tx)?;
        tx.execute("DELETE FROM watched_folders WHERE id = ?1", params![id])?;
        tx.commit()?;
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
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub watched_id: i64,
    pub parent_id: Option<i64>,
    pub path: String,
    pub name: String,
    /// Whether the user hid the folder: its photos are hidden, and so is any photo added to
    /// it later (`Library::set_folder_hidden`).
    pub hidden: bool,
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
        delete_until_stable(|| {
            conn.execute(
                "DELETE FROM folders
                 WHERE watched_id = ?1 AND seen_scan < ?2
                   AND NOT EXISTS (SELECT 1 FROM items WHERE items.folder_id = folders.id)
                   AND NOT EXISTS (SELECT 1 FROM folders c WHERE c.parent_id = folders.id)",
                params![watched_id, scan_id],
            )
        })
    }

    /// `prune_folders`, restricted to `dir` and everything beneath it. A subtree scan must
    /// never prune folders it did not walk.
    pub fn prune_folders_under(&self, watched_id: i64, scan_id: i64, dir: &str) -> Result<usize> {
        let conn = self.writer();
        delete_until_stable(|| {
            conn.execute(
                "WITH RECURSIVE sub(id) AS (
                     SELECT id FROM folders WHERE watched_id = ?1 AND path = ?3
                     UNION ALL
                     SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id
                 )
                 DELETE FROM folders
                 WHERE id IN (SELECT id FROM sub) AND seen_scan < ?2
                   AND NOT EXISTS (SELECT 1 FROM items WHERE items.folder_id = folders.id)
                   AND NOT EXISTS (SELECT 1 FROM folders c WHERE c.parent_id = folders.id)",
                params![watched_id, scan_id, dir],
            )
        })
    }

    /// All folders in tree order (parents before children, siblings alphabetical; `path`
    /// breaks ties between names that differ only in case).
    pub fn folders(&self) -> Result<Vec<Folder>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, watched_id, parent_id, path, name, hidden FROM folders ORDER BY sort_key, path",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Folder {
                    id: r.get(0)?,
                    watched_id: r.get(1)?,
                    parent_id: r.get(2)?,
                    path: r.get(3)?,
                    name: r.get(4)?,
                    hidden: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// Runs `delete` until it removes nothing, returning the total.
///
/// Both prunes need the repeat and for the same reason: the statement only deletes a folder
/// with no children, so emptying a leaf is what makes its parent deletable on the next pass.
/// One `DELETE` cannot express that, and one pass would leave a deleted tree's upper levels
/// behind until some later scan happened to run enough times.
fn delete_until_stable(mut delete: impl FnMut() -> rusqlite::Result<usize>) -> Result<usize> {
    let mut total = 0;
    loop {
        let removed = delete()?;
        if removed == 0 {
            return Ok(total);
        }
        total += removed;
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
    use crate::Error;
    use crate::testutil::{new_item, temp_library, watch};
    use std::path::PathBuf;

    /// Creates each directory under `root` and returns its canonical path.
    fn dirs(root: &Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| {
                let path = name
                    .split('/')
                    .fold(root.to_path_buf(), |p, part| p.join(part));
                std::fs::create_dir_all(&path).unwrap();
                paths::canonicalize(path).unwrap()
            })
            .collect()
    }

    #[test]
    fn add_watched_folder_canonicalises_and_dedupes() {
        let (dir, lib) = temp_library();
        let d = dirs(dir.path(), &["photos"]);
        let a = lib
            .add_watched_folder(&dir.path().join("photos").join("."), &[])
            .unwrap();
        assert_eq!(Path::new(&a.path), d[0].as_path());
        assert_eq!(lib.add_watched_folder(&d[0], &[]).unwrap(), a);
        assert_eq!(lib.watched_folders().unwrap().len(), 1);
    }

    #[test]
    fn add_watched_folder_rejects_missing_and_overlapping_folders() {
        let (dir, lib) = temp_library();
        let d = dirs(dir.path(), &["photos/2024", "other"]);
        let photos = d[0].parent().unwrap().to_path_buf();
        lib.add_watched_folder(&photos, &[]).unwrap();

        assert!(matches!(
            lib.add_watched_folder(&d[0], &[]),
            Err(Error::FolderOverlap { .. })
        ));
        assert!(matches!(
            lib.add_watched_folder(dir.path(), &[]),
            Err(Error::FolderOverlap { .. })
        ));
        assert!(lib.add_watched_folder(&d[1], &[]).is_ok());
        assert!(matches!(
            lib.add_watched_folder(&dir.path().join("missing"), &[]),
            Err(Error::FolderNotFound(_))
        ));
    }

    #[test]
    fn add_watched_folder_rejects_photons_own_directories() {
        let (dir, lib) = temp_library();
        let d = dirs(dir.path(), &["cache/thumbs"]);
        let cache = d[0].parent().unwrap().to_path_buf();
        let excluded = [cache.clone()];
        assert!(matches!(
            lib.add_watched_folder(&cache, &excluded),
            Err(Error::FolderExcluded { .. })
        ));
        assert!(matches!(
            lib.add_watched_folder(&d[0], &excluded),
            Err(Error::FolderExcluded { .. })
        ));
        // A folder containing an excluded one is fine: the scanner skips the excluded part.
        assert!(lib.add_watched_folder(dir.path(), &excluded).is_ok());
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn duplicate_detection_ignores_case() {
        let (dir, lib) = temp_library();
        let d = dirs(dir.path(), &["Photos"]);
        let a = lib.add_watched_folder(&d[0], &[]).unwrap();
        let again = lib
            .add_watched_folder(&dir.path().join("PHOTOS"), &[])
            .unwrap();
        assert_eq!(again.id, a.id);
    }

    #[test]
    fn photo_counts_are_per_watched_root_and_skip_missing_items() {
        let (_dir, lib) = temp_library();
        let a = watch(&lib, "/a");
        let b = watch(&lib, "/b");
        watch(&lib, "/empty");
        let a_root = lib.upsert_folder(a.id, None, "/a", 1).unwrap();
        let a_sub = lib.upsert_folder(a.id, Some(a_root), "/a/sub", 1).unwrap();
        let b_root = lib.upsert_folder(b.id, None, "/b", 1).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(a_root, "/a/1.jpg", 0),
                new_item(a_sub, "/a/sub/2.jpg", 0),
                new_item(b_root, "/b/3.jpg", 0),
                new_item(b_root, "/b/gone.jpg", 0),
            ])
            .unwrap();
        lib.mark_missing(&ids[3..], 1).unwrap();

        let mut counts = lib.watched_photo_counts().unwrap();
        counts.sort();
        assert_eq!(counts, [(a.id, 2), (b.id, 1)]);
    }

    #[test]
    fn folders_serialise_as_camel_case() {
        let folder = Folder {
            id: 1,
            watched_id: 2,
            parent_id: None,
            path: "p".into(),
            name: "p".into(),
            hidden: false,
        };
        let json = serde_json::to_string(&folder).unwrap();
        assert!(json.contains("\"watchedId\":2") && json.contains("\"parentId\":null"));
    }

    #[test]
    fn upsert_folder_is_idempotent_and_tracks_parent() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/photos");
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
                hidden: false,
            }
        );
    }

    #[test]
    fn folders_are_listed_in_tree_order() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/a b", 1).unwrap();
        let a = lib.upsert_folder(w.id, Some(root), "/p/a", 1).unwrap();
        lib.upsert_folder(w.id, Some(a), "/p/a/z", 1).unwrap();

        let names: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["p", "a", "z", "a b"]);
    }

    #[test]
    fn folders_have_a_parent_index() {
        let (_dir, lib) = temp_library();
        let found: i64 = lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'folders_parent'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, 1);
    }

    #[test]
    fn prune_removes_only_unseen_empty_leaf_folders() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
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

    #[test]
    fn prune_folders_under_stays_inside_the_subtree() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        let a = lib.upsert_folder(w.id, Some(root), "/p/a", 1).unwrap();
        lib.upsert_folder(w.id, Some(a), "/p/a/gone", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/b", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/c-stale", 1)
            .unwrap();

        // A later scan of /p/a only saw /p/a itself.
        lib.upsert_folder(w.id, Some(root), "/p/a", 2).unwrap();
        assert_eq!(lib.prune_folders_under(w.id, 2, "/p/a").unwrap(), 1);

        let paths: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["/p", "/p/a", "/p/b", "/p/c-stale"]);
    }

    #[test]
    fn prune_folders_under_removes_an_empty_chain() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        let a = lib.upsert_folder(w.id, Some(root), "/p/a", 1).unwrap();
        let mid = lib.upsert_folder(w.id, Some(a), "/p/a/mid", 1).unwrap();
        lib.upsert_folder(w.id, Some(mid), "/p/a/mid/leaf", 1)
            .unwrap();

        lib.upsert_folder(w.id, Some(root), "/p/a", 2).unwrap();
        assert_eq!(lib.prune_folders_under(w.id, 2, "/p/a").unwrap(), 2);
        let paths: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["/p", "/p/a"]);
    }
}
