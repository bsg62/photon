//! The queries behind the duplicate finder. The hashing itself is `crate::duplicates`;
//! this is only what it reads and writes, and what the UI asks afterwards.

use super::Library;
use crate::Result;
use rusqlite::params;

/// A file the hashing pass should read: it shares its size with another live file and has
/// no hash yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashCandidate {
    pub id: i64,
    pub path: String,
    /// The fingerprint the hash will be stored against; see [`Library::set_content_hash`].
    pub size: i64,
    pub mtime_ms: i64,
}

/// Another file with the same bytes, or the same picture, as the one asked about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemCopy {
    pub id: i64,
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// The photos that have at least one byte-identical twin **or** a look-alike, as a grid
/// filter. Applied to the driver as well as the outer `WHERE`, like every membership view,
/// so a folder is placed by its oldest matching photo and the sidebar agrees with the grid.
///
/// **Both halves count their own group's live members, and that is not symmetry for its own
/// sake.** The two columns are different kinds of fact. `content_hash` is derived here, at
/// query time, so `HAVING COUNT(*) > 1` is the whole of the membership test and a photo
/// whose one twin is deleted or goes missing stops matching on the very next query, with no
/// pass in between - which is why `remove_watched_folder` and `set_item_edit` have never
/// needed to run one. `similar_group` is *materialised* by the last pass: `crate::similar`
/// only ever emits members of groups of two or more, but a later write can leave a survivor
/// behind - `set_item_edit` clears the edited row's group, and any delete removes members
/// from groups it does not rewrite - and a bare `similar_group IS NOT NULL` would then show
/// that survivor in Duplicates as a lone duplicate with no copy of any kind. Repeating the
/// `HAVING COUNT(*) > 1` test over the *stored* groups makes this half self-correcting in
/// exactly the way the identical half already is, so a stale column can only cost a photo
/// its grouping until the next pass, never a wrong view. The requested passes
/// (`Engine::request_similar_pass` after an edit or a folder removal) are what make the
/// grouping right again; this is what keeps the view right in the meantime.
///
/// `UNION ALL` inside an `IN`, not a plain `i.content_hash IN (...) OR i.similar_group IS NOT
/// NULL`: the `OR` form plans as a scan of every live row with both halves checked in place,
/// `items_similar_group` untouched, because an `OR` of two unrelated conditions is not the
/// rowid-merge case SQLite optimises. Two membership subqueries joined by `UNION ALL` are
/// seeks against `items_content_hash` and `items_similar_group` each, materialised once into
/// the list `i` is then looked up against by rowid;
/// `the_widened_view_reaches_look_alikes_through_the_similar_index_too` pins that plan. `ALL`
/// rather than a de-duplicating `UNION` because the outer `IN` only tests membership - a photo
/// counted in both halves costs nothing extra, while a plain `UNION` forces a sort to
/// de-duplicate that the `IN` never needed.
///
/// In `grid_query` this is not a pure win: driving `i` from the materialised id list replaces
/// the folder-driven walk `GRID_ORDER`'s own comment measured at 88->60->47ms, and the result
/// still needs `USE TEMP B-TREE FOR ORDER BY` to put the matched rows into grid order (there is
/// no index shaped like `GRID_ORDER` over an arbitrary id list). Judged worth it because the
/// old `OR` form scanned every live row - twice, once for the driver's subquery and once for
/// the outer `WHERE` - while this sorts only the matched subset, which for a duplicate/look-alike
/// view is normally a small fraction of the library;
/// `the_grid_query_also_reaches_look_alikes_through_the_similar_index` pins the index use, not
/// the sort, since the sort is real and expected here rather than a defect to eliminate.
pub(crate) const DUPLICATE_FILTER: &str = "AND i.id IN (
    SELECT id FROM items WHERE content_hash IN (
        SELECT content_hash FROM items
        WHERE content_hash IS NOT NULL AND missing_since IS NULL
        GROUP BY content_hash HAVING COUNT(*) > 1)
    UNION ALL
    SELECT id FROM items WHERE similar_group IS NOT NULL AND missing_since IS NULL
      AND similar_group IN (
        SELECT similar_group FROM items
        WHERE similar_group IS NOT NULL AND missing_since IS NULL
        GROUP BY similar_group HAVING COUNT(*) > 1))";

