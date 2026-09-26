//! The queries behind look-alike photos. The hashing and grouping are `crate::similar`;
//! this is only what they read and write, and what the UI asks afterwards.

use super::Library;
use super::duplicates::{COPY_COLUMNS, ItemCopy, shown_copy};
use super::items::edit_from_db;
use crate::Result;
use crate::edit::{Crop, Edit};
use crate::media::fingerprint;
use rusqlite::params;

/// A photo with a perceptual hash; see [`Library::percep_hashes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashedPhoto {
    pub id: i64,
    pub hash: u64,
    /// The cache key of the grid thumbnail the hash was taken from - `Item::thumb_key()`'s
    /// value.
    pub thumb_key: u64,
}

/// A photo whose thumbnail exists but whose perceptual hash does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimilarCandidate {
    pub id: i64,
    /// The cache key of the grid thumbnail to hash - `Item::thumb_key()`'s value.
    pub thumb_key: u64,
    /// The fingerprint the hash will be stored against; see [`Library::set_percep_hash`].
    pub size: i64,
    pub mtime_ms: i64,
    /// The edit the hash will be stored against - part of the same guard, because a
    /// perceptual hash describes the photo *as shown*; see [`Library::set_percep_hash`].
    pub edit: Edit,
}

/// Live, thumbnailed, unhashed rows - rare once a library has settled, since a photo takes
/// this path once per picture: `update_items` clears `percep_hash` when the file behind the
/// row changes, and `set_item_edit` clears it when the user changes what photon shows.
const CANDIDATES_SQL: &str = "SELECT i.id, i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
     FROM items i
     JOIN folders f ON f.id = i.folder_id
     JOIN watched_folders w ON w.id = f.watched_id
     WHERE i.missing_since IS NULL AND i.percep_hash IS NULL
       AND i.thumb_state = 1 AND w.online = 1
       -- A poster frame is not the video; it would pair with the still taken beside it.
       AND i.kind = 0
     ORDER BY i.id";

/// `group`'s output follows `percep_hashes()`'s unordered query (effectively rowid), not
/// hash order, and the stored rows are in index order, so neither side of the comparison
/// can be trusted to arrive sorted.
fn sorted(pairs: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut out = pairs.to_vec();
    out.sort_unstable();
    out
}

