//! The faces photon found itself (`face_detect`), and which photos it has looked at. The
//! detecting is `crate::face_detect`; this is what it reads and writes.

use super::Library;
use super::items::edit_from_db;
use super::settings::face_detection_on;
use crate::Result;
use crate::edit::{Crop, Edit};
use crate::face_detect::{Detection, Rect};
use crate::media::fingerprint;
use rusqlite::params;

/// A photo the detector has not looked at, or that an older detector did.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceCandidate {
    pub id: i64,
    /// The cache key of the preview to read - `Item::thumb_key()`'s value.
    pub thumb_key: u64,
    /// What the detections will be stored against; see [`Library::write_face_batch`].
    pub size: i64,
    pub mtime_ms: i64,
    pub edit: Edit,
}

/// Live images whose thumbnail is ready and whose `face_version` is not the current one,
/// after a given id. `thumb_state = 1` is the point, as it is for the look-alike pass:
/// detection reads the cached preview, so a library from before this feature fills in
/// without a source file being decoded.
///
/// Not `w.online = 1`, unlike that pass's list: the preview is in photon's own cache, so a
/// drive that is unplugged can still be detected.
///
/// The `+` keeps this a walk of the table by id. Without statistics the bare term has
/// SQLite walk a partial index on that predicate instead (`library/mod.rs`).
const CANDIDATES_SQL: &str = "SELECT id, path, size, mtime_ms, edit_turns, edit_crop FROM items
     WHERE id > ?1 AND +missing_since IS NULL AND thumb_state = 1
       -- A poster frame is not the video.
       AND kind = 0
       AND face_version IS NOT ?2
     ORDER BY id LIMIT ?3";

fn landmarks_blob(points: &[(f32, f32); 5]) -> [u8; 40] {
    let mut blob = [0u8; 40];
    for (i, (x, y)) in points.iter().enumerate() {
        blob[i * 8..i * 8 + 4].copy_from_slice(&x.to_le_bytes());
        blob[i * 8 + 4..i * 8 + 8].copy_from_slice(&y.to_le_bytes());
    }
    blob
}