/// Runs after every scan, so the size grouping has to come from `items_size` rather than
/// a sort of the whole table; `the_candidate_query_groups_sizes_from_the_index` pins that.
const CANDIDATES_SQL: &str = "SELECT i.id, i.path, i.size, i.mtime_ms
     FROM items i
     JOIN folders f ON f.id = i.folder_id
     JOIN watched_folders w ON w.id = f.watched_id
     WHERE i.missing_since IS NULL AND i.content_hash IS NULL AND w.online = 1
       AND i.size IN (SELECT size FROM items WHERE missing_since IS NULL
                      GROUP BY size HAVING COUNT(*) > 1)
     ORDER BY i.size, i.id";

/// Runs for every photo the viewer opens, so it has to come from `items_content_hash`;
/// `a_photos_copies_are_found_through_the_hash_index` pins that. An unhashed photo has no
/// copies by construction: NULL equals nothing.
const COPIES_SQL: &str = "SELECT o.id, o.path, o.width, o.height FROM items i
     JOIN items o ON o.content_hash = i.content_hash AND o.id <> i.id
     WHERE i.id = ?1 AND o.missing_since IS NULL
     ORDER BY o.path";

/// `duplicate_count`'s query, shared with its plan test so the two cannot drift onto two
/// different strings that happen to look alike.
fn duplicate_count_sql() -> String {
    format!("SELECT COUNT(*) FROM items i WHERE i.missing_since IS NULL {DUPLICATE_FILTER}")
}

