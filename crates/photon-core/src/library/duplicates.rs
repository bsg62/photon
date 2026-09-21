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
/// The identical half counts live rows only: a photo whose one twin has gone missing is no
/// longer a duplicate of anything the user can find. The look-alike half is a single column
/// read, because `crate::similar` has already done the grouping.
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
pub(crate) const DUPLICATE_FILTER: &str = "AND i.id IN (
    SELECT id FROM items WHERE content_hash IN (
        SELECT content_hash FROM items
        WHERE content_hash IS NOT NULL AND missing_since IS NULL
        GROUP BY content_hash HAVING COUNT(*) > 1)
    UNION ALL
    SELECT id FROM items WHERE similar_group IS NOT NULL AND missing_since IS NULL)";

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

    /// How many photos have a byte-identical twin. Counts photos, not groups, because it
    /// labels a view that shows photos.
    pub fn duplicate_count(&self) -> Result<usize> {
        let conn = self.reader()?;
        let count: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM items i WHERE i.missing_since IS NULL {DUPLICATE_FILTER}"
            ),
            [],
            |r| r.get(0),
        )?;
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
    use crate::testutil::temp_library;

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
        let sql = format!(
            "SELECT COUNT(*) FROM items i WHERE i.missing_since IS NULL {DUPLICATE_FILTER}"
        );
        let plan = plan(&lib, &sql, &[]);
        assert!(
            plan.iter().any(|step| step.contains("items_similar_group")),
            "expected the partial similar_group index, got {plan:?}"
        );
        assert!(
            !plan.iter().any(|step| step.contains("TEMP B-TREE")),
            "the two halves must be served by their own indexes, not a sort: {plan:?}"
        );
    }
}
