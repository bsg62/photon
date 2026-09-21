//! The queries behind look-alike photos. The hashing and grouping are `crate::similar`;
//! this is only what they read and write, and what the UI asks afterwards.

use super::Library;
use super::duplicates::ItemCopy;
use super::items::edit_from_db;
use crate::Result;
use crate::media::fingerprint;
use rusqlite::params;

/// A photo whose thumbnail exists but whose perceptual hash does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimilarCandidate {
    pub id: i64,
    /// The cache key of the grid thumbnail to hash - `Item::thumb_key()`'s value.
    pub thumb_key: u64,
    /// The fingerprint the hash will be stored against; see [`Library::set_percep_hash`].
    pub size: i64,
    pub mtime_ms: i64,
}

/// Live, thumbnailed, unhashed rows - rare once a library has settled, since a photo takes
/// this path exactly once (a scan clears `percep_hash` only by resetting the row itself).
const CANDIDATES_SQL: &str = "SELECT i.id, i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
     FROM items i
     JOIN folders f ON f.id = i.folder_id
     JOIN watched_folders w ON w.id = f.watched_id
     WHERE i.missing_since IS NULL AND i.percep_hash IS NULL
       AND i.thumb_state = 1 AND w.online = 1
     ORDER BY i.id";

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
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Stores a perceptual hash, but only against the fingerprint it was computed for -
    /// the same guard [`Library::set_content_hash`] has, and for the same reason: the pass
    /// reads the thumbnail long after the row was listed, and a hash of the old picture must
    /// not land on a row whose file has since moved on.
    pub fn set_percep_hash(&self, candidate: &SimilarCandidate, hash: u64) -> Result<bool> {
        let changed = self.writer().execute(
            "UPDATE items SET percep_hash = ?2
             WHERE id = ?1 AND size = ?3 AND mtime_ms = ?4 AND missing_since IS NULL",
            params![
                candidate.id,
                hash as i64,
                candidate.size,
                candidate.mtime_ms
            ],
        )?;
        Ok(changed == 1)
    }

    /// Every live photo's perceptual hash. One integer per photo, so a 100k library is
    /// 1.6 MB - which is what makes grouping in Rust affordable and an SQL index pointless.
    pub fn percep_hashes(&self) -> Result<Vec<(i64, u64)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, percep_hash FROM items
             WHERE percep_hash IS NOT NULL AND missing_since IS NULL",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let hash: i64 = r.get(1)?;
                Ok((r.get::<_, i64>(0)?, hash as u64))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Replaces every group in one transaction.
    ///
    /// Wholesale rather than incremental: the pass recomputes the whole library, and
    /// clearing first is what lets a group *shrink* - a photo that no longer resembles
    /// anything must lose its group, and an UPDATE of only the new members would leave it
    /// pointing at a group it is no longer in.
    pub fn set_similar_groups(&self, groups: &[(i64, i64)]) -> Result<()> {
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
        Ok(())
    }

    /// The other live photos that look like `item_id`, by path.
    pub fn similar_of(&self, item_id: i64) -> Result<Vec<ItemCopy>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT o.id, o.path, o.width, o.height FROM items i
             JOIN items o ON o.similar_group = i.similar_group AND o.id <> i.id
             WHERE i.id = ?1 AND i.similar_group IS NOT NULL AND o.missing_since IS NULL
             ORDER BY o.path",
        )?;
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
    use crate::library::NewItem;
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
        assert_eq!(lib.percep_hashes().unwrap(), vec![(ids[0], 0xdead_beef)]);
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
}
