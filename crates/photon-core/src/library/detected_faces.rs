//! The faces photon found itself (`face_detect`), and which photos it has looked at. The
//! detecting is `crate::face_detect`; this is what it reads and writes.

use super::Library;
use super::items::edit_from_db;
use super::settings::face_detection_on;
use crate::Result;
use crate::edit::{Crop, Edit};
use crate::face_detect::{Detection, Rect};
use crate::face_embed::{Embedding, FaceBox, to_blob};
use crate::media::fingerprint;
use rusqlite::params;
use std::collections::HashMap;

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
/// The `+` keeps this a walk of the table by id from the id given. With the bare term the
/// plan is `SEARCH items USING INDEX items_pending (thumb_state=? AND rowid>?)`: this
/// query names `thumb_state`, so the partial index SQLite takes is that one and not
/// `items_size`. That walk is in id order too, so the harm `library/mod.rs` describes for
/// the bare term - `items_size`, in random table order - is not this query's. What the `+`
/// buys here is the table read once rather than an index entry and then its row for each
/// photo, when nearly every live photo's thumbnail is ready; where few are, the index
/// would skip the rest. Neither has been timed, so the `+` is the convention for a
/// whole-library read, kept, not a measured saving.
const CANDIDATES_SQL: &str = "SELECT id, path, size, mtime_ms, edit_turns, edit_crop FROM items
     WHERE id > ?1 AND +missing_since IS NULL AND thumb_state = 1
       -- A poster frame is not the video.
       AND kind = 0
       AND face_version IS NOT ?2
     ORDER BY id LIMIT ?3";

/// `face_progress`'s query, shared with its plan test. The `+` keeps it a scan in table
/// order rather than a walk of `items_size`; see `library/mod.rs`.
const PROGRESS_SQL: &str = "SELECT count(*) FILTER (WHERE face_version IS ?1), count(*) FROM items
     WHERE +missing_since IS NULL AND kind = 0";

/// A photo with faces still to embed, and those faces; see [`Library::embed_candidates`].
#[derive(Clone, Debug)]
pub struct EmbedCandidate {
    pub item_id: i64,
    /// The cache key of the preview to read - `Item::thumb_key()`'s value.
    pub thumb_key: u64,
    /// What the vectors will be stored against; see [`Library::write_embeddings`].
    pub size: i64,
    pub mtime_ms: i64,
    pub edit: Edit,
    /// Each face's row id and where it is in the picture as shown.
    pub faces: Vec<(i64, FaceBox)>,
}

/// Photos, in id order after a given one, with at least one face whose
/// `embedding_version` is not the current one; each with every such face. Driven from
/// `detected_faces` through its item index: a library has far fewer faces than photos.
///
/// The photo's own conditions (live, preview ready) are in the subquery, so the `LIMIT`
/// counts only photos that can be listed: outside it, a page made of missing photos came
/// back empty while later photos still had faces to do, and a pass that stops on an empty
/// page, or asks for one photo to learn whether there is any work, stopped there. The
/// outer query keeps only the per-face condition, which the subquery cannot give it.
/// `CROSS JOIN` fixes the order: faces first, each photo then read by rowid. Left to choose,
/// SQLite walked every ready photo through `items_pending` and probed faces for each.
const EMBED_CANDIDATES_SQL: &str =
    "SELECT f.id, f.item_id, f.left, f.top, f.right, f.bottom, f.landmarks,
            i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
     FROM detected_faces f JOIN items i ON i.id = f.item_id
     WHERE f.item_id IN (SELECT DISTINCT d.item_id
                         FROM detected_faces d CROSS JOIN items j ON j.id = d.item_id
                         WHERE d.item_id > ?1 AND d.embedding_version IS NOT ?2
                           AND j.missing_since IS NULL AND j.thumb_state = 1
                         ORDER BY d.item_id LIMIT ?3)
       AND f.embedding_version IS NOT ?2
     ORDER BY f.item_id, f.id";

const EMBED_PROGRESS_SQL: &str =
    "SELECT count(*) FILTER (WHERE f.embedding_version IS ?1), count(*)
     FROM detected_faces f JOIN items i ON i.id = f.item_id
     WHERE i.missing_since IS NULL";