impl Library {
    /// Up to `limit` candidates with an id above `after_id`, in id order. The pass pages
    /// with the last id it read, so a photo it had to skip is not handed back in the same
    /// pass.
    pub fn face_candidates(
        &self,
        after_id: i64,
        limit: usize,
        version: i64,
    ) -> Result<Vec<FaceCandidate>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(CANDIDATES_SQL)?;
        let rows = stmt
            .query_map(params![after_id, version, limit as i64], |r| {
                let path: String = r.get(1)?;
                let size: i64 = r.get(2)?;
                let mtime_ms: i64 = r.get(3)?;
                let edit = edit_from_db(r.get(4)?, r.get(5)?);
                Ok(FaceCandidate {
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

    /// Stores what the detector found, and that it looked, for each photo of the batch.
    /// Returns how many photos were written.
    ///
    /// **Nothing is written with the setting off.** It is read here, inside the
    /// transaction, and writes are one connection: the switch's delete and a batch that was
    /// being detected when the user threw it cannot interleave, so no face lands after
    /// "off".
    ///
    /// **A photo that moved on is skipped,** by the guard `set_percep_hash` uses and for
    /// its reason: the preview was read long after the row was listed, and a file rewritten
    /// or an edit made in between makes these the faces of a picture the row no longer
    /// shows. The row keeps its cleared `face_version` and the next pass looks at the new
    /// picture.
    pub fn write_face_batch(
        &self,
        batch: &[(FaceCandidate, Vec<Detection>)],
        version: i64,
    ) -> Result<usize> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        if !face_detection_on(&tx)? {
            return Ok(0);
        }
        let mut written = 0;
        {
            let mut mark = tx.prepare_cached(
                "UPDATE items SET face_version = ?2
                 WHERE id = ?1 AND size = ?3 AND mtime_ms = ?4 AND missing_since IS NULL
                   AND edit_turns = ?5 AND edit_crop IS ?6",
            )?;
            let mut delete = tx.prepare_cached("DELETE FROM detected_faces WHERE item_id = ?1")?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO detected_faces (item_id, left, top, right, bottom, landmarks, score)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (candidate, faces) in batch {
                let marked = mark.execute(params![
                    candidate.id,
                    version,
                    candidate.size,
                    candidate.mtime_ms,
                    candidate.edit.turns,
                    candidate.edit.crop.map(Crop::to_db),
                ])?;
                if marked != 1 {
                    continue;
                }
                written += 1;
                // An older detector's faces, when the version moved.
                delete.execute(params![candidate.id])?;
                for face in faces {
                    insert.execute(params![
                        candidate.id,
                        face.rect.left,
                        face.rect.top,
                        face.rect.right,
                        face.rect.bottom,
                        landmarks_blob(&face.landmarks).as_slice(),
                        face.score as f64,
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(written)
    }

    /// How many live images the current detector has looked at, and how many there are.
    /// The total includes photos whose thumbnail is not ready yet, so the progress line
    /// can sit below its end while thumbnails are still being made.
    pub fn face_progress(&self, version: i64) -> Result<(u64, u64)> {
        let conn = self.reader()?;
        let (checked, total): (i64, i64) = conn.query_row(
            "SELECT count(*) FILTER (WHERE face_version IS ?1), count(*) FROM items
             WHERE +missing_since IS NULL AND kind = 0",
            params![version],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((checked as u64, total as u64))
    }

    /// The faces photon detected on one photo, in the picture as shown.
    pub fn item_detected_faces(&self, item_id: i64) -> Result<Vec<Rect>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT left, top, right, bottom FROM detected_faces WHERE item_id = ?1 ORDER BY id",
        )?;
        let rects = stmt
            .query_map(params![item_id], |r| {
                Ok(Rect {
                    left: r.get(0)?,
                    top: r.get(1)?,
                    right: r.get(2)?,
                    bottom: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Edit;
    use crate::face_detect::{DETECTOR_VERSION, Detection, Rect};
    use crate::media::ThumbState;
    use crate::testutil::{new_item, seed_folder, temp_library};

    const V: i64 = DETECTOR_VERSION;

    fn face(left: f64) -> Detection {
        Detection {
            rect: Rect {
                left,
                top: 0.2,
                right: left + 0.1,
                bottom: 0.4,
            },
            landmarks: [(0.1, 0.2), (0.3, 0.4), (0.5, 0.6), (0.7, 0.8), (0.9, 1.0)],
            score: 0.9,
        }
    }

    /// A library with the setting on and `names.len()` photos, thumbnails ready.
    fn seeded(names: &[&str]) -> (tempfile::TempDir, Library, Vec<i64>) {
        let (dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, dir.path());
        let items: Vec<_> = names
            .iter()
            .map(|n| new_item(folder, &format!("{}/{n}", dir.path().display()), 1))
            .collect();
        let ids = lib.insert_items(&items).unwrap();
        for id in &ids {
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }
        lib.set_face_detection(true).unwrap();
        (dir, lib, ids)
    }

    fn rows(lib: &Library) -> i64 {
        lib.reader()
            .unwrap()
            .query_row("SELECT count(*) FROM detected_faces", [], |r| r.get(0))
            .unwrap()
    }

    fn version_of(lib: &Library, id: i64) -> Option<i64> {
        lib.reader()
            .unwrap()
            .query_row("SELECT face_version FROM items WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }

    #[test]
    fn a_ready_photo_is_a_candidate_until_it_is_written() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg"]);
        let candidates = lib.face_candidates(0, 10, V).unwrap();
        assert_eq!(candidates.iter().map(|c| c.id).collect::<Vec<_>>(), ids);

        let written = lib
            .write_face_batch(&[(candidates[0].clone(), vec![face(0.1), face(0.5)])], V)
            .unwrap();
        assert_eq!(written, 1);
        assert_eq!(rows(&lib), 2);
        assert_eq!(version_of(&lib, ids[0]), Some(V));
        let left = lib.face_candidates(0, 10, V).unwrap();
        assert_eq!(left.iter().map(|c| c.id).collect::<Vec<_>>(), [ids[1]]);
    }

    /// A photo with no face is written too, or it would be detected on every pass.
    #[test]
    fn a_photo_without_faces_is_marked_as_looked_at() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        assert_eq!(
            lib.write_face_batch(&[(c[0].clone(), vec![])], V).unwrap(),
            1
        );
        assert_eq!(version_of(&lib, ids[0]), Some(V));
        assert!(lib.face_candidates(0, 10, V).unwrap().is_empty());
    }

    /// Each of the candidate query's conditions, by breaking one at a time.
    #[test]
    fn what_is_not_a_candidate() {
        let (_dir, lib, ids) = seeded(&["pending.jpg", "video.mp4", "missing.jpg", "old.jpg"]);
        let w = lib.writer();
        w.execute("UPDATE items SET thumb_state = 0 WHERE id = ?1", [ids[0]])
            .unwrap();
        w.execute("UPDATE items SET kind = 1 WHERE id = ?1", [ids[1]])
            .unwrap();
        w.execute("UPDATE items SET missing_since = 5 WHERE id = ?1", [ids[2]])
            .unwrap();
        w.execute(
            "UPDATE items SET face_version = ?2 WHERE id = ?1",
            [ids[3], V],
        )
        .unwrap();
        drop(w);
        assert!(lib.face_candidates(0, 10, V).unwrap().is_empty());
        // A newer detector looks again at what an older one saw.
        let again = lib.face_candidates(0, 10, V + 1).unwrap();
        assert_eq!(again.iter().map(|c| c.id).collect::<Vec<_>>(), [ids[3]]);
    }

    #[test]
    fn candidates_page_by_id() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg", "c.jpg"]);
        let first = lib.face_candidates(0, 2, V).unwrap();
        assert_eq!(first.iter().map(|c| c.id).collect::<Vec<_>>(), ids[..2]);
        let rest = lib.face_candidates(first[1].id, 2, V).unwrap();
        assert_eq!(rest.iter().map(|c| c.id).collect::<Vec<_>>(), ids[2..]);
    }

    /// The pass reads the preview long after it listed the row. An edit in between makes
    /// what it detected a picture the row no longer shows.
    #[test]
    fn a_batch_is_refused_for_a_photo_edited_since_it_was_listed() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let listed = lib.face_candidates(0, 10, V).unwrap();
        lib.set_item_edit(
            ids[0],
            Edit {
                turns: 1,
                crop: None,
            },
        )
        .unwrap();
        assert_eq!(
            lib.write_face_batch(&[(listed[0].clone(), vec![face(0.1)])], V)
                .unwrap(),
            0
        );
        assert_eq!(rows(&lib), 0);
        assert_eq!(version_of(&lib, ids[0]), None);
    }

    /// As above for the file itself changing.
    #[test]
    fn a_batch_is_refused_for_a_photo_whose_file_changed() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let listed = lib.face_candidates(0, 10, V).unwrap();
        lib.writer()
            .execute(
                "UPDATE items SET mtime_ms = mtime_ms + 1 WHERE id = ?1",
                [ids[0]],
            )
            .unwrap();
        assert_eq!(
            lib.write_face_batch(&[(listed[0].clone(), vec![face(0.1)])], V)
                .unwrap(),
            0
        );
        assert_eq!(rows(&lib), 0);
    }

    /// Off means no face data is kept: a batch that was in flight when the user switched
    /// off writes nothing.
    #[test]
    fn a_batch_writes_nothing_with_the_setting_off() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let listed = lib.face_candidates(0, 10, V).unwrap();
        lib.set_face_detection(false).unwrap();
        assert_eq!(
            lib.write_face_batch(&[(listed[0].clone(), vec![face(0.1)])], V)
                .unwrap(),
            0
        );
        assert_eq!(rows(&lib), 0);
        assert_eq!(version_of(&lib, ids[0]), None);
    }

    #[test]
    fn switching_off_deletes_every_detection() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.1)])], V)
            .unwrap();
        assert!(lib.face_detection().unwrap());

        lib.set_face_detection(false).unwrap();
        assert!(!lib.face_detection().unwrap());
        assert_eq!(rows(&lib), 0);
        assert_eq!(version_of(&lib, ids[0]), None);
    }

    #[test]
    fn the_setting_is_off_until_set() {
        let (_dir, lib) = temp_library();
        assert!(!lib.face_detection().unwrap());
    }

    #[test]
    fn an_edit_clears_the_photos_detections() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(
            &[
                (c[0].clone(), vec![face(0.1)]),
                (c[1].clone(), vec![face(0.2)]),
            ],
            V,
        )
        .unwrap();
        lib.set_item_edit(
            ids[0],
            Edit {
                turns: 1,
                crop: None,
            },
        )
        .unwrap();
        assert!(lib.item_detected_faces(ids[0]).unwrap().is_empty());
        assert_eq!(version_of(&lib, ids[0]), None);
        assert_eq!(
            lib.item_detected_faces(ids[1]).unwrap().len(),
            1,
            "the other photo's stay"
        );
    }

    #[test]
    fn a_rewritten_file_clears_the_photos_detections() {
        let (dir, lib, ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.1)])], V)
            .unwrap();
        let folder: i64 = lib
            .reader()
            .unwrap()
            .query_row("SELECT folder_id FROM items WHERE id = ?1", [ids[0]], |r| {
                r.get(0)
            })
            .unwrap();
        let mut changed = new_item(folder, &format!("{}/a.jpg", dir.path().display()), 1);
        changed.size += 1;
        lib.update_items(&[(ids[0], changed)]).unwrap();
        assert!(lib.item_detected_faces(ids[0]).unwrap().is_empty());
        assert_eq!(version_of(&lib, ids[0]), None);
    }

    #[test]
    fn progress_counts_live_images() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg", "gone.jpg", "clip.mp4"]);
        let w = lib.writer();
        w.execute("UPDATE items SET missing_since = 5 WHERE id = ?1", [ids[2]])
            .unwrap();
        w.execute("UPDATE items SET kind = 1 WHERE id = ?1", [ids[3]])
            .unwrap();
        drop(w);
        assert_eq!(lib.face_progress(V).unwrap(), (0, 2));
        let c = lib.face_candidates(0, 1, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![])], V).unwrap();
        assert_eq!(lib.face_progress(V).unwrap(), (1, 2));
    }

    #[test]
    fn rectangles_and_landmarks_round_trip() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.25)])], V)
            .unwrap();
        assert_eq!(lib.item_detected_faces(ids[0]).unwrap(), [face(0.25).rect]);
        let blob: Vec<u8> = lib
            .reader()
            .unwrap()
            .query_row("SELECT landmarks FROM detected_faces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(blob.len(), 40);
        assert_eq!(f32::from_le_bytes(blob[0..4].try_into().unwrap()), 0.1);
        assert_eq!(f32::from_le_bytes(blob[36..40].try_into().unwrap()), 1.0);
    }

    /// The candidate list reads every live photo, so it must walk the table by id, not the
    /// partial index `items_size` (`library/mod.rs` has why the bare term picks it).
    #[test]
    fn the_candidate_list_walks_the_table_by_id() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {CANDIDATES_SQL}"))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map(rusqlite::params![0, V, 10], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|s| s.contains("INTEGER PRIMARY KEY")),
            "expected a walk by rowid, got {plan:?}"
        );
        assert!(!plan.iter().any(|s| s.contains("items_")), "{plan:?}");
    }
}