impl Library {
    /// Live photos whose thumbnail is ready but which have no perceptual hash yet.
    ///
    /// `thumb_state = 1` (Ready) is the point: the hash is taken from the cached 256px grid
    /// thumbnail, so a photo whose thumbnail has not been rendered has nothing to hash. It
    /// becomes a candidate as soon as it does, which is why an existing library fills in
    /// without anything being re-decoded.
    pub fn similar_candidates(&self) -> Result<Vec<SimilarCandidate>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(CANDIDATES_SQL)?;
        let rows = stmt
            .query_map([], |r| {
                let path: String = r.get(1)?;
                let size: i64 = r.get(2)?;
                let mtime_ms: i64 = r.get(3)?;
                let turns: i64 = r.get(4)?;
                let crop: Option<i64> = r.get(5)?;
                let edit = edit_from_db(turns, crop);
                Ok(SimilarCandidate {
                    id: r.get(0)?,
                    thumb_key: edit.thumb_key(fingerprint(&path, size, mtime_ms)),
                    size,
                    mtime_ms,
                    edit,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Stores a perceptual hash, but only against the picture it was computed for - the
    /// fingerprint **and the edit**, because the pass reads the thumbnail long after the row
    /// was listed and a hash of the old picture must not land on a row that has since moved
    /// on.
    ///
    /// The edit is where this parts company with [`Library::set_content_hash`], whose guard
    /// it otherwise mirrors. A content hash is a fact about the *file*, so size and mtime
    /// are the whole of it. A perceptual hash is a fact about the photo **as shown**, and
    /// `set_item_edit` changes what is shown without touching either: a turn landing between
    /// the listing and this write leaves every column `set_content_hash` checks unchanged,
    /// while the thumbnail just hashed - still on disk, since the GC has not run and keys
    /// recur by design - is of the *unturned* picture. `dhash` is deliberately not
    /// rotation-invariant, so the row would take the hash of a picture photon no longer
    /// shows, group with the wrong photos, and - `percep_hash` being non-NULL - never be a
    /// candidate again. This is `set_thumb_state_if_unchanged`'s hazard exactly, and it
    /// takes the same two extra columns to close.
    pub fn set_percep_hash(&self, candidate: &SimilarCandidate, hash: u64) -> Result<bool> {
        let changed = self.writer().execute(
            "UPDATE items SET percep_hash = ?2
             WHERE id = ?1 AND size = ?3 AND mtime_ms = ?4 AND missing_since IS NULL
               AND edit_turns = ?5 AND edit_crop IS ?6",
            params![
                candidate.id,
                hash as i64,
                candidate.size,
                candidate.mtime_ms,
                candidate.edit.turns,
                candidate.edit.crop.map(Crop::to_db),
            ],
        )?;
        Ok(changed == 1)
    }

    /// Every live photo's perceptual hash, with the key of the thumbnail it was taken from.
    /// A few integers per photo, so a 100k library is a few MB - which is what makes
    /// grouping in Rust affordable and an SQL index pointless.
    ///
    /// The thumbnail key is here because a hash only nominates a pair: `similar::update`
    /// confirms it against the two thumbnails themselves, and it must read the thumbnail of
    /// the picture the row shows now. `set_percep_hash` only stores a hash against the
    /// fingerprint and edit it was taken for, and both writers that move either clear the
    /// hash, so a row with a hash has a key describing the same picture.
    pub fn percep_hashes(&self) -> Result<Vec<HashedPhoto>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, percep_hash, path, size, mtime_ms, edit_turns, edit_crop FROM items
             WHERE percep_hash IS NOT NULL AND missing_since IS NULL
               -- A poster frame is not the video; it would pair with the still taken beside it.
               AND kind = 0",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let hash: i64 = r.get(1)?;
                let path: String = r.get(2)?;
                let edit = edit_from_db(r.get(5)?, r.get(6)?);
                Ok(HashedPhoto {
                    id: r.get(0)?,
                    hash: hash as u64,
                    thumb_key: edit.thumb_key(fingerprint(&path, r.get(3)?, r.get(4)?)),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Replaces every group in one transaction, and says whether that changed anything.
    ///
    /// Wholesale rather than incremental: the pass recomputes the whole library, and
    /// clearing first is what lets a group *shrink* - a photo that no longer resembles
    /// anything must lose its group, and an UPDATE of only the new members would leave it
    /// pointing at a group it is no longer in.
    ///
    /// The comparison first is not a micro-optimisation. This runs at the end of every
    /// scan, including the watcher's subtree scans two seconds after a single file lands,
    /// and almost every one of those recomputes exactly the groups already stored; without
    /// it, each writes every grouped row again for no change. It is affordable because both
    /// the read and the write it replaces are bounded by the grouped rows rather than by
    /// the library - `items_similar_group` is a partial index over just those.
    ///
    /// The answer is also what tells the caller whether the grid needs rebuilding: a
    /// regroup that hashes nothing can still move photos into or out of the Duplicates
    /// view, which is what changing the distance setting does.
    pub fn set_similar_groups(&self, groups: &[(i64, i64)]) -> Result<bool> {
        if self.similar_groups()? == sorted(groups) {
            return Ok(false);
        }
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE items SET similar_group = NULL WHERE similar_group IS NOT NULL",
            [],
        )?;
        {
            let mut stmt =
                tx.prepare_cached("UPDATE items SET similar_group = ?2 WHERE id = ?1")?;
            for (id, group) in groups {
                stmt.execute(params![id, group])?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    /// Every stored `(item, group)` pair, sorted, for comparison with a freshly computed
    /// set. Served by `items_similar_group`, so it reads the grouped rows and not the rest.
    ///
    /// `pub` because it is also the only way to see what a pass *left behind*, which is
    /// what `photon-app`'s tests about requesting a pass assert on: the Duplicates view is
    /// kept honest by `DUPLICATE_FILTER` whether or not a pass ran, so the view cannot tell
    /// the two apart and the column can.
    pub fn similar_groups(&self) -> Result<Vec<(i64, i64)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, similar_group FROM items WHERE similar_group IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(sorted(&rows))
    }

    /// The other live photos that look like `item_id`, by path.
    pub fn similar_of(&self, item_id: i64) -> Result<Vec<ItemCopy>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {COPY_COLUMNS} FROM items i
             JOIN items o ON o.similar_group = i.similar_group AND o.id <> i.id
             WHERE i.id = ?1 AND i.similar_group IS NOT NULL AND o.missing_since IS NULL
               AND i.hidden = 0 AND o.hidden = 0
             ORDER BY o.path"
        ))?;
        let rows = stmt
            .query_map([item_id], shown_copy)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::CANDIDATES_SQL;
    use crate::library::NewItem;
    use crate::media::MediaKind;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    /// `new_item` fixes size/mtime at 100/1000; these tests are specifically about size and
    /// mtime as the fingerprint's inputs, so they override them by name rather than by
    /// position - the same struct-update pattern `tags.rs`'s tests already use.
    fn item_at(folder: i64, path: &str, size: i64, mtime_ms: i64) -> NewItem {
        NewItem {
            size,
            mtime_ms,
            ..new_item(folder, path, 0)
        }
    }

    #[test]
    fn a_photo_with_no_thumbnail_is_not_a_candidate() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        lib.insert_items(&[item_at(folder, "/pics/a.jpg", 10, 100)])
            .unwrap();
        // thumb_state defaults to Pending, so nothing is ready to hash.
        assert!(lib.similar_candidates().unwrap().is_empty());
    }

    #[test]
    fn a_video_is_never_a_look_alike_candidate() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let video = NewItem {
            kind: MediaKind::Video,
            ..item_at(folder, "/pics/clip.mp4", 20, 100)
        };
        let ids = lib
            .insert_items(&[item_at(folder, "/pics/a.jpg", 10, 100), video])
            .unwrap();
        lib.writer()
            .execute("UPDATE items SET thumb_state = 1", [])
            .unwrap();
        let candidates: Vec<i64> = lib
            .similar_candidates()
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(candidates, [ids[0]], "a poster frame is not the video");
        // A hash already stored against a video (a library from a build before this rule) is
        // never compared either.
        lib.writer()
            .execute("UPDATE items SET percep_hash = 1 WHERE id = ?1", [ids[1]])
            .unwrap();
        assert!(lib.percep_hashes().unwrap().iter().all(|h| h.id != ids[1]));
    }

    #[test]
    fn a_ready_photo_without_a_hash_is_a_candidate_and_takes_one() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[item_at(folder, "/pics/a.jpg", 10, 100)])
            .unwrap();
        lib.writer()
            .execute("UPDATE items SET thumb_state = 1 WHERE id = ?1", [ids[0]])
            .unwrap();

        let candidates = lib.similar_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(lib.set_percep_hash(&candidates[0], 0xdead_beef).unwrap());

        assert!(
            lib.similar_candidates().unwrap().is_empty(),
            "still a candidate after hashing"
        );
        assert_eq!(
            lib.percep_hashes()
                .unwrap()
                .iter()
                .map(|p| (p.id, p.hash))
                .collect::<Vec<_>>(),
            vec![(ids[0], 0xdead_beef)]
        );
    }

    /// SQLite has no unsigned integer column; `percep_hash` is stored as `i64` and cast at
    /// the boundary. `0xdead_beef` alone never exercises the top bit, and a `dhash` sets it
    /// about half the time - a lossy round-trip here would show up as photos silently
    /// grouped by the wrong (truncated or sign-flipped) value on every other library.
    #[test]
    fn the_full_width_of_a_hash_survives_the_i64_column() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[item_at(folder, "/pics/a.jpg", 10, 100)])
            .unwrap();
        lib.writer()
            .execute("UPDATE items SET thumb_state = 1 WHERE id = ?1", [ids[0]])
            .unwrap();
        let candidate = lib.similar_candidates().unwrap().remove(0);
        assert!(
            lib.set_percep_hash(&candidate, 0xffff_ffff_ffff_ffff)
                .unwrap()
        );
        assert_eq!(
            lib.percep_hashes()
                .unwrap()
                .iter()
                .map(|p| (p.id, p.hash))
                .collect::<Vec<_>>(),
            vec![(ids[0], 0xffff_ffff_ffff_ffff)]
        );
    }

    /// The same guard `set_content_hash` has: the pass reads long after the row was listed,
    /// and a hash computed from the old thumbnail must not land on a row whose file moved.
    #[test]
    fn a_hash_is_refused_once_the_row_has_moved_on() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[item_at(folder, "/pics/a.jpg", 10, 100)])
            .unwrap();
        lib.writer()
            .execute("UPDATE items SET thumb_state = 1 WHERE id = ?1", [ids[0]])
            .unwrap();
        let candidate = lib.similar_candidates().unwrap().remove(0);

        lib.update_items(&[(ids[0], item_at(folder, "/pics/a.jpg", 20, 200))])
            .unwrap();
        assert!(!lib.set_percep_hash(&candidate, 0xdead_beef).unwrap());
    }

    #[test]
    fn groups_are_replaced_wholesale() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[
                item_at(folder, "/pics/a.jpg", 10, 100),
                item_at(folder, "/pics/b.jpg", 11, 101),
            ])
            .unwrap();
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])])
            .unwrap();
        assert_eq!(lib.similar_of(ids[0]).unwrap().len(), 1);

        // A later pass finds nothing similar: the old groups must go, not linger.
        lib.set_similar_groups(&[]).unwrap();
        assert!(lib.similar_of(ids[0]).unwrap().is_empty());
    }

    /// The signal the grid refresh is gated on, and what keeps the watcher's two-second
    /// subtree scans from rewriting every grouped row for no change: a regroup that
    /// computes what is already stored must write nothing and say so.
    #[test]
    fn a_regroup_that_changes_nothing_writes_nothing() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[
                item_at(folder, "/pics/a.jpg", 10, 100),
                item_at(folder, "/pics/b.jpg", 11, 101),
            ])
            .unwrap();
        let groups = [(ids[0], ids[0]), (ids[1], ids[0])];

        assert!(lib.set_similar_groups(&groups).unwrap());
        let before = lib.writer().total_changes();

        // The same set, handed over in the other order: `group`'s output follows
        // `percep_hashes()`'s query order, not hash order, so the comparison cannot lean on
        // the two sides arriving sorted.
        assert!(!lib.set_similar_groups(&[groups[1], groups[0]]).unwrap());
        assert_eq!(lib.writer().total_changes(), before, "it wrote anyway");
        assert_eq!(
            lib.similar_of(ids[0]).unwrap().len(),
            1,
            "the group is gone"
        );

        // And a real change is still a change, in both directions.
        assert!(lib.set_similar_groups(&[]).unwrap());
        assert!(!lib.set_similar_groups(&[]).unwrap());
    }

    #[test]
    fn the_duplicates_view_counts_look_alikes_too() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[
                item_at(folder, "/pics/a.jpg", 10, 100),
                item_at(folder, "/pics/b.jpg", 11, 101),
            ])
            .unwrap();
        assert_eq!(lib.duplicate_count().unwrap(), 0);
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])])
            .unwrap();
        assert_eq!(
            lib.duplicate_count().unwrap(),
            2,
            "look-alikes are not in the view"
        );
    }

    /// Pins that the candidate list is found by probing `items_pending` rather than by
    /// scanning every item and checking `percep_hash IS NULL` row by row - the same drift
    /// `the_recent_view_is_served_by_its_index` guards against for the grid.
    #[test]
    fn the_candidate_list_is_served_by_its_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {CANDIDATES_SQL}"))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|step| step.contains("items_pending")),
            "expected an index probe, got {plan:?}"
        );
        assert!(
            !plan.iter().any(|step| step.contains("SCAN i")),
            "items must be searched, not scanned: {plan:?}"
        );
    }

    /// A perceptual hash is a fact about the photo *as shown*, so an edit landing between
    /// the listing and the write must refuse the hash - and an edit moves no column
    /// `set_content_hash`'s guard checks. The first pass after an upgrade lists the whole
    /// library and runs for minutes, so "the user turned a photo while the pass was
    /// running" is the ordinary case, not a contrived one. Taken, the row would carry the
    /// hash of its unturned self forever: `dhash` is not rotation-invariant, and a non-NULL
    /// `percep_hash` is never a candidate again.
    #[test]
    fn an_edit_between_listing_and_writing_refuses_the_hash() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[item_at(folder, "/pics/a.jpg", 10, 100)])
            .unwrap();
        lib.writer()
            .execute("UPDATE items SET thumb_state = 1 WHERE id = ?1", [ids[0]])
            .unwrap();
        let candidate = lib.similar_candidates().unwrap().remove(0);

        // The user presses `R`: the picture changes, the file does not.
        assert!(
            lib.set_item_edit(
                ids[0],
                crate::edit::Edit {
                    turns: 1,
                    crop: None
                }
            )
            .unwrap()
        );

        assert!(
            !lib.set_percep_hash(&candidate, 0xdead_beef).unwrap(),
            "a hash of the pre-edit thumbnail landed on the edited row"
        );
        assert!(
            lib.percep_hashes().unwrap().is_empty(),
            "and nothing was written"
        );
    }

    /// The panel prints a look-alike's dimensions under "which of these is the big one?",
    /// so they have to be the photo's **as shown**, like `ViewerItem`'s - EXIF orientation
    /// applied, then the user's turns and crop. Read straight from the columns, an upright
    /// phone photo answers with its sensor's landscape size and a cropped copy with the
    /// frame it was cropped out of, which is the opposite of what the question asks.
    #[test]
    fn a_look_alikes_dimensions_are_the_ones_on_screen() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        // 400x300 on the sensor, orientation 6: a quarter turn, so 300x400 upright.
        let upright = NewItem {
            orientation: 6,
            ..item_at(folder, "/pics/upright.jpg", 10, 100)
        };
        let ids = lib
            .insert_items(&[item_at(folder, "/pics/a.jpg", 11, 101), upright])
            .unwrap();
        // ...and then cropped to its left half: 150x400 on screen.
        lib.set_item_edit(
            ids[1],
            crate::edit::Edit {
                turns: 0,
                crop: Some(crate::edit::Crop {
                    left: 0,
                    top: 0,
                    right: 32768,
                    bottom: 65535,
                }),
            },
        )
        .unwrap();
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])])
            .unwrap();

        let alike = lib.similar_of(ids[0]).unwrap();
        assert_eq!(alike.len(), 1);
        assert_eq!(
            (alike[0].width, alike[0].height),
            (150, 400),
            "the raw columns (400x300) are neither turned nor cropped"
        );
    }
}