fn landmarks_from_blob(b: &[u8]) -> [(f32, f32); 5] {
    let f = |i: usize| {
        b.get(i * 4..i * 4 + 4)
            .and_then(|s| s.try_into().ok())
            .map_or(0.0, f32::from_le_bytes)
    };
    [
        (f(0), f(1)),
        (f(2), f(3)),
        (f(4), f(5)),
        (f(6), f(7)),
        (f(8), f(9)),
    ]
}

/// What the user did to one face of a photo, to hand to the face found at the same place
/// when the detector looks again, and the vector it had.
struct Carried {
    rect: Rect,
    person_id: Option<i64>,
    confirmed: bool,
    ignored: bool,
    rejected: Vec<i64>,
    embedding: Option<Vec<u8>>,
}

fn read_carried(tx: &rusqlite::Connection, item_id: i64) -> rusqlite::Result<Vec<Carried>> {
    let mut faces = tx.prepare_cached(
        "SELECT id, left, top, right, bottom, person_id, confirmed, ignored, embedding
         FROM detected_faces WHERE item_id = ?1 ORDER BY id",
    )?;
    let mut rejections =
        tx.prepare_cached("SELECT person_id FROM face_rejections WHERE face_id = ?1")?;
    let rows: Vec<(i64, Carried)> = faces
        .query_map(params![item_id], |r| {
            Ok((
                r.get(0)?,
                Carried {
                    rect: Rect {
                        left: r.get(1)?,
                        top: r.get(2)?,
                        right: r.get(3)?,
                        bottom: r.get(4)?,
                    },
                    person_id: r.get(5)?,
                    confirmed: r.get::<_, i64>(6)? == 1,
                    ignored: r.get::<_, i64>(7)? == 1,
                    rejected: Vec::new(),
                    embedding: r.get(8)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter()
        .map(|(id, mut c)| {
            c.rejected = rejections
                .query_map(params![id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(c)
        })
        .collect()
}

/// Hands `from` to the new face `face_id`. The vector goes too, with no
/// `embedding_version`: the face is still embedded again, from its own box, but until then
/// it counts towards its group's average. Without it, a detector re-run would empty every
/// named person's average for the length of the embedding step, and each face recognised
/// meanwhile would start a group of its own rather than become a suggestion.
fn carry(tx: &rusqlite::Connection, face_id: i64, from: &Carried) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE detected_faces SET person_id = ?2, confirmed = ?3, ignored = ?4, embedding = ?5
         WHERE id = ?1",
        params![
            face_id,
            from.person_id,
            from.confirmed as i64,
            from.ignored as i64,
            from.embedding,
        ],
    )?;
    for person in &from.rejected {
        tx.execute(
            "INSERT OR IGNORE INTO face_rejections (face_id, person_id) VALUES (?1, ?2)",
            params![face_id, person],
        )?;
    }
    Ok(())
}

/// How much two boxes of the same detector overlap: their intersection over their union.
fn overlap(a: &Rect, b: &Rect) -> f64 {
    let w = (a.right.min(b.right) - a.left.max(b.left)).max(0.0);
    let h = (a.bottom.min(b.bottom) - a.top.max(b.top)).max(0.0);
    let inter = w * h;
    let union =
        (a.right - a.left) * (a.bottom - a.top) + (b.right - b.left) * (b.bottom - b.top) - inter;
    if union > 0.0 { inter / union } else { 0.0 }
}

/// Which old face each new face inherits from, as `(new, old)` indexes: among the pairs
/// `merge::same_face` calls one face, the best-fitting first, each face of either side in
/// one pair at most.
///
/// Not the first old face that matches. A centre inside the other box is a loose test,
/// meant for Picasa's generous boxes against the detector's tight ones; two people close
/// together pass it - a small face whose centre lies inside a large one - and a new small
/// face listed first would take the large face's name. Both sides here are the detector's
/// own boxes, so how much they overlap says which is which.
fn pairs_by_fit(old: &[Carried], new: &[Detection]) -> Vec<(usize, usize)> {
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (n, face) in new.iter().enumerate() {
        for (o, was) in old.iter().enumerate() {
            if crate::face_detect::merge::same_face(&was.rect, &face.rect) {
                candidates.push((overlap(&was.rect, &face.rect), n, o));
            }
        }
    }
    // Best fit first; a tie in the order the faces were found and stored.
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let (mut new_taken, mut old_taken) = (vec![false; new.len()], vec![false; old.len()]);
    let mut pairs = Vec::new();
    for (_, n, o) in candidates {
        if !new_taken[n] && !old_taken[o] {
            new_taken[n] = true;
            old_taken[o] = true;
            pairs.push((n, o));
        }
    }
    pairs
}

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
                // What the user did to the faces this picture had, to hand to the faces
                // found at the same places. The picture is unchanged (the guard above
                // checked size, mtime and edit), so a face at the same place is the same
                // face; an edit or a new file clears the rows before this and carries
                // nothing.
                let old = read_carried(&tx, candidate.id)?;
                // An older detector's faces, when the version moved.
                delete.execute(params![candidate.id])?;
                let mut ids = Vec::with_capacity(faces.len());
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
                    ids.push(tx.last_insert_rowid());
                }
                for (n, o) in pairs_by_fit(&old, faces) {
                    carry(&tx, ids[n], &old[o])?;
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
        let (checked, total): (i64, i64) = conn.query_row(PROGRESS_SQL, params![version], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        Ok((checked as u64, total as u64))
    }

    /// Up to `limit` photos with an id above `after_item` that have faces whose vector was
    /// not made by embedder `version`, each with all such faces, in id order. Paged by photo
    /// with the last id read, as `face_candidates` is, so a photo that cannot be read is
    /// not handed back within one pass. Only photos whose preview is cached: the embedder
    /// reads that, never the source.
    pub fn embed_candidates(
        &self,
        after_item: i64,
        limit: usize,
        version: i64,
    ) -> Result<Vec<EmbedCandidate>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(EMBED_CANDIDATES_SQL)?;
        let mut rows = stmt.query(params![after_item, version, limit as i64])?;
        let mut out: Vec<EmbedCandidate> = Vec::new();
        while let Some(r) = rows.next()? {
            let item_id: i64 = r.get(1)?;
            let face = FaceBox {
                rect: Rect {
                    left: r.get(2)?,
                    top: r.get(3)?,
                    right: r.get(4)?,
                    bottom: r.get(5)?,
                },
                // Shorter than 40 bytes gives zeros, which the aligner refuses: that face
                // fails to embed rather than panicking.
                landmarks: landmarks_from_blob(&r.get::<_, Vec<u8>>(6)?),
            };
            let face_id: i64 = r.get(0)?;
            if let Some(last) = out.last_mut().filter(|c| c.item_id == item_id) {
                last.faces.push((face_id, face));
                continue;
            }
            let path: String = r.get(7)?;
            let size: i64 = r.get(8)?;
            let mtime_ms: i64 = r.get(9)?;
            let edit = edit_from_db(r.get(10)?, r.get(11)?);
            out.push(EmbedCandidate {
                item_id,
                thumb_key: edit.thumb_key(fingerprint(&path, size, mtime_ms)),
                size,
                mtime_ms,
                edit,
                faces: vec![(face_id, face)],
            });
        }
        Ok(out)
    }

    /// Stores each face's vector (`None` for a face too small or unreadable, which is
    /// still marked as looked at) and the embedder's version. Returns the faces written.
    ///
    /// Refused as `write_face_batch` is, for the same reasons: with the setting off it
    /// writes nothing, and a photo whose file or edit moved since it was listed is skipped,
    /// since its faces are rows the photo no longer has.
    #[allow(clippy::type_complexity)]
    pub fn write_embeddings(
        &self,
        batch: &[(EmbedCandidate, Vec<(i64, Option<Embedding>)>)],
        version: i64,
    ) -> Result<usize> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        if !face_detection_on(&tx)? {
            return Ok(0);
        }
        let mut written = 0;
        {
            let mut unchanged = tx.prepare_cached(
                "SELECT 1 FROM items WHERE id = ?1 AND size = ?2 AND mtime_ms = ?3
                   AND missing_since IS NULL AND edit_turns = ?4 AND edit_crop IS ?5",
            )?;
            let mut set = tx.prepare_cached(
                "UPDATE detected_faces SET embedding = ?2, embedding_version = ?3
                 WHERE id = ?1 AND item_id = ?4",
            )?;
            for (candidate, vectors) in batch {
                if !unchanged.exists(params![
                    candidate.item_id,
                    candidate.size,
                    candidate.mtime_ms,
                    candidate.edit.turns,
                    candidate.edit.crop.map(Crop::to_db),
                ])? {
                    continue;
                }
                for (face_id, vector) in vectors {
                    let blob = vector.as_ref().map(to_blob);
                    written += set.execute(params![face_id, blob, version, candidate.item_id])?;
                }
            }
        }
        tx.commit()?;
        Ok(written)
    }

    /// Faces whose vector is of embedder `version`, and all faces, on live images.
    pub fn embed_progress(&self, version: i64) -> Result<(u64, u64)> {
        let conn = self.reader()?;
        let (done, total): (i64, i64) =
            conn.query_row(EMBED_PROGRESS_SQL, params![version], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        Ok((done as u64, total as u64))
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

/// How many faces each photo has, for search: Picasa's and the detected ones, merged by
/// `face_detect::merge::count` with the photo's edit, so search counts what the viewer
/// draws. Photos without a face are absent.
///
/// Read whole, and only for a query that asks (`Needs::faces`): a few numbers per face.
pub(crate) fn search_face_counts(conn: &rusqlite::Connection) -> Result<HashMap<i64, u32>> {
    use crate::face_detect::merge;
    let rect = |r: &rusqlite::Row<'_>, at: usize| -> rusqlite::Result<Rect> {
        Ok(Rect {
            left: r.get(at)?,
            top: r.get(at + 1)?,
            right: r.get(at + 2)?,
            bottom: r.get(at + 3)?,
        })
    };
    let mut detected: HashMap<i64, Vec<Rect>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT item_id, left, top, right, bottom FROM detected_faces")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        detected.entry(r.get(0)?).or_default().push(rect(r, 1)?);
    }
    // Picasa's rectangles are in the unedited picture, so each needs its photo's edit.
    let mut picasa: HashMap<i64, (Edit, Vec<Rect>)> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT f.item_id, f.left, f.top, f.right, f.bottom, i.edit_turns, i.edit_crop
         FROM faces f JOIN items i ON i.id = f.item_id",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let edit = edit_from_db(r.get(5)?, r.get(6)?);
        picasa
            .entry(r.get(0)?)
            .or_insert_with(|| (edit, Vec::new()))
            .1
            .push(rect(r, 1)?);
    }
    let mut counts = HashMap::new();
    for (id, (edit, faces)) in &picasa {
        let found = detected.remove(id).unwrap_or_default();
        let n = merge::count(*edit, faces, &found);
        if n > 0 {
            counts.insert(*id, n);
        }
    }
    for (id, found) in detected {
        counts.insert(id, found.len() as u32);
    }
    Ok(counts)
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

    /// The candidate list walks the table by id. Without the `+` it is
    /// `SEARCH items USING INDEX items_pending (thumb_state=? AND rowid>?)` - still in id
    /// order, an index entry and a row per photo instead of the row alone, and not timed
    /// either way (see `CANDIDATES_SQL`). This pins which of the two it is, so the plan
    /// cannot move without someone looking.
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

    /// The progress count reads every live photo, so the `+` must keep it off `items_size`.
    #[test]
    fn the_progress_count_is_not_served_by_a_partial_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {PROGRESS_SQL}"))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map(rusqlite::params![V], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(!plan.iter().any(|s| s.contains("items_")), "{plan:?}");
    }

    /// Picasa's faces and the detections are counted through the one merge rule, with the
    /// photo's edit: a detection lying over a Picasa face is the same face.
    #[test]
    fn search_counts_merge_both_sources() {
        let (_dir, lib, ids) = seeded(&["both.jpg", "detected.jpg", "picasa.jpg", "none.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(
            &[
                // Over the Picasa face below, and one more elsewhere.
                (c[0].clone(), vec![face(0.12), face(0.7)]),
                (c[1].clone(), vec![face(0.1), face(0.4), face(0.7)]),
            ],
            V,
        )
        .unwrap();
        let picasa = |contact: &str| crate::picasa::Face {
            contact: contact.into(),
            left: 0.10,
            top: 0.15,
            right: 0.25,
            bottom: 0.45,
        };
        lib.set_item_faces(&[(ids[0], vec![picasa("a")]), (ids[2], vec![picasa("b")])])
            .unwrap();

        let counts = search_face_counts(&lib.reader().unwrap()).unwrap();
        assert_eq!(
            counts.get(&ids[0]),
            Some(&2),
            "one shared, one detected only"
        );
        assert_eq!(counts.get(&ids[1]), Some(&3));
        assert_eq!(
            counts.get(&ids[2]),
            Some(&1),
            "an unnamed Picasa face is a face"
        );
        assert_eq!(counts.get(&ids[3]), None);
    }

    /// The whole path: a face search reads the counts and answers with the right photos.
    #[test]
    fn a_face_search_finds_photos_by_their_faces() {
        use crate::grid::GridView;
        let (_dir, lib, ids) = seeded(&["with.jpg", "without.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(
            &[(c[0].clone(), vec![face(0.1)]), (c[1].clone(), vec![])],
            V,
        )
        .unwrap();
        let ids_for = |query: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, query)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(ids_for("has:face"), [ids[0]]);
        assert_eq!(ids_for("-has:face"), [ids[1]]);
        assert_eq!(ids_for("faces:1"), [ids[0]]);
        assert_eq!(ids_for("faces:2+"), Vec::<i64>::new());
        assert_eq!(ids_for("faces:0"), [ids[1]]);
    }

    use crate::face_embed::{EMBEDDER_VERSION as EV, Embedding};

    fn unit(i: usize) -> Embedding {
        let mut e = [0f32; 128];
        e[i] = 1.0;
        e
    }

    /// Every photo of the library detected with two faces, ready to embed.
    fn detected(lib: &Library) {
        let c = lib.face_candidates(0, 10, V).unwrap();
        let batch: Vec<_> = c
            .iter()
            .map(|c| (c.clone(), vec![face(0.1), face(0.5)]))
            .collect();
        lib.write_face_batch(&batch, V).unwrap();
    }

    #[test]
    fn faces_to_embed_are_listed_by_photo_with_their_boxes() {
        let (_dir, lib, ids) = seeded(&["a.jpg", "b.jpg"]);
        detected(&lib);
        let c = lib.embed_candidates(0, 10, EV).unwrap();
        assert_eq!(c.iter().map(|c| c.item_id).collect::<Vec<_>>(), ids);
        assert_eq!(c[0].faces.len(), 2);
        assert_eq!(c[0].faces[0].1.rect, face(0.1).rect);
        assert_eq!(c[0].faces[0].1.landmarks, face(0.1).landmarks);
        // Paging is by photo.
        assert_eq!(lib.embed_candidates(ids[0], 10, EV).unwrap().len(), 1);
    }

    #[test]
    fn an_embedded_face_is_no_longer_a_candidate() {
        let (_dir, lib, _ids) = seeded(&["a.jpg"]);
        detected(&lib);
        let c = lib.embed_candidates(0, 10, EV).unwrap();
        let faces: Vec<_> = c[0].faces.iter().map(|(id, _)| *id).collect();
        let written = lib
            .write_embeddings(
                &[(
                    c[0].clone(),
                    vec![(faces[0], Some(unit(0))), (faces[1], None)],
                )],
                EV,
            )
            .unwrap();
        assert_eq!(written, 2);
        assert!(
            lib.embed_candidates(0, 10, EV).unwrap().is_empty(),
            "a face too small is looked at too"
        );
        assert_eq!(lib.embed_progress(EV).unwrap(), (2, 2));
        // A photo with one face still to do lists that face alone.
        lib.writer()
            .execute(
                "UPDATE detected_faces SET embedding_version = NULL WHERE id = ?1",
                [faces[1]],
            )
            .unwrap();
        let again = lib.embed_candidates(0, 10, EV).unwrap();
        assert_eq!(
            again[0].faces.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [faces[1]]
        );
    }

    #[test]
    fn embeddings_are_refused_with_the_switch_off_or_the_photo_moved_on() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        detected(&lib);
        let c = lib.embed_candidates(0, 10, EV).unwrap();
        let one = vec![(c[0].faces[0].0, Some(unit(0)))];
        lib.set_item_edit(
            ids[0],
            Edit {
                turns: 1,
                crop: None,
            },
        )
        .unwrap();
        assert_eq!(
            lib.write_embeddings(&[(c[0].clone(), one.clone())], EV)
                .unwrap(),
            0
        );
        let (_dir2, lib2, _ids2) = seeded(&["a.jpg"]);
        detected(&lib2);
        let c2 = lib2.embed_candidates(0, 10, EV).unwrap();
        lib2.set_face_detection(false).unwrap();
        let one2 = vec![(c2[0].faces[0].0, Some(unit(0)))];
        assert_eq!(
            lib2.write_embeddings(&[(c2[0].clone(), one2)], EV).unwrap(),
            0
        );

        // The rows still there: a switch that is off but whose delete has not reached them,
        // and a file rewritten without its faces having been cleared yet.
        let (_dir3, lib3, _ids3) = seeded(&["a.jpg"]);
        detected(&lib3);
        let c3 = lib3.embed_candidates(0, 10, EV).unwrap();
        let one3 = vec![(c3[0].faces[0].0, Some(unit(0)))];
        lib3.writer()
            .execute(
                "UPDATE settings SET value = '0' WHERE key = 'face_detection'",
                [],
            )
            .unwrap();
        assert_eq!(
            lib3.write_embeddings(&[(c3[0].clone(), one3.clone())], EV)
                .unwrap(),
            0
        );
        lib3.writer()
            .execute(
                "UPDATE settings SET value = '1' WHERE key = 'face_detection'",
                [],
            )
            .unwrap();
        lib3.writer()
            .execute("UPDATE items SET mtime_ms = mtime_ms + 1", [])
            .unwrap();
        assert_eq!(
            lib3.write_embeddings(&[(c3[0].clone(), one3)], EV).unwrap(),
            0
        );
    }

    /// A detector re-run over an unchanged picture keeps what the user did: the face at
    /// the same place inherits its group, its confirmation and its rejections.
    #[test]
    fn a_detector_rerun_carries_a_confirmed_face_over() {
        let (_dir, lib, _ids) = seeded(&["a.jpg"]);
        detected(&lib);
        let (keep, gone) = {
            let c = lib.embed_candidates(0, 10, EV).unwrap();
            (c[0].faces[0].0, c[0].faces[1].0)
        };
        let w = lib.writer();
        w.execute(
            "INSERT INTO people (id, name) VALUES (7, 'Anna'), (8, NULL)",
            [],
        )
        .unwrap();
        w.execute(
            "UPDATE detected_faces SET person_id = 7, confirmed = 1 WHERE id = ?1",
            [keep],
        )
        .unwrap();
        w.execute("INSERT INTO face_rejections VALUES (?1, 8)", [keep])
            .unwrap();
        w.execute(
            "UPDATE detected_faces SET ignored = 1 WHERE id = ?1",
            [gone],
        )
        .unwrap();
        drop(w);
        // The same picture, detected again by a newer detector: one face where the
        // confirmed one was, nudged, and nothing where the other was.
        let c = lib.face_candidates(0, 10, V + 1).unwrap();
        let mut moved = face(0.1);
        moved.rect.left += 0.01;
        moved.rect.right += 0.01;
        lib.write_face_batch(&[(c[0].clone(), vec![moved])], V + 1)
            .unwrap();
        let r = lib.reader().unwrap();
        let (person, confirmed, ignored, rejected): (Option<i64>, i64, i64, i64) = r
            .query_row(
                "SELECT person_id, confirmed, ignored,
                        (SELECT count(*) FROM face_rejections WHERE face_id = f.id AND person_id = 8)
                 FROM detected_faces f",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((person, confirmed, ignored, rejected), (Some(7), 1, 0, 1));
    }

    /// An edit is not a re-run: the picture changed, and the user's confirmation goes with
    /// its rows (the face comes back as a suggestion once grouped again).
    #[test]
    fn an_edit_drops_the_confirmation() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        detected(&lib);
        lib.writer()
            .execute("INSERT INTO people (id, name) VALUES (7, 'Anna')", [])
            .unwrap();
        lib.writer()
            .execute("UPDATE detected_faces SET person_id = 7, confirmed = 1", [])
            .unwrap();
        lib.set_item_edit(
            ids[0],
            Edit {
                turns: 1,
                crop: None,
            },
        )
        .unwrap();
        let n: i64 = lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM detected_faces WHERE confirmed = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn switching_off_deletes_people_links_and_rejections() {
        let (_dir, lib, _ids) = seeded(&["a.jpg"]);
        detected(&lib);
        let w = lib.writer();
        w.execute("INSERT INTO people (id, name) VALUES (7, 'Anna')", [])
            .unwrap();
        w.execute("INSERT INTO person_contacts VALUES ('ada', 7)", [])
            .unwrap();
        w.execute(
            "INSERT INTO face_rejections SELECT id, 7 FROM detected_faces",
            [],
        )
        .unwrap();
        drop(w);
        lib.set_face_detection(false).unwrap();
        let r = lib.reader().unwrap();
        for table in [
            "people",
            "person_contacts",
            "face_rejections",
            "detected_faces",
        ] {
            let n: i64 = r
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table}");
        }
    }

    /// The page's limit counts photos that can be listed: a missing photo, or one whose
    /// preview is not ready, at the lowest id must not hide an eligible one behind it.
    #[test]
    fn ineligible_photos_do_not_use_up_the_page() {
        let (_dir, lib, ids) = seeded(&["gone.jpg", "pending.jpg", "ok.jpg"]);
        detected(&lib);
        let w = lib.writer();
        w.execute("UPDATE items SET missing_since = 5 WHERE id = ?1", [ids[0]])
            .unwrap();
        w.execute("UPDATE items SET thumb_state = 0 WHERE id = ?1", [ids[1]])
            .unwrap();
        drop(w);
        let c = lib.embed_candidates(0, 1, EV).unwrap();
        assert_eq!(c.iter().map(|c| c.item_id).collect::<Vec<_>>(), [ids[2]]);
    }

    /// A re-run with one face, at the place of the face the user ignored: it inherits that
    /// face's state and not the first old face's.
    #[test]
    fn a_rerun_face_inherits_from_the_face_at_its_own_place() {
        let (_dir, lib, ids) = seeded(&["a.jpg"]);
        detected(&lib);
        let (first, second) = {
            let c = lib.embed_candidates(0, 10, EV).unwrap();
            (c[0].faces[0].0, c[0].faces[1].0)
        };
        let w = lib.writer();
        w.execute("INSERT INTO people (id, name) VALUES (7, 'Anna')", [])
            .unwrap();
        w.execute(
            "UPDATE detected_faces SET person_id = 7, confirmed = 1 WHERE id = ?1",
            [first],
        )
        .unwrap();
        w.execute("INSERT INTO face_rejections VALUES (?1, 7)", [first])
            .unwrap();
        w.execute(
            "UPDATE detected_faces SET ignored = 1 WHERE id = ?1",
            [second],
        )
        .unwrap();
        drop(w);
        let c = lib.face_candidates(0, 10, V + 1).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.5)])], V + 1)
            .unwrap();
        let (person, confirmed, ignored, rejected): (Option<i64>, i64, i64, i64) = lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT person_id, confirmed, ignored,
                        (SELECT count(*) FROM face_rejections)
                 FROM detected_faces",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((person, confirmed, ignored, rejected), (None, 0, 1, 0));
        assert_eq!(rows(&lib), 1);
        assert_eq!(version_of(&lib, ids[0]), Some(V + 1));
    }

    /// Two new faces over one old face: the old face's state goes to one of them.
    #[test]
    fn a_rerun_hands_one_old_face_to_one_new_face() {
        let (_dir, lib, _ids) = seeded(&["a.jpg"]);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.1)])], V)
            .unwrap();
        let w = lib.writer();
        w.execute("INSERT INTO people (id, name) VALUES (7, 'Anna')", [])
            .unwrap();
        w.execute("UPDATE detected_faces SET person_id = 7, confirmed = 1", [])
            .unwrap();
        drop(w);
        let c = lib.face_candidates(0, 10, V + 1).unwrap();
        let mut nudged = face(0.1);
        nudged.rect.left += 0.01;
        nudged.rect.right += 0.01;
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.1), nudged])], V + 1)
            .unwrap();
        let n: i64 = lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM detected_faces WHERE person_id = 7",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((rows(&lib), n), (2, 1));
    }

    /// Two people close together: Ben's small face lies inside Anna's large one, so each
    /// new box passes the centre test against both old ones. The re-run lists Ben's face
    /// first, and he keeps his name rather than taking hers.
    #[test]
    fn a_rerun_hands_overlapping_faces_back_by_fit() {
        let (_dir, lib, _ids) = seeded(&["a.jpg"]);
        let at = |left: f64, top: f64, right: f64, bottom: f64| Detection {
            rect: Rect {
                left,
                top,
                right,
                bottom,
            },
            ..face(0.0)
        };
        let anna = at(0.1, 0.1, 0.6, 0.6);
        let ben = at(0.3, 0.3, 0.42, 0.42);
        let c = lib.face_candidates(0, 10, V).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![anna.clone(), ben.clone()])], V)
            .unwrap();
        let w = lib.writer();
        w.execute(
            "INSERT INTO people (id, name) VALUES (7, 'Anna'), (8, 'Ben')",
            [],
        )
        .unwrap();
        w.execute(
            "UPDATE detected_faces SET person_id = CASE WHEN left < 0.2 THEN 7 ELSE 8 END,
                                       confirmed = 1",
            [],
        )
        .unwrap();
        drop(w);
        let c = lib.face_candidates(0, 10, V + 1).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![ben, anna])], V + 1)
            .unwrap();
        let r = lib.reader().unwrap();
        let mut stmt = r
            .prepare("SELECT left, person_id FROM detected_faces ORDER BY left")
            .unwrap();
        let rows: Vec<(f64, Option<i64>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows, [(0.1, Some(7)), (0.3, Some(8))]);
    }

    /// The vector goes with what the user did, without a version: the face is embedded
    /// again, and meanwhile counts towards its group (`library/people.rs` has the rest).
    #[test]
    fn a_rerun_carries_the_vector_to_be_made_again() {
        let (_dir, lib, _ids) = seeded(&["a.jpg"]);
        detected(&lib);
        let c = lib.embed_candidates(0, 10, EV).unwrap();
        let faces: Vec<_> = c[0].faces.iter().map(|(id, _)| *id).collect();
        lib.write_embeddings(
            &[(
                c[0].clone(),
                vec![(faces[0], Some(unit(3))), (faces[1], None)],
            )],
            EV,
        )
        .unwrap();
        let c = lib.face_candidates(0, 10, V + 1).unwrap();
        lib.write_face_batch(&[(c[0].clone(), vec![face(0.1), face(0.5)])], V + 1)
            .unwrap();
        let rows: Vec<(Option<Vec<u8>>, Option<i64>)> = {
            let r = lib.reader().unwrap();
            let mut stmt = r
                .prepare("SELECT embedding, embedding_version FROM detected_faces ORDER BY left")
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(rows, [(Some(to_blob(&unit(3))), None), (None, None)]);
        assert_eq!(lib.embed_candidates(0, 10, EV).unwrap()[0].faces.len(), 2);
    }

    #[test]
    fn the_embedding_candidates_are_found_through_the_face_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {EMBED_CANDIDATES_SQL}"))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map(rusqlite::params![0, EV, 10], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|s| s.contains("detected_faces_item")),
            "{plan:?}"
        );
        assert!(
            !plan.iter().any(|s| s.contains("items_pending")),
            "faces drive it, not every ready photo: {plan:?}"
        );
    }
}
