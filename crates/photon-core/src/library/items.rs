use super::Library;
use crate::Result;
use crate::media::{MediaKind, ThumbState, fingerprint};
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
pub(crate) const GRID_ORDER: &str = "ORDER BY f.sort_key, i.taken_at, i.file_name";

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
                "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                    it.taken_at
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
                        width = ?8, height = ?9, orientation = ?10, taken_at = ?11,
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
                    it.taken_at
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

    /// Items still waiting for thumbnails, in grid order.
    pub fn pending_thumb_ids(&self) -> Result<Vec<i64>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.thumb_state = 0 AND i.missing_since IS NULL {GRID_ORDER}"
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
    fn live_fingerprints_cover_all_items() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let expected = lib.item(id).unwrap().unwrap().fingerprint();
        assert_eq!(lib.live_fingerprints().unwrap(), HashSet::from([expected]));
    }
}
