use super::Library;
use crate::Result;
use crate::grid::GridEntry;
use crate::media::{MediaKind, ThumbState, fingerprint};
use crate::metadata::oriented_dims;
use rusqlite::{OptionalExtension, Row, params};
use std::collections::{HashMap, HashSet};

/// A file discovered by the scanner, ready to be inserted or to replace an existing row.
#[derive(Clone, Debug, PartialEq)]
pub struct NewItem {
    pub folder_id: i64,
    pub path: String,
    pub file_name: String,
    pub kind: MediaKind,
    pub size: i64,
    pub mtime_ms: i64,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
    /// `None` when the file has not been read for a rating yet.
    pub rating: Option<u8>,
}

/// What the scanner needs to know about an indexed file to detect changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownItem {
    pub id: i64,
    pub size: i64,
    pub mtime_ms: i64,
    pub missing: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: i64,
    pub folder_id: i64,
    pub path: String,
    pub kind: MediaKind,
    pub size: i64,
    pub mtime_ms: i64,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
    pub thumb_state: ThumbState,
    pub thumb_error: Option<String>,
    pub missing_since: Option<i64>,
}

impl Item {
    pub fn fingerprint(&self) -> u64 {
        fingerprint(&self.path, self.size, self.mtime_ms)
    }
}

/// Grid order, shared by every query that walks items the way the grid shows them.
/// `f.path` breaks sort_key ties (e.g. `/p/A` vs `/p/a` on a case-sensitive filesystem)
/// so each folder's items stay contiguous.
pub(crate) const GRID_ORDER: &str = "ORDER BY f.sort_key, f.path, i.taken_at, i.file_name";

fn row_to_item(r: &Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        path: r.get(2)?,
        kind: MediaKind::from_db(r.get(3)?).unwrap_or(MediaKind::Image),
        size: r.get(4)?,
        mtime_ms: r.get(5)?,
        width: r.get(6)?,
        height: r.get(7)?,
        orientation: r.get(8)?,
        taken_at: r.get(9)?,
        thumb_state: ThumbState::from_db(r.get(10)?),
        thumb_error: r.get(11)?,
        missing_since: r.get(12)?,
    })
}