impl Library {
    /// Unhashed live files whose size another live file shares, in online folders only.
    ///
    /// Size first is what makes the finder affordable: two files of different sizes cannot
    /// be identical, a photo's exact byte size is close to unique, and so nearly the whole
    /// library is never opened. An offline root is skipped rather than tried: it is rescanned
    /// every 30 seconds while its drive is away, and every one of its candidates would fail
    /// to open each time.
    pub fn hash_candidates(&self) -> Result<Vec<HashCandidate>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(CANDIDATES_SQL)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(HashCandidate {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    size: r.get(2)?,
                    mtime_ms: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Stores a hash, but only against the fingerprint it was computed for. The pass reads
    /// files long after the scan that listed them; a file rewritten in between has a row
    /// the next scan will reset, and a hash of the *new* bytes stored under the *old* size
    /// and mtime must not land on it meanwhile. Returns whether the row took it.
    pub fn set_content_hash(&self, candidate: &HashCandidate, hash: &[u8; 16]) -> Result<bool> {
        let changed = self.writer().execute(
            "UPDATE items SET content_hash = ?2
             WHERE id = ?1 AND size = ?3 AND mtime_ms = ?4 AND missing_since IS NULL",
            params![
                candidate.id,
                hash.as_slice(),
                candidate.size,
                candidate.mtime_ms
            ],
        )?;
        Ok(changed == 1)
    }

    /// How many photos have a byte-identical twin or a look-alike. Counts photos, not
    /// groups, because it labels a view that shows photos.
    pub fn duplicate_count(&self) -> Result<usize> {
        let conn = self.reader()?;
        let count: i64 = conn.query_row(&duplicate_count_sql(), [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// The other live files with the same bytes as `item_id`, by path.
    pub fn copies_of(&self, item_id: i64) -> Result<Vec<ItemCopy>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(COPIES_SQL)?;
        let rows = stmt
            .query_map([item_id], |r| {
                Ok(ItemCopy {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    width: r.get(2)?,
                    height: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GridView;
    use crate::library::items::{GRID_COLUMNS, grid_query};
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    fn plan(lib: &Library, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Vec<String> {
        let conn = lib.reader().unwrap();
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        stmt.query_map(params, |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn the_candidate_query_groups_sizes_from_the_index() {
        let (_dir, lib) = temp_library();
        let plan = plan(&lib, CANDIDATES_SQL, &[]);
        assert!(
            plan.iter().any(|step| step.contains("items_size")),
            "expected the size index, got {plan:?}"
        );
    }

    #[test]
    fn a_photos_copies_are_found_through_the_hash_index() {
        let (_dir, lib) = temp_library();
        let plan = plan(&lib, COPIES_SQL, &[&1i64]);
        assert!(
            plan.iter().any(|step| step.contains("items_content_hash")),
            "expected the hash index, got {plan:?}"
        );
    }

    #[test]
    fn the_widened_view_reaches_look_alikes_through_the_similar_index_too() {
        let (_dir, lib) = temp_library();
        let plan = plan(&lib, &duplicate_count_sql(), &[]);
        assert!(
            plan.iter().any(|step| step.contains("items_similar_group")),
            "expected the partial similar_group index, got {plan:?}"
        );
        // No `TEMP B-TREE` assertion here: `SELECT COUNT(*)` has no `ORDER BY` or `GROUP BY`
        // of its own, so it cannot produce one either way - that check would pass whether or
        // not the query is actually well-planned, which is not a check. See the grid-query
        // test below for a query where a sort is real and worth naming.
    }

    /// The grid query is the one CLAUDE.md actually warns about: driver and outer filter
    /// pinned to the same string, so a folder is placed by its oldest *matching* photo. The
    /// `COUNT(*)` query above never runs the driver at all, so it cannot catch a driver that
    /// silently stopped seeing look-alikes.
    #[test]
    fn the_grid_query_also_reaches_look_alikes_through_the_similar_index() {
        let (_dir, lib) = temp_library();
        let plan = plan(&lib, &grid_query(GRID_COLUMNS, DUPLICATE_FILTER), &[]);
        assert!(
            plan.iter().any(|step| step.contains("items_similar_group")),
            "expected the partial similar_group index, got {plan:?}"
        );
        // Unlike `duplicate_count`, this query really does sort (`GRID_ORDER`) over the
        // matched subset, so a bare `TEMP B-TREE` absence assertion here would fail today -
        // see the widening comment on `DUPLICATE_FILTER` for why that sort is still cheaper
        // than the alternative it replaced.
    }

    /// The analogue of `the_starred_view_places_a_folder_by_its_oldest_starred_photo`
    /// (`items.rs`) for the widened Duplicates view: nothing else in this file discriminates
    /// folder *placement*, only membership and counts, and the rewrite from a column test to
    /// an `i.id IN (...)` subquery is exactly the kind of change that could stop reaching
    /// `folder_order`'s copy of the filter while still reaching the outer one.
    #[test]
    fn the_duplicates_view_places_a_folder_by_its_oldest_matching_photo() {
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
                // alpha's oldest photo overall predates zulu's, but is not a look-alike of
                // anything; alpha's *matching* photo is newer than zulu's.
                new_item(alpha, "/p/alpha/old-unmatched.jpg", 1),
                new_item(alpha, "/p/alpha/new-matched.jpg", 9),
                new_item(zulu, "/p/zulu/matched.jpg", 5),
            ])
            .unwrap();
        lib.set_similar_groups(&[(ids[1], ids[1]), (ids[2], ids[1])])
            .unwrap();

        let folders: Vec<i64> = lib
            .entries_for(GridView::Duplicates, "")
            .unwrap()
            .iter()
            .map(|e| e.folder_id)
            .collect();
        assert_eq!(
            folders,
            [alpha, zulu],
            "alpha's oldest *matching* photo (9) is newer than zulu's (5), so alpha leads; \
             by its oldest photo overall (1) it would trail"
        );
    }

    /// A group's last survivor is not a duplicate. Both halves of the filter have to answer
    /// that the same way, and only the identical half did so for free: `similar_group` is
    /// left behind by writes that never run a hashing pass, and before the `HAVING` test was
    /// added to the look-alike half this showed a lone photo in Duplicates - "1 photo",
    /// nothing it resembles - until the app was restarted.
    #[test]
    fn a_look_alike_whose_only_partner_is_purged_stops_being_a_duplicate() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])])
            .unwrap();
        assert_eq!(lib.duplicate_count().unwrap(), 2);

        // What `remove_watched_folder`'s cascade does to a photo grouped with one in
        // another root: the row goes, the survivor's stale column stays.
        lib.purge_items(&[ids[1]]).unwrap();
        assert_eq!(
            lib.duplicate_count().unwrap(),
            0,
            "the survivor of a purged pair is a duplicate of nothing"
        );
        assert!(
            lib.entries_for(GridView::Duplicates, "")
                .unwrap()
                .is_empty(),
            "and the grid must agree with the count"
        );
    }

    /// The same hole reached by the other route, and the common one: the user presses `R` in
    /// the viewer. `set_item_edit` clears the edited row's group in its own transaction, so
    /// the *partner* is the survivor here, and the grid re-renders immediately because the
    /// edit refreshes it.
    #[test]
    fn a_look_alike_whose_only_partner_is_edited_stops_being_a_duplicate() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])])
            .unwrap();

        assert!(
            lib.set_item_edit(
                ids[1],
                crate::edit::Edit {
                    turns: 1,
                    crop: None
                }
            )
            .unwrap()
        );
        assert_eq!(
            lib.duplicate_count().unwrap(),
            0,
            "the unedited partner is a duplicate of nothing until the next pass regroups"
        );
    }
}
