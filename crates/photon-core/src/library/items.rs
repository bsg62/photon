use super::Library;
use crate::Result;
use crate::grid::{GridEntry, GridView};
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
///
/// Folders run newest first by the date of their **oldest** photo, then each folder's photos
/// run oldest to newest. That is the same axis the sidebar groups by, so the list beside the
/// grid is an index of it rather than a second, unrelated ordering — before this the grid ran
/// alphabetically by path while the sidebar ran by year, and scrolling one bore no relation
/// to reading the other.
///
/// `o.oldest` and `o.fpath` come from [`folder_order`], the per-folder driver every grid
/// query is built on. [`grid_query`] pairs the two structurally so a caller cannot take one
/// without the other; `pending_thumb_ids` is the one hand-assembled query and says why.
///
/// In the Starred view the driver is given the same filter as the outer `WHERE`, so a folder
/// is placed by its oldest *starred* photo: the sidebar's sections come from that same
/// filtered index, so the two agree. Search filters in Rust after the query and so places a
/// folder by its oldest photo overall, exactly as it always has.
///
/// `fpath` breaks ties — two folders whose oldest photos share a timestamp would otherwise
/// interleave, the same hazard `sort_key` collisions used to pose.
///
/// Two earlier shapes are recorded here so nobody goes back to them. A window function,
/// `MIN(i.taken_at) OVER (PARTITION BY i.folder_id)`, made SQLite materialise and sort the
/// whole row set twice: ~88ms for `startup_grid_100k`. A `GROUP BY` join sorted it once:
/// ~60ms. Driving from the ordered *folder* list instead sorts ~1,000 folders and then walks
/// each folder's rows through `items_folder`, sorting only within a folder: ~47ms. The row
/// order is byte-identical across all three, verified on the 100k bench library.
pub(crate) const GRID_ORDER: &str = "ORDER BY o.oldest DESC, o.fpath, i.taken_at, i.file_name";

/// The grid's driver: every folder with a matching live photo, placed by its oldest one and
/// its path, already in grid order. Aliased `o` for [`GRID_ORDER`].
///
/// `filter` is the same `AND …` fragment on `i` the caller's outer `WHERE` uses, so the
/// minimum is taken over the rows the view shows rather than the whole folder; it may name
/// only `items` columns, since inside this subquery `i` is the subquery's own alias.
fn folder_order(filter: &str) -> String {
    format!(
        "(SELECT i.folder_id, MIN(i.taken_at) AS oldest, f.path AS fpath
          FROM items i JOIN folders f ON f.id = i.folder_id
          WHERE i.missing_since IS NULL {filter}
          GROUP BY i.folder_id
          ORDER BY oldest DESC, fpath) o"
    )
}

/// A whole grid query: `select` over the live items matching `filter`, in grid order, with
/// `f` (the item's folder) joined for callers that read a folder column. The one place the
/// driver's filter and the outer filter are spelled, so they cannot drift apart - a
/// driver placed by one set of rows and a result holding another is how Starred would
/// silently sort by the wrong photo.
fn grid_query(select: &str, filter: &str) -> String {
    let driver = folder_order(filter);
    format!(
        "SELECT {select}
         FROM {driver}
         JOIN items i ON i.folder_id = o.folder_id
         JOIN folders f ON f.id = i.folder_id
         WHERE i.missing_since IS NULL {filter}
         {GRID_ORDER}"
    )
}

/// The Recent view's query. Shared with the test that checks its plan, so the `ORDER BY`
/// the `items_recent` index was built for cannot drift from the one actually run.
fn recent_sql() -> String {
    format!(
        "SELECT {GRID_COLUMNS}
         FROM items i
         WHERE i.missing_since IS NULL
         ORDER BY i.taken_at DESC, i.file_name DESC, i.id DESC
         LIMIT {RECENT_LIMIT}"
    )
}

/// How many photos the Recent view shows. Picasa's equivalent list was a fixed-size window
/// onto the newest photos rather than a filter, so there is nothing to derive this from: it
/// is a chosen number, large enough to cover a few trips' worth of photos and small enough
/// that the view stays a shortlist rather than a second All view.
pub const RECENT_LIMIT: usize = 500;

