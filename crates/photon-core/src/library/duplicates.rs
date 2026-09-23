//! The queries behind the duplicate finder. The hashing itself is `crate::duplicates`;
//! this is only what it reads and writes, and what the UI asks afterwards.

use super::Library;
use crate::Result;
use rusqlite::{OptionalExtension, params};

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
    /// The copy's dimensions **as shown**, like `ViewerItem`'s: EXIF orientation applied,
    /// then the user's turns and crop. The raw columns would answer the wrong question -
    /// the panel prints these under "which of these is the big one?", and an upright phone
    /// photo's columns are its sensor's landscape ones.
    pub width: u32,
    pub height: u32,
}

/// The columns [`shown_copy`] reads, in its order. Shared so the two queries that build an
/// `ItemCopy` cannot drift into selecting different things.
pub(super) const COPY_COLUMNS: &str = "o.id, o.path, o.width, o.height, o.orientation, o.edit_turns, \
                            o.edit_crop";

/// One `ItemCopy` from a row of [`COPY_COLUMNS`], sized as the photo is displayed.
pub(super) fn shown_copy(r: &rusqlite::Row<'_>) -> rusqlite::Result<ItemCopy> {
    let (width, height): (u32, u32) = (r.get(2)?, r.get(3)?);
    let (upright_w, upright_h) = crate::metadata::oriented_dims(width, height, r.get(4)?);
    let edit = super::items::edit_from_db(r.get(5)?, r.get(6)?);
    let (width, height) = edit.dims(upright_w, upright_h);
    Ok(ItemCopy {
        id: r.get(0)?,
        path: r.get(1)?,
        width,
        height,
    })
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
pub(crate) const DUPLICATE_FILTER: &str = concat!("AND i.id IN (", duplicate_ids!(), ")");

/// The ids of every photo with a copy: the body of [`DUPLICATE_FILTER`], and of the grid's
/// `has_copies` column (`items::GRID_COLUMNS`). A macro because both are `const` strings
/// built with `concat!`, which takes literals only; one text is what keeps a tile's mark and
/// the Duplicates view from disagreeing about which photos have copies.
macro_rules! duplicate_ids {
    () => {
        "SELECT id FROM items WHERE hidden = 0 AND content_hash IN (
        SELECT content_hash FROM items
        WHERE content_hash IS NOT NULL AND missing_since IS NULL AND hidden = 0
        GROUP BY content_hash HAVING COUNT(*) > 1)
    UNION ALL
    SELECT id FROM items WHERE similar_group IS NOT NULL AND missing_since IS NULL
      AND hidden = 0 AND similar_group IN (
        SELECT similar_group FROM items
        WHERE similar_group IS NOT NULL AND missing_since IS NULL AND hidden = 0
        GROUP BY similar_group HAVING COUNT(*) > 1)"
    };
}
pub(crate) use duplicate_ids;

/// One photo and its copies, as a grid filter: `?1` is the photo's id and `?2` the hash it
/// had when the view opened ([`CopiesArg`]). The same two relations `copies_of` and
/// `similar_of` list for the info panel, so the view and the panel cannot disagree about
/// what a copy is.
///
/// The frozen hash is used **only once the photo's row is gone**. While the row exists its
/// own current hash decides, even when that is NULL: a file rewritten with new bytes has its
/// hash cleared, and the twins of its old bytes are not copies of it any more.
///
/// `=`, never `IS`: a NULL hash names no group, and under `IS` every unhashed photo would be
/// a copy of every other. `UNION ALL` inside an `IN` for the reason `DUPLICATE_FILTER` gives:
/// an `OR` of the three plans as a scan of every live row. Missing rows are dropped by
/// `grid_query`'s own `missing_since IS NULL`.
pub(crate) const COPIES_FILTER: &str = "AND i.id IN (
    SELECT ?1
    UNION ALL
    SELECT c.id FROM items c WHERE c.content_hash =
        CASE WHEN EXISTS (SELECT 1 FROM items WHERE id = ?1)
             THEN (SELECT content_hash FROM items WHERE id = ?1)
             ELSE ?2 END
    UNION ALL
    SELECT c.id FROM items a JOIN items c ON c.similar_group = a.similar_group
    WHERE a.id = ?1)";