impl Library {
    pub fn insert_items(&self, items: &[NewItem]) -> Result<Vec<i64>> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let mut ids = Vec::with_capacity(items.len());
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at, rating)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for it in items {
                stmt.execute(params![
                    it.folder_id,
                    it.path,
                    it.file_name,
                    it.kind.to_db(),
                    it.size,
                    it.mtime_ms,
                    it.width,
                    it.height,
                    it.orientation,
                    it.taken_at,
                    it.rating
                ])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Replaces changed (or reappeared) items. Their thumbnails must be rebuilt.
    pub fn update_items(&self, items: &[(i64, NewItem)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE items SET folder_id = ?2, path = ?3, file_name = ?4, kind = ?5, size = ?6, mtime_ms = ?7,
                        width = ?8, height = ?9, orientation = ?10, taken_at = ?11, rating = ?12,
                        thumb_state = 0, thumb_error = NULL, missing_since = NULL
                 WHERE id = ?1",
            )?;
            for (id, it) in items {
                stmt.execute(params![
                    id,
                    it.folder_id,
                    it.path,
                    it.file_name,
                    it.kind.to_db(),
                    it.size,
                    it.mtime_ms,
                    it.width,
                    it.height,
                    it.orientation,
                    it.taken_at,
                    it.rating
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Soft-deletes items; they stay hidden until a later scan purges or restores them.
    pub fn mark_missing(&self, ids: &[i64], now_ms: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE items SET missing_since = ?2 WHERE id = ?1 AND missing_since IS NULL",
            )?;
            for id in ids {
                stmt.execute(params![id, now_ms])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn purge_items(&self, ids: &[i64]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM items WHERE id = ?1")?;
            for id in ids {
                stmt.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every item under a watched folder, keyed by path, including soft-deleted ones.
    pub fn known_items(&self, watched_id: i64) -> Result<HashMap<String, KnownItem>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(
            "SELECT i.path, i.id, i.size, i.mtime_ms, i.missing_since IS NOT NULL
             FROM items i JOIN folders f ON f.id = i.folder_id WHERE f.watched_id = ?1",
        )?;
        let rows = stmt
            .query_map(params![watched_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    KnownItem {
                        id: r.get(1)?,
                        size: r.get(2)?,
                        mtime_ms: r.get(3)?,
                        missing: r.get(4)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }

    /// Every item in `dir`'s folder and all folders beneath it, keyed by path, including
    /// soft-deleted ones. Membership comes from the folder tree rather than a path-prefix
    /// match, so `%` and `_` in a filename need no escaping and a directory with no folder
    /// row simply yields nothing.
    pub fn known_items_under(
        &self,
        watched_id: i64,
        dir: &str,
    ) -> Result<HashMap<String, KnownItem>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(
            "WITH RECURSIVE sub(id) AS (
                 SELECT id FROM folders WHERE watched_id = ?1 AND path = ?2
                 UNION ALL
                 SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id
             )
             SELECT i.path, i.id, i.size, i.mtime_ms, i.missing_since IS NOT NULL
             FROM items i WHERE i.folder_id IN (SELECT id FROM sub)",
        )?;
        let rows = stmt
            .query_map(params![watched_id, dir], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    KnownItem {
                        id: r.get(1)?,
                        size: r.get(2)?,
                        mtime_ms: r.get(3)?,
                        missing: r.get(4)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }

    pub fn item(&self, id: i64) -> Result<Option<Item>> {
        let item = self
            .reader()
            .query_row(
                "SELECT id, folder_id, path, kind, size, mtime_ms, width, height, orientation, taken_at,
                        thumb_state, thumb_error, missing_since
                 FROM items WHERE id = ?1",
                params![id],
                row_to_item,
            )
            .optional()?;
        Ok(item)
    }

    pub fn set_thumb_state(&self, id: i64, state: ThumbState, error: Option<&str>) -> Result<()> {
        self.writer().execute(
            "UPDATE items SET thumb_state = ?2, thumb_error = ?3 WHERE id = ?1",
            params![id, state.to_db(), error],
        )?;
        Ok(())
    }

    /// Like `set_thumb_state`, but only if the row still matches `item` (path/size/mtime).
    /// Returns `false` without writing if the item changed since it was read, so a worker
    /// processing a stale snapshot can't clobber a rescan's reset to `Pending`.
    pub fn set_thumb_state_if_unchanged(
        &self,
        item: &Item,
        state: ThumbState,
        error: Option<&str>,
    ) -> Result<bool> {
        let changed = self.writer().execute(
            "UPDATE items SET thumb_state = ?2, thumb_error = ?3
             WHERE id = ?1 AND path = ?4 AND size = ?5 AND mtime_ms = ?6",
            params![
                item.id,
                state.to_db(),
                error,
                item.path,
                item.size,
                item.mtime_ms
            ],
        )?;
        Ok(changed > 0)
    }

    /// Items still waiting for thumbnails, in grid order. Items under an offline watched
    /// folder are skipped: their files can't be read until the folder comes back.
    pub fn pending_thumb_ids(&self) -> Result<Vec<i64>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id FROM items i
             JOIN folders f ON f.id = i.folder_id
             JOIN watched_folders w ON w.id = f.watched_id
             WHERE i.thumb_state = 0 AND i.missing_since IS NULL AND w.online = 1 {GRID_ORDER}"
        ))?;
        let ids = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids)
    }

    /// Fingerprints of every indexed item; thumbnails for anything else are garbage.
    pub fn live_fingerprints(&self) -> Result<HashSet<u64>> {
        let conn = self.reader();
        let mut stmt = conn.prepare("SELECT path, size, mtime_ms FROM items")?;
        let set = stmt
            .query_map([], |r| {
                Ok(fingerprint(&r.get::<_, String>(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<HashSet<u64>>>()?;
        Ok(set)
    }

    /// Every visible item in grid order: folder tree order, then capture time, then name.
    pub fn grid_entries(&self) -> Result<Vec<GridEntry>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id, i.folder_id, i.taken_at, i.width, i.height, i.orientation, i.kind, i.path, i.size, i.mtime_ms, i.rating
             FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.missing_since IS NULL {GRID_ORDER}"
        ))?;
        let rows = stmt
            .query_map([], |r| {
                let (w, h) = oriented_dims(r.get(3)?, r.get(4)?, r.get(5)?);
                Ok(GridEntry {
                    id: r.get(0)?,
                    folder_id: r.get(1)?,
                    taken_at: r.get(2)?,
                    aspect: if w == 0 || h == 0 {
                        1.0
                    } else {
                        w as f32 / h as f32
                    },
                    kind: MediaKind::from_db(r.get(6)?).unwrap_or(MediaKind::Image),
                    starred: r.get::<_, Option<i64>>(10)?.unwrap_or(0) >= 1,
                    thumb_key: fingerprint(&r.get::<_, String>(7)?, r.get(8)?, r.get(9)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// How many photos carry at least one star. Served by the `items_starred` partial index.
    ///
    /// `rating >= 1` also excludes unread rows without a second clause: a comparison
    /// against NULL is never true in SQL.
    pub fn starred_count(&self) -> Result<usize> {
        let conn = self.reader();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM items WHERE rating >= 1 AND missing_since IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    #[test]
    fn insert_and_read_back_item() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 10)])
            .unwrap();

        let item = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(item.path, "/p/a.jpg");
        assert_eq!(item.folder_id, folder);
        assert_eq!(
            (item.width, item.height, item.orientation, item.taken_at),
            (400, 300, 1, 10)
        );
        assert_eq!(item.thumb_state, ThumbState::Pending);
        assert_eq!(item.missing_since, None);
        assert_eq!(item.fingerprint(), fingerprint("/p/a.jpg", 100, 1_000));
        assert!(lib.item(9_999).unwrap().is_none());
    }

    #[test]
    fn known_items_track_missing_update_and_purge() {
        let (_dir, lib) = temp_library();
        let (watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        let (a, b) = (ids[0], ids[1]);

        let known = lib.known_items(watched).unwrap();
        assert_eq!(known.len(), 2);
        assert_eq!(
            known["/p/a.jpg"],
            KnownItem {
                id: a,
                size: 100,
                mtime_ms: 1_000,
                missing: false
            }
        );

        lib.mark_missing(&[a], 50).unwrap();
        assert!(lib.known_items(watched).unwrap()["/p/a.jpg"].missing);
        assert_eq!(lib.item(a).unwrap().unwrap().missing_since, Some(50));

        lib.set_thumb_state(a, ThumbState::Ready, None).unwrap();
        let changed = NewItem {
            size: 200,
            ..new_item(folder, "/p/a.jpg", 1)
        };
        lib.update_items(&[(a, changed)]).unwrap();
        let item = lib.item(a).unwrap().unwrap();
        assert_eq!(
            (item.size, item.missing_since, item.thumb_state),
            (200, None, ThumbState::Pending)
        );

        lib.purge_items(&[b]).unwrap();
        assert!(lib.item(b).unwrap().is_none());
    }

    #[test]
    fn thumb_state_records_errors() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        lib.set_thumb_state(id, ThumbState::Failed, Some("corrupt"))
            .unwrap();
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Failed);
        assert_eq!(item.thumb_error.as_deref(), Some("corrupt"));
    }

    #[test]
    fn guarded_thumb_state_write_skips_stale_snapshots() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let stale = lib.item(id).unwrap().unwrap();

        let changed = NewItem {
            size: 999,
            ..new_item(folder, "/p/a.jpg", 1)
        };
        lib.update_items(&[(id, changed)]).unwrap();

        let ok = lib
            .set_thumb_state_if_unchanged(&stale, ThumbState::Failed, Some("stale"))
            .unwrap();
        assert!(!ok);

        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.size, 999);
        assert_eq!(item.thumb_state, ThumbState::Pending);
        assert_eq!(item.thumb_error, None);
    }

    #[test]
    fn pending_ids_follow_grid_order_and_skip_done_or_missing() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(b, "/p/b/1.jpg", 1),
                new_item(a, "/p/a/2.jpg", 5),
                new_item(a, "/p/a/1.jpg", 2),
            ])
            .unwrap();
        let (b1, a2, a1) = (ids[0], ids[1], ids[2]);
        assert_eq!(lib.pending_thumb_ids().unwrap(), [a1, a2, b1]);

        lib.set_thumb_state(a1, ThumbState::Ready, None).unwrap();
        lib.mark_missing(&[b1], 99).unwrap();
        assert_eq!(lib.pending_thumb_ids().unwrap(), [a2]);
    }

    #[test]
    fn pending_ids_skip_offline_watched_folders() {
        let (_dir, lib) = temp_library();
        let (on_w, on_f) = seed_folder(&lib, Path::new("/on"));
        let (off_w, off_f) = seed_folder(&lib, Path::new("/off"));
        let ids = lib
            .insert_items(&[
                new_item(on_f, "/on/a.jpg", 1),
                new_item(off_f, "/off/b.jpg", 1),
            ])
            .unwrap();
        lib.set_watched_online(off_w, false).unwrap();
        assert_eq!(lib.pending_thumb_ids().unwrap(), [ids[0]]);
        lib.set_watched_online(off_w, true).unwrap();
        lib.set_watched_online(on_w, false).unwrap();
        assert_eq!(lib.pending_thumb_ids().unwrap(), [ids[1]]);
    }

    #[test]
    fn folders_colliding_on_sort_key_stay_contiguous() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        // Case-sensitive filesystems allow both; they share a case-insensitive sort_key.
        let lower = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let upper = lib.upsert_folder(watched, Some(root), "/p/A", 1).unwrap();
        lib.insert_items(&[
            new_item(lower, "/p/a/1.jpg", 1),
            new_item(upper, "/p/A/2.jpg", 2),
            new_item(lower, "/p/a/3.jpg", 3),
            new_item(upper, "/p/A/4.jpg", 4),
        ])
        .unwrap();

        let folders: Vec<i64> = lib
            .grid_entries()
            .unwrap()
            .iter()
            .map(|e| e.folder_id)
            .collect();
        assert_eq!(folders, [upper, upper, lower, lower]);
        let listed: Vec<i64> = lib.folders().unwrap().iter().map(|f| f.id).collect();
        assert_eq!(listed, [root, upper, lower]);
    }

    #[test]
    fn grid_entries_are_ordered_oriented_and_skip_missing() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let rotated = NewItem {
            orientation: 6,
            ..new_item(a, "/p/a/2.jpg", 5)
        };
        let unknown = NewItem {
            width: 0,
            height: 0,
            ..new_item(b, "/p/b/1.jpg", 1)
        };
        let ids = lib
            .insert_items(&[
                unknown,
                rotated,
                new_item(a, "/p/a/1.jpg", 2),
                new_item(a, "/p/a/3.jpg", 9),
            ])
            .unwrap();
        lib.mark_missing(&[ids[3]], 1).unwrap();

        let entries = lib.grid_entries().unwrap();
        let order: Vec<i64> = entries.iter().map(|e| e.id).collect();
        assert_eq!(order, [ids[2], ids[1], ids[0]]);
        assert_eq!(entries[0].aspect, 400.0 / 300.0);
        assert_eq!(entries[1].aspect, 300.0 / 400.0);
        assert_eq!(entries[2].aspect, 1.0);
        assert_eq!(entries[0].folder_id, a);
        let expected = lib.item(ids[2]).unwrap().unwrap().fingerprint();
        assert_eq!(entries[0].thumb_key, expected);
    }

    #[test]
    fn known_items_under_covers_only_that_subtree() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let deep = lib.upsert_folder(watched, Some(a), "/p/a/deep", 1).unwrap();
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        lib.insert_items(&[
            new_item(root, "/p/top.jpg", 1),
            new_item(a, "/p/a/one.jpg", 2),
            new_item(deep, "/p/a/deep/two.jpg", 3),
            new_item(b, "/p/b/three.jpg", 4),
        ])
        .unwrap();

        let under_a = lib.known_items_under(watched, "/p/a").unwrap();
        let mut paths: Vec<&str> = under_a.keys().map(String::as_str).collect();
        paths.sort();
        assert_eq!(paths, ["/p/a/deep/two.jpg", "/p/a/one.jpg"]);

        assert_eq!(lib.known_items_under(watched, "/p").unwrap().len(), 4);
        assert!(
            lib.known_items_under(watched, "/p/missing")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn known_items_under_includes_soft_deleted_items() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let ids = lib.insert_items(&[new_item(a, "/p/a/one.jpg", 1)]).unwrap();
        lib.mark_missing(&ids, 99).unwrap();

        let under = lib.known_items_under(watched, "/p/a").unwrap();
        assert!(under["/p/a/one.jpg"].missing);
    }

    #[test]
    fn live_fingerprints_cover_all_items() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let expected = lib.item(id).unwrap().unwrap().fingerprint();
        assert_eq!(lib.live_fingerprints().unwrap(), HashSet::from([expected]));
    }

    /// `new_item` builds a row with `rating: None`; this is the same row with a rating, as
    /// the scanner produces once it has read the file's XMP.
    fn rated(folder: i64, path: &str, taken_at: i64, rating: u8) -> NewItem {
        NewItem {
            rating: Some(rating),
            ..new_item(folder, path, taken_at)
        }
    }

    #[test]
    fn grid_entries_report_whether_each_photo_is_starred() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[
            rated(folder, "/p/starred.jpg", 1, 3),
            rated(folder, "/p/unrated.jpg", 2, 0),
            new_item(folder, "/p/unread.jpg", 3),
        ])
        .unwrap();

        let mut starred: Vec<(String, bool)> = lib
            .grid_entries()
            .unwrap()
            .iter()
            .map(|e| (lib.item(e.id).unwrap().unwrap().path, e.starred))
            .collect();
        starred.sort();
        assert_eq!(
            starred,
            [
                ("/p/starred.jpg".to_string(), true),
                ("/p/unrated.jpg".to_string(), false),
                ("/p/unread.jpg".to_string(), false),
            ]
        );
    }

    #[test]
    fn only_photos_rated_at_least_one_star_are_counted() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[
            rated(folder, "/p/a.jpg", 1, 3),
            rated(folder, "/p/b.jpg", 2, 0),
            rated(folder, "/p/c.jpg", 3, 1),
        ])
        .unwrap();

        // Three rated photos, two of them starred: zero stars is a read rating, not a star.
        assert_eq!(lib.starred_count().unwrap(), 2);
    }

    #[test]
    fn an_unread_rating_is_not_counted_as_starred() {
        // `new_item` leaves `rating` NULL, which is what an unread row looks like. NULL is
        // not >= 1, so it must not reach the Starred count — SQL comparisons against NULL
        // are never true, and this pins that rather than trusting it.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn a_missing_item_is_not_counted_as_starred() {
        // A soft-deleted photo must not inflate the Starred count, the same way it does
        // not appear in the grid.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[rated(folder, "/p/a.jpg", 1, 4)])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 1);
        lib.mark_missing(&ids, 1).unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn a_changed_photo_keeps_the_rating_its_rescan_read() {
        // `update_items` runs when a file's size or mtime changed, and carries the rating
        // the fresh scan read — so a star added in another program survives a rescan.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[rated(folder, "/p/a.jpg", 1, 0)])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);

        lib.update_items(&[(ids[0], rated(folder, "/p/a.jpg", 1, 5))])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 1);
    }
}