/// The grid's columns, in the order `map_grid_row` reads them. Both query paths select
/// this same prefix so one mapping serves both.
///
/// `search_entries` appends more columns after this prefix and reads them by index
/// starting at `GRID_COLUMN_COUNT`: adding a column here shifts those indices, so keep
/// the two in sync.
const GRID_COLUMNS: &str = "i.id, i.folder_id, i.taken_at, i.width, i.height, i.orientation, i.kind, i.path, i.size, i.mtime_ms, i.rating";

/// Number of columns selected by `GRID_COLUMNS`. `search_entries` uses this rather than a
/// bare `11` so a future column added to `GRID_COLUMNS` can't silently shift `file_name`
/// and `folder name` into the wrong indices without also touching this constant.
const GRID_COLUMN_COUNT: usize = 11;

fn map_grid_row(r: &Row<'_>) -> rusqlite::Result<GridEntry> {
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
}

/// Shared by `known_items` and `known_items_under`, whose two queries select the same five
/// columns in the same order and differ only in how they scope the rows.
fn row_to_known(r: &Row<'_>) -> rusqlite::Result<(String, KnownItem)> {
    Ok((
        r.get::<_, String>(0)?,
        KnownItem {
            id: r.get(1)?,
            size: r.get(2)?,
            mtime_ms: r.get(3)?,
            missing: r.get(4)?,
        },
    ))
}

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
    ///
    /// Deliberately does not touch `rating`: a star is not a property of the file (see
    /// `set_ratings`), and `NewItem.rating` is always `None` for a scanned file. Writing it
    /// here would `NULL` out a folder's existing stars on the next size/mtime change, and a
    /// folder the Picasa pass could not read that scan would have no way to restore it.
    pub fn update_items(&self, items: &[(i64, NewItem)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        // The row's fingerprint changes with its size or mtime, so the thumbnails written
        // under the old one are orphaned by this write.
        super::settings::bump_thumb_gc_epoch(&tx)?;
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
                    it.taken_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every live item in one folder, as `(id, lowercased file name, current rating)`.
    ///
    /// Lowercased here because Picasa's INI may disagree in case with the files on disk, and
    /// this codebase folds case in Rust rather than in SQL: there is no `COLLATE NOCASE` on
    /// `file_name` and `lower()` is ASCII-only in SQLite without the ICU extension, which is a
    /// native dependency photon does not take. The current rating is included so the Picasa
    /// pass can write only the rows that actually change, rather than every row every scan.
    pub fn folder_item_names(&self, folder_id: i64) -> Result<Vec<(i64, String, Option<i64>)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, file_name, rating FROM items WHERE folder_id = ?1 AND missing_since IS NULL",
        )?;
        let rows = stmt
            .query_map(params![folder_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?.to_lowercase(),
                    r.get::<_, Option<i64>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Sets the rating on specific items, leaving every other column alone.
    ///
    /// Deliberately not part of `update_items`: that rewrites a row from a rescanned file and
    /// resets its thumbnail, which is wrong for a star that changed while the photo did not.
    pub fn set_ratings(&self, ratings: &[(i64, u8)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE items SET rating = ?2 WHERE id = ?1")?;
            for (id, rating) in ratings {
                stmt.execute(params![id, rating])?;
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
        super::settings::bump_thumb_gc_epoch(&tx)?;
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
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT i.path, i.id, i.size, i.mtime_ms, i.missing_since IS NOT NULL
             FROM items i JOIN folders f ON f.id = i.folder_id WHERE f.watched_id = ?1",
        )?;
        let rows = stmt
            .query_map(params![watched_id], row_to_known)?
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
        let conn = self.reader()?;
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
            .query_map(params![watched_id, dir], row_to_known)?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }

    pub fn item(&self, id: i64) -> Result<Option<Item>> {
        let item = self
            .reader()?
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
    ///
    /// "Grid order" means the All view's: folders placed by their oldest photo overall, so
    /// the queue works through folders in the order the grid shows them. The join is
    /// therefore unfiltered, unlike Starred's.
    ///
    /// Assembled by hand rather than through `grid_query`, because its outer filter reads
    /// `w.online`, which the driver cannot see. The driver is therefore unfiltered, and
    /// the planner walks from `items_pending` regardless, so the shape costs nothing.
    pub fn pending_thumb_ids(&self) -> Result<Vec<i64>> {
        let conn = self.reader()?;
        let driver = folder_order("");
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id
             FROM {driver}
             JOIN items i ON i.folder_id = o.folder_id
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
        let conn = self.reader()?;
        let mut stmt = conn.prepare("SELECT path, size, mtime_ms FROM items")?;
        let set = stmt
            .query_map([], |r| {
                Ok(fingerprint(&r.get::<_, String>(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<HashSet<u64>>>()?;
        Ok(set)
    }

    /// Every visible item in grid order: newest folder first by its oldest photo, then each
    /// folder's photos oldest to newest.
    pub fn grid_entries(&self) -> Result<Vec<GridEntry>> {
        self.entries_for(GridView::All, "")
    }

    /// The grid's rows for one view. `query` is used only by `Search`; the other views
    /// ignore it. One entry point rather than two, because `GridView` is matched
    /// exhaustively and a `Search` arm that could not see the query would have to lie.
    pub fn entries_for(&self, view: GridView, query: &str) -> Result<Vec<GridEntry>> {
        match view {
            GridView::All => self.entries_filtered(""),
            GridView::Starred => self.entries_filtered("AND i.rating >= 1"),
            GridView::Recent => self.recent_entries(),
            GridView::Search => self.search_entries(query),
        }
    }

    /// The grid's rows for a `WHERE` filter fragment, applied both to the rows returned and
    /// to the per-folder placement the order is built on (see `grid_query`). `Starred`
    /// filters to `rating >= 1`; the `items_starred` partial index can narrow that scan, but
    /// the query still joins `folders` and orders by `GRID_ORDER`, so it does not serve the
    /// query outright the way it does `starred_count`.
    fn entries_filtered(&self, filter: &str) -> Result<Vec<GridEntry>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(&grid_query(GRID_COLUMNS, filter))?;
        let rows = stmt
            .query_map([], map_grid_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The newest `RECENT_LIMIT` photos, newest capture date first.
    ///
    /// The one view that does not use `GRID_ORDER`: ordering by folder would make "newest"
    /// mean "in the newest folder", and the whole point of this list is the individual
    /// photos.
    ///
    /// The consequence is that `GridIndex::build` starts a section on every photo wherever
    /// folders overlap in time — 500 photos came back as 500 sections from a library of
    /// twelve interleaved folders. These rows are therefore *not* laid out as folder runs:
    /// the UI collapses them into one continuous run of tiles (`layout.ts`,
    /// `layoutSections`). Anything else reading `sections` for this view has to expect a
    /// folder to appear in many of them.
    ///
    /// `file_name` and `id` break ties so the cut at `RECENT_LIMIT` is deterministic:
    /// without them two photos sharing a capture time could swap across the boundary
    /// between rebuilds and the view would flicker for no reason.
    ///
    /// No join to `folders`: unlike `GRID_ORDER`, nothing here reads a folder column.
    fn recent_entries(&self) -> Result<Vec<GridEntry>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(&recent_sql())?;
        let rows = stmt
            .query_map([], map_grid_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Photos whose file name or folder name contains `query`, case-insensitively.
    ///
    /// The match runs in Rust rather than as SQL `LIKE` for two reasons (spec §3):
    /// SQLite folds case for ASCII only, so `MÜNCHEN` would not find `München`; and
    /// `LIKE` would read `%` and `_` in the user's query as wildcards. This is one pass
    /// over the same rows an index rebuild already reads, with two short string compares
    /// added per row.
    fn search_entries(&self, query: &str) -> Result<Vec<GridEntry>> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.reader()?;
        let mut stmt = conn.prepare(&grid_query(
            &format!("{GRID_COLUMNS}, i.file_name, f.name"),
            "",
        ))?;
        let rows = stmt
            .query_map([], |r| {
                let file_name: String = r.get(GRID_COLUMN_COUNT)?;
                let folder_name: String = r.get(GRID_COLUMN_COUNT + 1)?;
                let hit = file_name.to_lowercase().contains(&needle)
                    || folder_name.to_lowercase().contains(&needle);
                // No `Ok(…?)` wrapper here: the closure already returns this type, and
                // wrapping it trips `clippy::needless_question_mark`, which the gate
                // treats as an error.
                hit.then(|| map_grid_row(r)).transpose()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(rows)
    }

    /// How many photos carry at least one star. Served by the `items_starred` partial index.
    ///
    /// `rating >= 1` also excludes unread rows without a second clause: a comparison
    /// against NULL is never true in SQL.
    pub fn starred_count(&self) -> Result<usize> {
        let conn = self.reader()?;
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
    fn folder_item_names_lowercases_excludes_missing_rows_and_reports_the_current_rating() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/DSC_0001.JPG", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        let (a, b) = (ids[0], ids[1]);
        lib.mark_missing(&[b], 50).unwrap();
        lib.set_ratings(&[(a, 1)]).unwrap();

        let mut names = lib.folder_item_names(folder).unwrap();
        names.sort();
        assert_eq!(names, vec![(a, "dsc_0001.jpg".to_string(), Some(1))]);
    }

    #[test]
    fn set_ratings_changes_only_the_rating() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let before = lib.item(id).unwrap().unwrap();

        lib.set_ratings(&[(id, 1)]).unwrap();

        let after = lib.item(id).unwrap().unwrap();
        assert_eq!(
            (
                after.path.clone(),
                after.size,
                after.mtime_ms,
                after.width,
                after.height,
                after.orientation,
                after.taken_at,
                after.thumb_state,
                after.missing_since
            ),
            (
                before.path,
                before.size,
                before.mtime_ms,
                before.width,
                before.height,
                before.orientation,
                before.taken_at,
                before.thumb_state,
                before.missing_since
            )
        );
        assert_eq!(lib.starred_count().unwrap(), 1);
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
        // Grid order, which the thumbnail queue follows so tiles render roughly in the order
        // they will be scrolled past. Folder `a` starts at 2 and `b` at 1, so `a` — the newer
        // folder by its oldest photo — comes first, and within it 2 before 5.
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
    fn folders_run_newest_first_by_their_oldest_photo_not_alphabetically() {
        // THE test for the grid's ordering. Path order and date order are made to disagree:
        // `alpha` sorts first by name but holds the older photos, so under the old
        // `ORDER BY f.sort_key` rule it led the grid while the sidebar — grouped by year,
        // newest first — listed `zulu` above it. Scrolling the grid then bore no relation to
        // reading the list beside it.
        //
        // Every other ordering test in this file happens to produce the same sequence under
        // both rules, so without this one the whole change is unpinned: reverting
        // `GRID_ORDER` to the path form leaves the suite green.
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let alpha = lib
            .upsert_folder(watched, Some(root), "/p/alpha", 1)
            .unwrap();
        let zulu = lib
            .upsert_folder(watched, Some(root), "/p/zulu", 1)
            .unwrap();
        let ids = lib
            .insert_items(&[
                new_item(alpha, "/p/alpha/old.jpg", 1_000),
                new_item(zulu, "/p/zulu/new.jpg", 9_000),
            ])
            .unwrap();
        let (alpha_old, zulu_new) = (ids[0], ids[1]);

        let order: Vec<i64> = lib.grid_entries().unwrap().iter().map(|e| e.id).collect();
        assert_eq!(
            order,
            [zulu_new, alpha_old],
            "the folder whose oldest photo is newer comes first, regardless of its name"
        );
    }

    #[test]
    fn folders_starting_on_the_same_photo_date_stay_contiguous() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        // Both folders' oldest photo is at 1, so the primary sort key ties and only `f.path`
        // keeps their photos from interleaving. Case-sensitive filesystems allow both names.
        let lower = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let upper = lib.upsert_folder(watched, Some(root), "/p/A", 1).unwrap();
        lib.insert_items(&[
            new_item(lower, "/p/a/1.jpg", 1),
            new_item(upper, "/p/A/2.jpg", 1),
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
        assert_eq!(
            folders,
            [upper, upper, lower, lower],
            "a tie on the folder's oldest photo must not interleave two folders' photos"
        );
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
        // Folder `a`'s oldest live photo is at 2 and `b`'s at 1, so `a` leads; within `a`,
        // 2 before 5. The missing item at 9 is excluded and so cannot decide `a`'s position.
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
    /// the scanner produces once its Picasa INI pass has confirmed a star.
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
    fn a_changed_photo_keeps_the_rating_a_later_set_ratings_call_wrote() {
        // `update_items` runs when a file's size or mtime changed, and it must leave
        // `rating` alone: it is the Picasa pass, via `set_ratings`, that owns the column,
        // not a rescan of the file itself.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[rated(folder, "/p/a.jpg", 1, 0)])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);

        lib.set_ratings(&[(ids[0], 5)]).unwrap();
        lib.update_items(&[(ids[0], new_item(folder, "/p/a.jpg", 1))])
            .unwrap();
        assert_eq!(
            lib.starred_count().unwrap(),
            1,
            "a rescan of the file must not clear a rating set_ratings wrote"
        );
    }

    #[test]
    fn the_starred_view_contains_exactly_the_starred_photos() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
                new_item(folder, "/p/c.jpg", 3),
            ])
            .unwrap();
        lib.set_ratings(&[(ids[0], 0), (ids[1], 1), (ids[2], 5)])
            .unwrap();

        let all: Vec<i64> = lib
            .entries_for(GridView::All, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        let starred: Vec<i64> = lib
            .entries_for(GridView::Starred, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(all, ids);
        assert_eq!(
            starred,
            vec![ids[1], ids[2]],
            "unrated and zero-rated are excluded"
        );
    }

    #[test]
    fn the_starred_view_places_a_folder_by_its_oldest_starred_photo() {
        // The ordering's per-folder minimum is taken over the rows the view actually shows,
        // not over the whole folder: the sidebar's sections come from this same filtered
        // index, so a folder whose only star is recent must sort as a recent folder in
        // Starred even though its unstarred photos go back years. An implementation that
        // computed each folder's oldest photo once, over every live row, would pass every
        // other ordering test and fail this one.
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let alpha = lib
            .upsert_folder(watched, Some(root), "/p/alpha", 1)
            .unwrap();
        let zulu = lib
            .upsert_folder(watched, Some(root), "/p/zulu", 1)
            .unwrap();
        lib.insert_items(&[
            rated(alpha, "/p/alpha/old-unstarred.jpg", 1, 0),
            rated(alpha, "/p/alpha/new-starred.jpg", 9, 1),
            rated(zulu, "/p/zulu/starred.jpg", 5, 1),
        ])
        .unwrap();

        let folders: Vec<i64> = lib
            .entries_for(GridView::Starred, "")
            .unwrap()
            .iter()
            .map(|e| e.folder_id)
            .collect();
        assert_eq!(
            folders,
            [alpha, zulu],
            "alpha's oldest *starred* photo (9) is newer than zulu's (5), so alpha leads; \
             by its oldest photo overall (1) it would trail"
        );
    }

    #[test]
    fn the_recent_view_is_the_newest_photos_first_across_folders() {
        let (_dir, lib) = temp_library();
        let (watched, older_folder) = seed_folder(&lib, Path::new("/p/older"));
        let newer_folder = lib.upsert_folder(watched, None, "/p/newer", 1).unwrap();
        // The older *folder* (by its oldest photo) holds the newest single photo, so a
        // result in folder order would put `/p/older/new.jpg` last instead of first.
        let ids = lib
            .insert_items(&[
                new_item(older_folder, "/p/older/old.jpg", 1),
                new_item(older_folder, "/p/older/new.jpg", 40),
                new_item(newer_folder, "/p/newer/a.jpg", 20),
                new_item(newer_folder, "/p/newer/b.jpg", 30),
            ])
            .unwrap();

        let recent: Vec<i64> = lib
            .entries_for(GridView::Recent, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();

        assert_eq!(recent, vec![ids[1], ids[3], ids[2], ids[0]]);
    }

    /// Pins that the Recent query is actually served by `items_recent` rather than by a
    /// scan and sort. An index whose columns or direction drift from the `ORDER BY` still
    /// exists and still passes the migration test, but SQLite silently stops using it.
    #[test]
    fn the_recent_view_is_served_by_its_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {}", recent_sql()))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|step| step.contains("items_recent")),
            "expected an index walk, got {plan:?}"
        );
        assert!(
            !plan.iter().any(|step| step.contains("TEMP B-TREE")),
            "the order must come from the index, not a sort: {plan:?}"
        );
    }

    #[test]
    fn the_recent_view_keeps_only_the_newest_recent_limit_photos() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let items: Vec<NewItem> = (0..RECENT_LIMIT + 10)
            .map(|i| new_item(folder, &format!("/p/{i:04}.jpg"), i as i64))
            .collect();
        let ids = lib.insert_items(&items).unwrap();

        let recent: Vec<i64> = lib
            .entries_for(GridView::Recent, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();

        assert_eq!(recent.len(), RECENT_LIMIT);
        // The ten oldest are the ones dropped, and the newest is still first.
        assert_eq!(recent[0], *ids.last().unwrap());
        assert_eq!(*recent.last().unwrap(), ids[10]);
    }

    #[test]
    fn a_missing_photo_is_not_in_the_recent_view() {
        // A soft-deleted photo must not occupy one of the slots, the same way it does not
        // appear in the grid or the Starred count.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        lib.mark_missing(&ids[1..], 50).unwrap();

        let recent: Vec<i64> = lib
            .entries_for(GridView::Recent, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(recent, vec![ids[0]]);
    }

    #[test]
    fn search_matches_a_substring_of_the_file_name() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/sunset-beach.jpg", 1),
                new_item(folder, "/p/mountain.jpg", 2),
            ])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "beach")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(hits, vec![ids[0]]);
    }

    #[test]
    fn search_matches_a_substring_of_the_folder_name() {
        let (_dir, lib) = temp_library();
        let (_watched, holiday) = seed_folder(&lib, Path::new("/holiday-2024"));
        let ids = lib
            .insert_items(&[new_item(holiday, "/holiday-2024/a.jpg", 1)])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "holiday")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(
            hits, ids,
            "the folder's name matches even though the file's does not"
        );
    }

    #[test]
    fn a_query_matching_neither_name_returns_nothing() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();

        assert!(
            lib.entries_for(GridView::Search, "zzz").unwrap().is_empty(),
            "no match is an empty result, not the whole library"
        );
    }

    #[test]
    fn search_folds_case_for_non_ascii_text() {
        // This is the test that pins the whole "match in Rust, not in SQL" decision
        // (spec §3): SQLite's LIKE and lower() fold ASCII only, so a `LIKE`-based
        // implementation passes the ASCII cases above and fails this one. Deleting it
        // removes the only evidence for the design.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/München"));
        lib.insert_items(&[new_item(folder, "/München/Straße.jpg", 1)])
            .unwrap();

        // Of these four, only "MÜNCHEN" actually discriminates the Rust-vs-SQL-LIKE
        // design: SQLite's LIKE folds ASCII case, and `ü` already matches `ü` exactly,
        // so `'München' LIKE '%münchen%'` is true under LIKE too. `Ü` is the character
        // LIKE does not fold. Do not trim this loop down without keeping "MÜNCHEN".
        for query in ["münchen", "MÜNCHEN", "München"] {
            assert_eq!(
                lib.entries_for(GridView::Search, query).unwrap().len(),
                1,
                "{query} must find the folder München regardless of case"
            );
        }
        assert_eq!(
            lib.entries_for(GridView::Search, "straße").unwrap().len(),
            1,
            "the file Straße.jpg is found by its own name"
        );
    }

    #[test]
    fn search_does_not_treat_ss_and_eszett_as_the_same_letter() {
        // A documented limit, not an aspiration. Rust's `to_lowercase` maps "Straße" to
        // "straße" and "STRASSE" to "strasse", so the two spellings never meet. Someone
        // who types `strasse` looking for `Straße.jpg` finds nothing.
        //
        // Left as-is deliberately: fixing it means full Unicode case-folding (ß → ss),
        // which needs a dependency or a hand-rolled table, and this is a simple search.
        // The test exists so the behaviour is a decision on record rather than a surprise.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/Straße.jpg", 1)])
            .unwrap();

        assert!(
            lib.entries_for(GridView::Search, "strasse")
                .unwrap()
                .is_empty(),
            "ß does not case-fold to ss"
        );
    }

    #[test]
    fn search_treats_sql_wildcards_as_literal_characters() {
        // A LIKE-based implementation would return both rows for "%", since an
        // unescaped % matches everything (spec §3).
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/50% grey.jpg", 1),
                new_item(folder, "/p/plain.jpg", 2),
            ])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "%")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(hits, vec![ids[0]], "% matches the character, not every row");

        // Same idea for `_`, LIKE's single-character wildcard (spec §7): a LIKE-based
        // implementation would return both rows, since an unescaped `_` matches any
        // one character rather than a literal underscore.
        let (_watched2, folder2) = seed_folder(&lib, Path::new("/q"));
        let underscore_ids = lib
            .insert_items(&[
                new_item(folder2, "/q/snap_01.jpg", 1),
                new_item(folder2, "/q/noseparator.jpg", 2),
            ])
            .unwrap();
        let underscore_hits: Vec<i64> = lib
            .entries_for(GridView::Search, "_")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(
            underscore_hits,
            vec![underscore_ids[0]],
            "_ matches the character, not any character"
        );
    }

    #[test]
    fn search_keeps_grid_order_and_excludes_missing_items() {
        // Three items so surviving results can actually show an order: a one-element
        // result cannot discriminate `{GRID_ORDER}` from no ordering at all, which is
        // exactly the gap this test used to leave (search_entries has its own SQL
        // string, separate from entries_filtered's, and nothing else exercised it).
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/trip"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/trip/b.jpg", 2),
                new_item(folder, "/trip/a.jpg", 1),
                new_item(folder, "/trip/c.jpg", 3),
            ])
            .unwrap();
        let (b, a, c) = (ids[0], ids[1], ids[2]);
        // `mark_missing` takes a timestamp as its second argument; the existing tests in
        // this file call it as `mark_missing(&ids, 99)`.
        lib.mark_missing(&[c], 99).unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "trip")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(
            hits,
            vec![a, b],
            "a missing item is excluded, and the survivors keep grid order (by taken_at here)"
        );
    }

    #[test]
    fn an_empty_search_query_matches_nothing_rather_than_everything() {
        // The engine turns an empty query back into the All view (Task 2); this is the
        // safety net under that, so a bug there shows as an empty grid rather than as a
        // "search" indistinguishable from the full library.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();

        assert!(lib.entries_for(GridView::Search, "").unwrap().is_empty());
        assert!(lib.entries_for(GridView::Search, "   ").unwrap().is_empty());
    }

    #[test]
    fn the_all_and_starred_views_are_unchanged_by_the_new_entry_point() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        lib.set_ratings(&[(ids[1], 3)]).unwrap();

        let all: Vec<i64> = lib
            .entries_for(GridView::All, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        let starred: Vec<i64> = lib
            .entries_for(GridView::Starred, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(all, ids);
        assert_eq!(starred, vec![ids[1]]);
        assert_eq!(
            lib.grid_entries().unwrap().len(),
            2,
            "the convenience wrapper still means All"
        );
    }
}