/// The Copies view's argument: the photo's id, and its content hash as it was when the
/// view opened. The hash is carried because every other way to its twins goes through the
/// photo's own row, and the commonest thing to do from this view - delete the copy you
/// opened it on - purges that row at the next scan. `None` for a photo not hashed yet.
///
/// Held as text in `ViewState.arg`, like every view argument: `"<id>"` or
/// `"<id>:<32 hex digits>"`. Anything else parses as `None`, which shows an empty grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopiesArg {
    pub anchor: i64,
    pub hash: Option<[u8; 16]>,
}

impl CopiesArg {
    pub fn parse(arg: &str) -> Option<Self> {
        let (anchor, hash) = match arg.split_once(':') {
            Some((anchor, hex)) => (anchor, Some(parse_hash(hex)?)),
            None => (arg, None),
        };
        Some(Self {
            anchor: anchor.parse().ok()?,
            hash,
        })
    }
}

impl std::fmt::Display for CopiesArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.anchor)?;
        if let Some(hash) = self.hash {
            f.write_str(":")?;
            for byte in hash {
                write!(f, "{byte:02x}")?;
            }
        }
        Ok(())
    }
}

/// 32 hex digits to 16 bytes. ASCII is checked first because the slicing below is by byte:
/// multi-byte text of the right length would otherwise split a character and panic.
fn parse_hash(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

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
fn copies_sql() -> String {
    format!(
        "SELECT {COPY_COLUMNS} FROM items i
     JOIN items o ON o.content_hash = i.content_hash AND o.id <> i.id
     WHERE i.id = ?1 AND o.missing_since IS NULL AND o.hidden = 0
     ORDER BY o.path"
    )
}

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

    /// The argument that opens the Copies view on `item_id`, with the photo's hash frozen
    /// into it (see [`CopiesArg`]). A photo with no row gets the bare id, which shows an
    /// empty grid rather than refusing the view.
    pub fn copies_view_arg(&self, item_id: i64) -> Result<String> {
        let hash: Option<Vec<u8>> = self
            .reader()?
            .query_row(
                "SELECT content_hash FROM items WHERE id = ?1",
                [item_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(CopiesArg {
            anchor: item_id,
            hash: hash.and_then(|h| h.try_into().ok()),
        }
        .to_string())
    }

    /// The other live files with the same bytes as `item_id`, by path.
    pub fn copies_of(&self, item_id: i64) -> Result<Vec<ItemCopy>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(&copies_sql())?;
        let rows = stmt
            .query_map([item_id], shown_copy)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GridView;
    use crate::library::items::{GRID_COLUMNS, Shown, grid_query};
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
        let plan = plan(&lib, &copies_sql(), &[&1i64]);
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
        let plan = plan(
            &lib,
            &grid_query(GRID_COLUMNS, Shown::Visible, DUPLICATE_FILTER),
            &[],
        );
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

    /// Hashes a row as the pass would: `new_item` fixes size/mtime at 100/1000, so the
    /// candidate names that fingerprint.
    fn hash(lib: &Library, id: i64, path: &str, hash: u8) {
        let candidate = HashCandidate {
            id,
            path: path.to_string(),
            size: 100,
            mtime_ms: 1_000,
        };
        assert!(lib.set_content_hash(&candidate, &[hash; 16]).unwrap());
    }

    fn copies_view(lib: &Library, anchor: i64) -> Vec<i64> {
        let arg = lib.copies_view_arg(anchor).unwrap();
        copies_view_of(lib, &arg)
    }

    /// The view as the engine builds it, from an argument taken earlier - which is the
    /// whole point of the argument carrying a hash.
    fn copies_view_of(lib: &Library, arg: &str) -> Vec<i64> {
        let mut ids: Vec<i64> = lib
            .entries_for(GridView::Copies, arg)
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        ids.sort();
        ids
    }

    /// The view is the photo plus exactly what its info panel lists: the same bytes, the
    /// same look-alike group, live rows only. The unhashed pair is the input that tells
    /// `=` from `IS`: two NULL hashes are "equal" under `IS`, and every unhashed photo in
    /// the library would be shown as a copy of every other.
    #[test]
    fn the_copies_view_holds_the_photo_its_twins_and_its_look_alikes() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let paths = [
            "/p/anchor.jpg",
            "/p/twin.jpg",
            "/p/alike.jpg",
            "/p/other-twin-pair-a.jpg",
            "/p/missing-twin.jpg",
            "/p/unhashed-a.jpg",
            "/p/unhashed-b.jpg",
        ];
        let ids = lib
            .insert_items(
                &paths
                    .iter()
                    .enumerate()
                    .map(|(n, p)| new_item(folder, p, n as i64))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let [anchor, twin, alike, other, missing, unhashed_a, _unhashed_b] = ids[..] else {
            unreachable!()
        };
        hash(&lib, anchor, paths[0], 1);
        hash(&lib, twin, paths[1], 1);
        hash(&lib, missing, paths[4], 1);
        // A different hash: identical to nothing here, so not a copy of the anchor.
        hash(&lib, other, paths[3], 2);
        lib.set_similar_groups(&[(anchor, anchor), (alike, anchor)])
            .unwrap();
        lib.mark_missing(&[missing], 5_000).unwrap();

        assert_eq!(copies_view(&lib, anchor), vec![anchor, twin, alike]);
        assert_eq!(
            copies_view(&lib, unhashed_a),
            vec![unhashed_a],
            "an unhashed photo is a copy of nothing, least of all every other unhashed photo"
        );
    }

    /// The tile mark: a grid row says whether its photo has a copy, by the same rule the
    /// Duplicates view uses, so a marked tile is exactly one whose menu offers "Show
    /// duplicates". A lone hash, a twin that has gone missing and a photo never hashed are
    /// the three ways to have a hash, or a group, and still no copy.
    #[test]
    fn grid_rows_carry_whether_the_photo_has_a_copy() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let paths = [
            "/p/twin-a.jpg",
            "/p/twin-b.jpg",
            "/p/alike-a.jpg",
            "/p/alike-b.jpg",
            "/p/lone-hash.jpg",
            "/p/survivor.jpg",
            "/p/gone-twin.jpg",
            "/p/unhashed.jpg",
        ];
        let ids = lib
            .insert_items(
                &paths
                    .iter()
                    .enumerate()
                    .map(|(n, p)| new_item(folder, p, n as i64))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        hash(&lib, ids[0], paths[0], 1);
        hash(&lib, ids[1], paths[1], 1);
        lib.set_similar_groups(&[(ids[2], ids[2]), (ids[3], ids[2])])
            .unwrap();
        hash(&lib, ids[4], paths[4], 2);
        hash(&lib, ids[5], paths[5], 3);
        hash(&lib, ids[6], paths[6], 3);
        lib.mark_missing(&[ids[6]], 5_000).unwrap();

        let marked = |view: GridView, arg: &str| -> Vec<(i64, bool)> {
            let mut rows: Vec<(i64, bool)> = lib
                .entries_for(view, arg)
                .unwrap()
                .iter()
                .map(|e| (e.id, e.has_copies))
                .collect();
            rows.sort();
            rows
        };
        let expected: Vec<(i64, bool)> = vec![
            (ids[0], true),
            (ids[1], true),
            (ids[2], true),
            (ids[3], true),
            (ids[4], false),
            (ids[5], false),
            (ids[7], false),
        ];
        assert_eq!(marked(GridView::All, ""), expected);
        // Recent and Search build their rows from their own queries; both select the same
        // column prefix, and a view that dropped the column would mark nothing.
        assert_eq!(marked(GridView::Recent, ""), expected);
        assert_eq!(marked(GridView::Search, "twin-a"), vec![(ids[0], true)]);
    }

    /// An argument that names no photo gives an empty grid, not an error: an error rolls the
    /// view back (`rebuild_or_restore`), and a stale id is an ordinary thing to hold.
    #[test]
    fn a_copies_argument_that_is_not_an_id_shows_nothing() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        for arg in [
            "",
            "x",
            "1:",
            "1:zz",
            "1:00",
            "x:00000000000000000000000000000000",
        ] {
            assert!(
                lib.entries_for(GridView::Copies, arg).unwrap().is_empty(),
                "{arg:?}"
            );
        }
    }

    #[test]
    fn a_copies_argument_round_trips_with_and_without_a_hash() {
        let hashed = CopiesArg {
            anchor: 7,
            hash: Some([0xab; 16]),
        };
        assert_eq!(hashed.to_string(), format!("7:{}", "ab".repeat(16)));
        assert_eq!(CopiesArg::parse(&hashed.to_string()), Some(hashed));
        let bare = CopiesArg {
            anchor: 7,
            hash: None,
        };
        assert_eq!(bare.to_string(), "7");
        assert_eq!(CopiesArg::parse("7"), Some(bare));
        // Multi-byte text of the right byte length must be refused, not sliced mid-character:
        // the leading "a" puts every "é" across a two-byte slice boundary.
        assert_eq!(CopiesArg::parse(&format!("7:a{}a", "é".repeat(15))), None);
    }

    /// The case the frozen hash exists for: the user deletes the photo they opened the view
    /// on, and once the scan purges its row, its twins are still each other's copies. The
    /// look-alike half cannot be frozen - a group's id is its smallest member's, so purging
    /// that member renumbers the group at the next pass - and drops out.
    #[test]
    fn the_copies_view_keeps_the_twins_of_a_purged_photo() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let paths = ["/p/anchor.jpg", "/p/twin.jpg", "/p/alike.jpg"];
        let ids = lib
            .insert_items(&[
                new_item(folder, paths[0], 1),
                new_item(folder, paths[1], 2),
                new_item(folder, paths[2], 3),
            ])
            .unwrap();
        let [anchor, twin, alike] = ids[..] else {
            unreachable!()
        };
        hash(&lib, anchor, paths[0], 1);
        hash(&lib, twin, paths[1], 1);
        lib.set_similar_groups(&[(anchor, anchor), (alike, anchor)])
            .unwrap();
        let arg = lib.copies_view_arg(anchor).unwrap();

        lib.purge_items(&[anchor]).unwrap();
        assert_eq!(copies_view_of(&lib, &arg), vec![twin]);
    }

    /// While the photo is indexed its *current* hash decides, not the one the view opened
    /// with: a file rewritten with new bytes has its hash cleared (`update_items`), and its
    /// old twins are no longer copies of it.
    #[test]
    fn the_photos_own_hash_wins_while_it_is_indexed() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let paths = ["/p/anchor.jpg", "/p/twin.jpg"];
        let ids = lib
            .insert_items(&[new_item(folder, paths[0], 1), new_item(folder, paths[1], 2)])
            .unwrap();
        hash(&lib, ids[0], paths[0], 1);
        hash(&lib, ids[1], paths[1], 1);
        let arg = lib.copies_view_arg(ids[0]).unwrap();

        lib.writer()
            .execute(
                "UPDATE items SET content_hash = NULL WHERE id = ?1",
                [ids[0]],
            )
            .unwrap();
        assert_eq!(copies_view_of(&lib, &arg), vec![ids[0]]);
    }

    /// The Copies analogue of `the_duplicates_view_places_a_folder_by_its_oldest_matching_photo`:
    /// the filter has to reach `folder_order`'s copy as well as the outer `WHERE`.
    #[test]
    fn the_copies_view_places_a_folder_by_its_oldest_matching_photo() {
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
                new_item(alpha, "/p/alpha/old-unrelated.jpg", 1),
                new_item(alpha, "/p/alpha/anchor.jpg", 9),
                new_item(zulu, "/p/zulu/alike.jpg", 5),
            ])
            .unwrap();
        lib.set_similar_groups(&[(ids[1], ids[1]), (ids[2], ids[1])])
            .unwrap();

        let folders: Vec<i64> = lib
            .entries_for(GridView::Copies, &ids[1].to_string())
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

    #[test]
    fn the_copies_view_reaches_both_halves_through_their_indexes() {
        let (_dir, lib) = temp_library();
        let plan = plan(
            &lib,
            &grid_query(GRID_COLUMNS, Shown::Visible, COPIES_FILTER),
            &[&1i64, &vec![7u8; 16]],
        );
        for index in ["items_content_hash", "items_similar_group"] {
            assert!(
                plan.iter().any(|step| step.contains(index)),
                "expected {index}, got {plan:?}"
            );
        }
    }
}
