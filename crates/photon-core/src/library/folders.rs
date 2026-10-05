use super::Library;
use crate::paths;
use crate::{Error, Result};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    pub id: i64,
    pub path: String,
    pub online: bool,
}

/// `watched_photo_counts`' query, shared with its plan test. The `+` keeps the planner from
/// walking `items_size` to find the live photos; see `library/mod.rs`.
const WATCHED_PHOTO_COUNTS_SQL: &str =
    "SELECT f.watched_id, count(*) FROM items i JOIN folders f ON f.id = i.folder_id
     WHERE +i.missing_since IS NULL
     GROUP BY f.watched_id";

impl Library {
    /// Registers a folder to watch after validating it. The path is canonicalised, and adding
    /// an already watched folder returns the existing entry. A folder that contains or sits
    /// inside another watched folder is refused, as is anything equal to or inside
    /// photon's own directories in `excluded`.
    pub fn add_watched_folder(&self, path: &Path, excluded: &[PathBuf]) -> Result<WatchedFolder> {
        let canonical =
            paths::canonicalize(path).map_err(|_| Error::FolderNotFound(path.to_path_buf()))?;
        if !canonical.is_dir() {
            return Err(Error::NotAFolder(path.to_path_buf()));
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
        let mut stmt = conn.prepare(WATCHED_PHOTO_COUNTS_SQL)?;
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
    /// The user's name for the folder in photon, shown in place of `name`; `None` for none.
    pub alias: Option<String>,
}

/// The longest alias kept, in characters: a folder name, not a caption.
pub const MAX_FOLDER_ALIAS_CHARS: usize = 255;

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
        let conn = self.writer();
        let mut stmt = conn.prepare_cached(
            "INSERT INTO folders (watched_id, parent_id, path, name, sort_key, seen_scan)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET parent_id = excluded.parent_id, seen_scan = excluded.seen_scan
             RETURNING id",
        )?;
        let id = stmt.query_row(
            params![watched_id, parent_id, path, name, sort_key(path), scan_id],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Every folder row under one watched folder, as `path -> (id, parent_id)`: what a scan
    /// compares its walk against, so it writes only the folders that are new or moved.
    pub fn folder_rows(&self, watched_id: i64) -> Result<HashMap<String, (i64, Option<i64>)>> {
        let conn = self.reader()?;
        let mut stmt =
            conn.prepare_cached("SELECT path, id, parent_id FROM folders WHERE watched_id = ?1")?;
        let rows = stmt
            .query_map(params![watched_id], |r| {
                Ok((r.get(0)?, (r.get(1)?, r.get(2)?)))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Marks folders as seen by scan `scan_id`, which is what keeps `prune_folders` off them,
    /// in one transaction. The caller chunks.
    pub fn mark_folders_seen(&self, ids: &[i64], scan_id: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE folders SET seen_scan = ?2 WHERE id = ?1")?;
            for id in ids {
                stmt.execute(params![id, scan_id])?;
            }
        }
        tx.commit()?;
        Ok(())
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
            "SELECT id, watched_id, parent_id, path, name, hidden, alias FROM folders ORDER BY sort_key, path",
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
                    alias: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Names a folder in photon, or clears the name with `None`. The input is trimmed and
    /// cut to `MAX_FOLDER_ALIAS_CHARS` on a character boundary. An empty result, or one equal
    /// to the directory's own name, is stored as NULL: "renaming it back" must not leave an
    /// alias that merely repeats the name, which would go on matching search as a second
    /// copy and outlive a later rename of the directory's row. Returns whether the stored
    /// value changed, so the engine rebuilds only when something did; `NotFound` for a
    /// folder that does not exist.
    pub fn set_folder_alias(&self, folder_id: i64, alias: Option<&str>) -> Result<bool> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let (name, current): (String, Option<String>) = tx
            .query_row(
                "SELECT name, alias FROM folders WHERE id = ?1",
                params![folder_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound(folder_id))?;
        // Trimmed again after the cut, which can land just after a space.
        let alias = alias
            .map(|a| {
                let cut: String = a.trim().chars().take(MAX_FOLDER_ALIAS_CHARS).collect();
                cut.trim_end().to_string()
            })
            .filter(|a| !a.is_empty() && *a != name);
        if alias == current {
            return Ok(false);
        }
        tx.execute(
            "UPDATE folders SET alias = ?2 WHERE id = ?1",
            params![folder_id, alias],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Gives folder `to` the alias and Hide-folder flag of `from`, when `to` has neither: a
    /// renamed directory is a new folder row, and what the user set on the old one follows
    /// the photos that followed it. A folder with a name or a flag of its own keeps them.
    /// The flag goes through `set_folder_hidden`, so photos of `to` indexed before the move
    /// was noticed are hidden with it. Returns whether anything was written.
    ///
    /// A folder that has no row any more gives nothing and takes nothing, and that is
    /// `Ok(false)`, not `NotFound`: the scanner calls this after it has re-pointed photos out
    /// of `from`, which leaves `from` empty, and a scan of another watched folder may prune
    /// an empty folder at any moment. Answered as an error it failed a scan over a name
    /// that was already past saving.
    pub fn inherit_folder_flags(&self, from: i64, to: i64) -> Result<bool> {
        let (alias, hidden, to_alias, to_hidden) = {
            let conn = self.reader()?;
            let read = |id: i64| -> Result<Option<(Option<String>, bool)>> {
                Ok(conn
                    .query_row(
                        "SELECT alias, hidden FROM folders WHERE id = ?1",
                        params![id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?)
            };
            let (Some((alias, hidden)), Some((to_alias, to_hidden))) = (read(from)?, read(to)?)
            else {
                return Ok(false);
            };
            (alias, hidden, to_alias, to_hidden)
        };
        if to_alias.is_some() || to_hidden || (alias.is_none() && !hidden) {
            return Ok(false);
        }
        if alias.is_some() {
            // Validated when it was set on `from`.
            self.writer().execute(
                "UPDATE folders SET alias = ?2 WHERE id = ?1",
                params![to, alias],
            )?;
        }
        if hidden {
            self.set_folder_hidden(to, true)?;
        }
        Ok(true)
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
        // A file is there, so it is not "not found": what a photo dropped on the window is.
        let file = dir.path().join("photo.jpg");
        std::fs::write(&file, b"x").unwrap();
        let refused = lib.add_watched_folder(&file, &[]).unwrap_err();
        assert!(matches!(refused, Error::NotAFolder(_)));
        // The path as the user knows it: not Debug's quotes and doubled backslashes.
        assert_eq!(
            refused.to_string(),
            format!(
                "{} is a file, not a folder: add the folder it is in",
                file.display()
            )
        );
        assert!(
            lib.watched_folders()
                .unwrap()
                .iter()
                .all(|w| !w.path.ends_with("photo.jpg"))
        );
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
            alias: None,
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
                alias: None,
            }
        );
    }

    fn alias_of(lib: &Library, folder_id: i64) -> Option<String> {
        let folders = lib.folders().unwrap();
        folders
            .into_iter()
            .find(|f| f.id == folder_id)
            .unwrap()
            .alias
    }

    #[test]
    fn a_folder_alias_is_trimmed_and_cleared_by_empty_or_the_real_name() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
        let folder = lib.upsert_folder(w.id, None, "/p/dcim-0412", 1).unwrap();

        assert!(lib.set_folder_alias(folder, Some("  Easter  ")).unwrap());
        assert_eq!(alias_of(&lib, folder).as_deref(), Some("Easter"));
        assert!(
            !lib.set_folder_alias(folder, Some("Easter")).unwrap(),
            "the same alias again reported a change"
        );

        assert!(lib.set_folder_alias(folder, Some("   ")).unwrap());
        assert_eq!(alias_of(&lib, folder), None, "a blank alias was kept");

        lib.set_folder_alias(folder, Some("Easter")).unwrap();
        assert!(lib.set_folder_alias(folder, Some("dcim-0412")).unwrap());
        assert_eq!(
            alias_of(&lib, folder),
            None,
            "an alias equal to the directory name was kept"
        );

        lib.set_folder_alias(folder, Some("Easter")).unwrap();
        assert!(lib.set_folder_alias(folder, None).unwrap());
        assert_eq!(alias_of(&lib, folder), None);

        assert!(matches!(
            lib.set_folder_alias(999, Some("x")),
            Err(Error::NotFound(999))
        ));
    }

    #[test]
    fn a_long_folder_alias_is_cut_on_a_character_boundary() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
        let folder = lib.upsert_folder(w.id, None, "/p/a", 1).unwrap();
        lib.set_folder_alias(folder, Some(&"é".repeat(300)))
            .unwrap();
        assert_eq!(
            alias_of(&lib, folder),
            Some("é".repeat(MAX_FOLDER_ALIAS_CHARS))
        );

        // A cut landing just after a space leaves no trailing space behind.
        let spaced = format!("{} tail", "a".repeat(MAX_FOLDER_ALIAS_CHARS - 1));
        lib.set_folder_alias(folder, Some(&spaced)).unwrap();
        assert_eq!(
            alias_of(&lib, folder),
            Some("a".repeat(MAX_FOLDER_ALIAS_CHARS - 1))
        );
    }

    #[test]
    fn an_alias_survives_upsert_folder() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
        let folder = lib.upsert_folder(w.id, None, "/p/a", 1).unwrap();
        lib.set_folder_alias(folder, Some("Easter")).unwrap();
        assert_eq!(lib.upsert_folder(w.id, None, "/p/a", 2).unwrap(), folder);
        assert_eq!(alias_of(&lib, folder).as_deref(), Some("Easter"));
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

    /// Pins the `+` in `WATCHED_PHOTO_COUNTS_SQL`: without it the planner walks
    /// `items_size` to find the live photos (`library/mod.rs`). Reaching them folder by
    /// folder through `items_folder`, or scanning the table, are both fine; walking a
    /// partial index end to end is what the `+` is there to stop.
    #[test]
    fn watched_photo_counts_do_not_walk_the_size_index() {
        let (_dir, lib) = temp_library();
        let plan = lib.query_plan(WATCHED_PHOTO_COUNTS_SQL, &[]);
        assert!(
            !plan.iter().any(|step| step.contains("items_size")),
            "walks the size index: {plan:?}"
        );
        assert!(
            !plan.iter().any(|step| step.starts_with("SCAN i USING")),
            "photos must not be read in an index's order: {plan:?}"
        );
    }

    #[test]
    fn a_folder_inherits_an_alias_and_the_hide_flag_only_when_it_has_neither() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/photos");
        let from = lib.upsert_folder(w.id, None, "/photos/old", 1).unwrap();
        let to = lib.upsert_folder(w.id, None, "/photos/new", 1).unwrap();
        let named = lib.upsert_folder(w.id, None, "/photos/named", 1).unwrap();
        lib.set_folder_alias(from, Some("Holiday")).unwrap();
        lib.set_folder_hidden(from, true).unwrap();
        lib.set_folder_alias(named, Some("Mine")).unwrap();
        let item = lib
            .insert_items(&[new_item(to, "/photos/new/a.jpg", 1)])
            .unwrap()[0];

        assert!(lib.inherit_folder_flags(from, to).unwrap());
        let folder = |id| {
            lib.folders()
                .unwrap()
                .into_iter()
                .find(|f| f.id == id)
                .unwrap()
        };
        assert_eq!(folder(to).alias.as_deref(), Some("Holiday"));
        assert!(folder(to).hidden);
        assert!(lib.item(item).unwrap().unwrap().hidden);
        assert!(
            !lib.inherit_folder_flags(from, to).unwrap(),
            "now it has both"
        );

        assert!(!lib.inherit_folder_flags(from, named).unwrap());
        assert_eq!(folder(named).alias.as_deref(), Some("Mine"));
        assert!(!folder(named).hidden);
    }

    /// A scan of another watched folder can prune the folder a photo left before its name
    /// is handed on. That is a state, not an error: answered as one, it failed the scan that
    /// had just followed the photo.
    #[test]
    fn a_folder_row_that_is_gone_gives_and_takes_nothing() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/photos");
        let here = lib.upsert_folder(w.id, None, "/photos/here", 1).unwrap();
        let gone = here + 1000;

        let before = lib.folders().unwrap();
        assert!(!lib.inherit_folder_flags(gone, here).unwrap());
        assert_eq!(lib.folders().unwrap(), before);

        lib.set_folder_alias(here, Some("Holiday")).unwrap();
        lib.set_folder_hidden(here, true).unwrap();
        let before = lib.folders().unwrap();
        assert!(!lib.inherit_folder_flags(here, gone).unwrap());
        assert_eq!(lib.folders().unwrap(), before);
    }
}
