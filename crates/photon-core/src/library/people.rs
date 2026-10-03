//! People: the grouping step that places faces, the operations the user corrects them
//! with, and what the People page reads. The rule is `crate::people`.

use super::Library;
use super::settings::face_detection_on;
use crate::face_embed::{DIM, from_blob};
use crate::people::{Choice, Group, choose, counts_toward_centroid};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, HashSet};

/// Faces per write of the grouping step: the writer is held for one batch, not the whole
/// first run over a library.
const GROUP_BATCH: usize = 512;

/// The faces the grouping step places: a vector, no group, not ignored. One string for the
/// step's list and for `has_ungrouped_faces`, so the pass never asks for work the step
/// would not do.
const UNGROUPED: &str = "person_id IS NULL AND ignored = 0 AND embedding IS NOT NULL";

impl Library {
    /// Places every face that has a vector, no group and is not ignored, by the rule, in
    /// face order. Returns how many were placed.
    ///
    /// Faces on hidden and missing photos are placed too, so that unhiding is instant; the
    /// page's reads are what leave them out.
    ///
    /// The groups' averages are computed here from their faces, not stored: a stored one
    /// would have to be kept right by every writer that moves a face. Groups left with no
    /// face are deleted first, unless named or linked to a Picasa contact.
    pub fn group_ungrouped_faces(&self, cancel: &dyn Fn() -> bool) -> Result<usize> {
        {
            let mut conn = self.writer();
            let tx = conn.transaction()?;
            if !face_detection_on(&tx)? {
                return Ok(0);
            }
            tx.execute(
                "DELETE FROM people WHERE name IS NULL
                   AND id NOT IN (SELECT person_id FROM person_contacts)
                   AND id NOT IN (SELECT person_id FROM detected_faces WHERE person_id IS NOT NULL)",
                [],
            )?;
            tx.commit()?;
        }
        let (mut groups, rejected) = self.groups_and_rejections()?;
        let ungrouped: Vec<(i64, Vec<f32>)> = {
            let conn = self.reader()?;
            let mut stmt = conn.prepare(&format!(
                "SELECT id, embedding FROM detected_faces WHERE {UNGROUPED} ORDER BY id"
            ))?;
            stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
                .filter_map(|row| {
                    row.map(|(id, b)| from_blob(&b).map(|e| (id, e.to_vec())))
                        .transpose()
                })
                .collect::<rusqlite::Result<_>>()?
        };
        let none = HashSet::new();
        let mut placed = 0;
        for chunk in ungrouped.chunks(GROUP_BATCH) {
            if cancel() {
                break;
            }
            let mut conn = self.writer();
            let tx = conn.transaction()?;
            if !face_detection_on(&tx)? {
                return Ok(placed);
            }
            for (face, vector) in chunk {
                let refused = rejected.get(face).unwrap_or(&none);
                let group = match choose(vector, &groups.list, refused) {
                    Choice::Join(id) => id,
                    Choice::New => {
                        tx.execute("INSERT INTO people (name) VALUES (NULL)", [])?;
                        let id = tx.last_insert_rowid();
                        groups.insert(id, false);
                        id
                    }
                };
                // Only a row still ungrouped is placed: an operation may have moved it
                // since it was read.
                let moved = tx.execute(
                    "UPDATE detected_faces SET person_id = ?2, confirmed = 0
                     WHERE id = ?1 AND person_id IS NULL AND ignored = 0",
                    params![face, group],
                )?;
                if moved == 1 {
                    groups.add(group, vector, false);
                    placed += 1;
                }
            }
            tx.commit()?;
        }
        Ok(placed)
    }

    /// Whether the grouping step has anything to place.
    pub fn has_ungrouped_faces(&self) -> Result<bool> {
        Ok(self.reader()?.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM detected_faces WHERE {UNGROUPED})"),
            [],
            |r| r.get(0),
        )?)
    }

    /// Every group with the sum of the vectors that count towards it, and each face's
    /// rejections.
    fn groups_and_rejections(&self) -> Result<(Groups, Rejections)> {
        let conn = self.reader()?;
        let mut groups = Groups::default();
        let mut stmt = conn.prepare("SELECT id, name IS NOT NULL FROM people")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)))? {
            let (id, named) = row?;
            groups.insert(id, named);
        }
        let mut stmt = conn.prepare(
            "SELECT person_id, embedding, confirmed FROM detected_faces
             WHERE person_id IS NOT NULL AND embedding IS NOT NULL",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, i64>(2)? == 1,
            ))
        })? {
            let (person, blob, confirmed) = row?;
            if let Some(e) = from_blob(&blob) {
                groups.add(person, &e, confirmed);
            }
        }
        let mut rejected = Rejections::new();
        let mut stmt = conn.prepare("SELECT face_id, person_id FROM face_rejections")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (face, person) = row?;
            rejected.entry(face).or_default().insert(person);
        }
        Ok((groups, rejected))
    }
}

/// Each face's "not this person"s, by face id.
type Rejections = HashMap<i64, HashSet<i64>>;

/// The groups as the rule sees them, and which are named (whose average only confirmed
/// faces move).
#[derive(Default)]
struct Groups {
    list: Vec<Group>,
    at: HashMap<i64, (usize, bool)>,
}

impl Groups {
    fn insert(&mut self, id: i64, named: bool) {
        self.at.insert(id, (self.list.len(), named));
        self.list.push(Group {
            id,
            sum: vec![0f32; DIM],
            count: 0,
        });
    }

    fn add(&mut self, id: i64, vector: &[f32], confirmed: bool) {
        if let Some(&(i, named)) = self.at.get(&id)
            && counts_toward_centroid(named, confirmed)
        {
            self.list[i].add(vector);
        }
    }
}

/// A name as stored: trimmed; empty is refused.
fn clean(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        Err(Error::EmptyPersonName)
    } else {
        Ok(name.to_string())
    }
}

/// Names compared without case, in Rust: SQLite's `lower()` is ASCII-only without ICU, so
/// "ÉMILE" would not be "émile" to it.
fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The named person, other than `except`, whose name is `name` in any case.
fn person_named(tx: &Connection, name: &str, except: i64) -> rusqlite::Result<Option<i64>> {
    let mut stmt =
        tx.prepare_cached("SELECT id, name FROM people WHERE name IS NOT NULL AND id != ?1")?;
    let found = stmt
        .query_map(params![except], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .find(|(_, n)| same_name(n, name))
        .map(|(id, _)| id);
    Ok(found)
}

/// Links every Picasa contact no person has to the named person whose name it carries:
/// the user typing a name Picasa uses is the user saying it is the same person.
pub(crate) fn link_contacts_by_name(tx: &Connection) -> rusqlite::Result<()> {
    let people: Vec<(i64, String)> = tx
        .prepare_cached("SELECT id, name FROM people WHERE name IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    if people.is_empty() {
        return Ok(());
    }
    let unlinked: Vec<(String, String)> = tx
        .prepare_cached(
            "SELECT hash, name FROM contacts WHERE hash NOT IN (SELECT contact FROM person_contacts)",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (hash, name) in unlinked {
        if let Some((id, _)) = people.iter().find(|(_, n)| same_name(n, &name)) {
            tx.execute(
                "INSERT OR IGNORE INTO person_contacts (contact, person_id) VALUES (?1, ?2)",
                params![hash, id],
            )?;
        }
    }
    Ok(())
}

fn is_named(tx: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(tx
        .query_row(
            "SELECT name IS NOT NULL FROM people WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false))
}

/// Moves everything of `from` to `into` and deletes `from`. An unnamed group's faces become
/// confirmed, as naming it would; a named person's keep their state, so a suggestion stays
/// one. Rejections move before the delete, or its cascade would take them: a face that said
/// "not this group" says it of the person the group became.
fn merge_into(tx: &Connection, from: i64, into: i64) -> rusqlite::Result<()> {
    let from_named = is_named(tx, from)?;
    tx.execute(
        "UPDATE detected_faces SET person_id = ?2,
             confirmed = CASE WHEN ?3 THEN confirmed ELSE 1 END
         WHERE person_id = ?1",
        params![from, into, from_named],
    )?;
    // A face that rejected both keeps the row it has against `into`; the other goes with
    // `from`'s delete.
    tx.execute(
        "UPDATE OR IGNORE face_rejections SET person_id = ?2 WHERE person_id = ?1",
        params![from, into],
    )?;
    tx.execute(
        "UPDATE person_contacts SET person_id = ?2 WHERE person_id = ?1",
        params![from, into],
    )?;
    tx.execute("DELETE FROM people WHERE id = ?1", params![from])?;
    Ok(())
}

impl Library {
    /// Names a group, or renames a person; returns the person it ended in. A name taken by
    /// another person, in any case, merges the group into them. A group named for the first
    /// time has its faces confirmed; a person renamed keeps their suggestions as
    /// suggestions. Either way a Picasa contact of that name no one has is linked.
    pub fn name_group(&self, group: i64, name: &str) -> Result<i64> {
        let name = clean(name)?;
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let person = if let Some(existing) = person_named(&tx, &name, group)? {
            merge_into(&tx, group, existing)?;
            existing
        } else {
            if !is_named(&tx, group)? {
                tx.execute(
                    "UPDATE detected_faces SET confirmed = 1 WHERE person_id = ?1",
                    params![group],
                )?;
            }
            tx.execute(
                "UPDATE people SET name = ?2, ignored = 0 WHERE id = ?1",
                params![group, name],
            )?;
            group
        };
        link_contacts_by_name(&tx)?;
        tx.commit()?;
        Ok(person)
    }

    /// As naming: a taken name merges.
    pub fn rename_person(&self, person: i64, name: &str) -> Result<i64> {
        self.name_group(person, name)
    }

    /// Confirms suggestions. Only a face of a named person: a confirmed face is one that
    /// carries a name.
    pub fn confirm_faces(&self, faces: &[i64]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE detected_faces SET confirmed = 1 WHERE id = ?1
                   AND person_id IN (SELECT id FROM people WHERE name IS NOT NULL)",
            )?;
            for face in faces {
                stmt.execute(params![face])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// "Not this person": records the rejection and takes each face out of its group. The
    /// next grouping step places it again, passing that group over.
    pub fn reject_faces(&self, faces: &[i64]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut reject = tx.prepare_cached(
                "INSERT OR IGNORE INTO face_rejections (face_id, person_id)
                 SELECT id, person_id FROM detected_faces WHERE id = ?1 AND person_id IS NOT NULL",
            )?;
            let mut out = tx.prepare_cached(
                "UPDATE detected_faces SET person_id = NULL, confirmed = 0 WHERE id = ?1",
            )?;
            for face in faces {
                reject.execute(params![face])?;
                out.execute(params![face])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Merges a group or a person into a named person. Refused into anything else, and
    /// into itself: the merge ends by deleting `from`, and `from` being `into` would delete
    /// the person whose faces it had just kept.
    pub fn merge_people(&self, from: i64, into: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        if from == into || !is_named(&tx, into)? {
            return Err(Error::NotAPerson(into));
        }
        merge_into(&tx, from, into)?;
        tx.commit()?;
        Ok(())
    }

    /// Ignores a group, or stops. A named person is refused: delete them first, which makes
    /// them a group again.
    pub fn set_person_ignored(&self, person: i64, ignored: bool) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        if ignored && is_named(&tx, person)? {
            return Err(Error::PersonNamed(person));
        }
        tx.execute(
            "UPDATE people SET ignored = ?2 WHERE id = ?1",
            params![person, ignored],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Ignores faces, taking each out of its group, or stops ignoring them, after which the
    /// grouping step places them by the rule.
    pub fn set_faces_ignored(&self, faces: &[i64], ignored: bool) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(if ignored {
                "UPDATE detected_faces SET ignored = 1, person_id = NULL, confirmed = 0 WHERE id = ?1"
            } else {
                "UPDATE detected_faces SET ignored = 0 WHERE id = ?1"
            })?;
            for face in faces {
                stmt.execute(params![face])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes a person: the row stays, as an unnamed group of the same faces, unconfirmed
    /// and no longer linked to a Picasa contact.
    pub fn delete_person(&self, person: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE people SET name = NULL WHERE id = ?1",
            params![person],
        )?;
        tx.execute(
            "DELETE FROM person_contacts WHERE person_id = ?1",
            params![person],
        )?;
        tx.execute(
            "UPDATE detected_faces SET confirmed = 0 WHERE person_id = ?1",
            params![person],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// How many people the user has named: what switching face detection off would delete.
    pub fn named_people_count(&self) -> Result<i64> {
        Ok(self.reader()?.query_row(
            "SELECT count(*) FROM people WHERE name IS NOT NULL",
            [],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use crate::face_detect::{DETECTOR_VERSION as V, Detection, Rect};
    use crate::face_embed::{EMBEDDER_VERSION as EV, to_blob};
    use crate::media::ThumbState;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::collections::HashMap;

    /// A unit vector at `deg` degrees in the first two dimensions.
    fn at(deg: f32) -> [f32; 128] {
        let mut v = [0f32; 128];
        v[0] = deg.to_radians().cos();
        v[1] = deg.to_radians().sin();
        v
    }

    struct L {
        _dir: tempfile::TempDir,
        lib: Library,
        items: Vec<i64>,
    }

    /// One photo per entry of `faces`, each with faces at those angles, detected and
    /// embedded, the switch on, nothing grouped yet. Returns the face ids in order.
    fn library(faces: &[&[f32]]) -> (L, Vec<i64>) {
        let (dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, dir.path());
        let items = lib
            .insert_items(
                &(0..faces.len())
                    .map(|i| new_item(folder, &format!("{}/{i}.jpg", dir.path().display()), 1))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        for id in &items {
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }
        lib.set_face_detection(true).unwrap();
        let c = lib.face_candidates(0, 100, V).unwrap();
        let batch: Vec<_> = c
            .iter()
            .zip(faces)
            .map(|(c, angles)| {
                let dets = angles
                    .iter()
                    .enumerate()
                    .map(|(k, _)| Detection {
                        rect: Rect {
                            left: 0.1 * k as f64,
                            top: 0.1,
                            right: 0.1 * k as f64 + 0.08,
                            bottom: 0.2,
                        },
                        landmarks: [(0.0, 0.0); 5],
                        score: 0.9,
                    })
                    .collect();
                (c.clone(), dets)
            })
            .collect();
        lib.write_face_batch(&batch, V).unwrap();
        let mut ids = Vec::new();
        for (item, angles) in items.iter().zip(faces) {
            let rows: Vec<i64> = {
                let r = lib.reader().unwrap();
                let mut s = r
                    .prepare("SELECT id FROM detected_faces WHERE item_id = ?1 ORDER BY id")
                    .unwrap();
                s.query_map([item], |r| r.get(0))
                    .unwrap()
                    .map(Result::unwrap)
                    .collect()
            };
            for (id, deg) in rows.iter().zip(angles.iter()) {
                lib.writer()
                    .execute(
                        "UPDATE detected_faces SET embedding = ?2, embedding_version = ?3 WHERE id = ?1",
                        rusqlite::params![id, to_blob(&at(*deg)), EV],
                    )
                    .unwrap();
                ids.push(*id);
            }
        }
        (
            L {
                _dir: dir,
                lib,
                items,
            },
            ids,
        )
    }

    fn never() -> bool {
        false
    }

    fn person_of(lib: &Library, face: i64) -> (Option<i64>, bool) {
        lib.reader()
            .unwrap()
            .query_row(
                "SELECT person_id, confirmed FROM detected_faces WHERE id = ?1",
                [face],
                |r| Ok((r.get(0)?, r.get::<_, i64>(1)? == 1)),
            )
            .unwrap()
    }

    fn set_ignored_by_hand(lib: &Library, face: i64, ignored: bool) {
        lib.writer()
            .execute(
                "UPDATE detected_faces SET ignored = ?2 WHERE id = ?1",
                rusqlite::params![face, ignored],
            )
            .unwrap();
    }

    #[test]
    fn alike_faces_share_a_group_and_others_start_their_own() {
        let (l, f) = library(&[&[0.0], &[10.0], &[90.0]]);
        assert_eq!(l.lib.group_ungrouped_faces(&never).unwrap(), 3);
        let (a, b, c) = (
            person_of(&l.lib, f[0]).0,
            person_of(&l.lib, f[1]).0,
            person_of(&l.lib, f[2]).0,
        );
        assert!(a.is_some() && a == b && c != a, "{a:?} {b:?} {c:?}");
        assert!(!l.lib.has_ungrouped_faces().unwrap());
    }

    #[test]
    fn grouping_is_in_face_order_and_repeatable() {
        let (l, f) = library(&[&[0.0], &[40.0], &[75.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        // 0° starts a group; 40° joins it (0.77); the group's average is then at 20°, and
        // 75° is 55° from it (0.57): it joins too, which it would not have against 0° alone.
        let groups: Vec<_> = f.iter().map(|id| person_of(&l.lib, *id).0).collect();
        assert!(groups.iter().all(|g| *g == groups[0]), "{groups:?}");
    }

    #[test]
    fn an_ignored_face_and_a_face_without_a_vector_are_not_grouped() {
        let (l, f) = library(&[&[0.0, 5.0]]);
        set_ignored_by_hand(&l.lib, f[0], true);
        l.lib
            .writer()
            .execute(
                "UPDATE detected_faces SET embedding = NULL WHERE id = ?1",
                [f[1]],
            )
            .unwrap();
        assert_eq!(l.lib.group_ungrouped_faces(&never).unwrap(), 0);
    }

    #[test]
    fn nothing_is_grouped_with_the_switch_off() {
        let (l, _f) = library(&[&[0.0]]);
        l.lib
            .writer()
            .execute(
                "UPDATE settings SET value = '0' WHERE key = 'face_detection'",
                [],
            )
            .unwrap();
        assert_eq!(l.lib.group_ungrouped_faces(&never).unwrap(), 0);
    }

    /// A cancelled step places nothing more, and what it left is still work.
    #[test]
    fn a_cancelled_step_places_nothing() {
        let (l, _f) = library(&[&[0.0], &[90.0]]);
        assert_eq!(l.lib.group_ungrouped_faces(&|| true).unwrap(), 0);
        assert!(l.lib.has_ungrouped_faces().unwrap());
    }

    /// A hidden photo's faces are grouped like any other, so that unhiding it is instant;
    /// only the page's reads leave them out.
    #[test]
    fn a_hidden_photos_faces_are_grouped() {
        let (l, f) = library(&[&[0.0], &[5.0]]);
        l.lib.set_hidden(&[l.items[1]], true).unwrap();
        assert_eq!(l.lib.group_ungrouped_faces(&never).unwrap(), 2);
        assert_eq!(person_of(&l.lib, f[1]).0, person_of(&l.lib, f[0]).0);
    }

    #[test]
    fn naming_a_group_confirms_its_faces() {
        let (l, f) = library(&[&[0.0], &[10.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        assert_eq!(l.lib.name_group(group, "  Anna ").unwrap(), group);
        assert_eq!(person_of(&l.lib, f[0]), (Some(group), true));
        assert_eq!(person_of(&l.lib, f[1]), (Some(group), true));
        let name: String = l
            .lib
            .reader()
            .unwrap()
            .query_row("SELECT name FROM people WHERE id = ?1", [group], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(name, "Anna", "trimmed");
    }

    #[test]
    fn an_empty_name_is_refused() {
        let (l, f) = library(&[&[0.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        assert!(matches!(
            l.lib.name_group(group, "   "),
            Err(Error::EmptyPersonName)
        ));
    }

    /// A name typed in another case is the same person: naming a second group "anna" adds
    /// it to Anna rather than making a second Anna.
    #[test]
    fn a_taken_name_in_any_case_merges() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (a, b) = (
            person_of(&l.lib, f[0]).0.unwrap(),
            person_of(&l.lib, f[1]).0.unwrap(),
        );
        l.lib.name_group(a, "Anna").unwrap();
        assert_eq!(l.lib.name_group(b, "anna").unwrap(), a);
        assert_eq!(person_of(&l.lib, f[1]), (Some(a), true));
        let groups: i64 = l
            .lib
            .reader()
            .unwrap()
            .query_row("SELECT count(*) FROM people", [], |r| r.get(0))
            .unwrap();
        assert_eq!(groups, 1);
    }

    /// A rename is a name, not a confirmation: the person's suggestions stay suggestions.
    /// Confirming them would put the name on faces the user has not looked at.
    #[test]
    fn renaming_a_person_leaves_their_suggestions_unconfirmed() {
        let (l, f) = library(&[&[0.0], &[10.0]]);
        set_ignored_by_hand(&l.lib, f[1], true);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let anna = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(anna, "Anna").unwrap();
        set_ignored_by_hand(&l.lib, f[1], false);
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (Some(anna), false));
        assert_eq!(l.lib.rename_person(anna, "Anne").unwrap(), anna);
        assert_eq!(person_of(&l.lib, f[0]), (Some(anna), true));
        assert_eq!(
            person_of(&l.lib, f[1]),
            (Some(anna), false),
            "still a suggestion"
        );
    }

    #[test]
    fn a_name_picasa_uses_links_the_contact() {
        let (l, f) = library(&[&[0.0]]);
        l.lib
            .upsert_contacts(&HashMap::from([("h1".to_string(), "ANNA".to_string())]))
            .unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(group, "Anna").unwrap();
        let linked: i64 = l
            .lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT person_id FROM person_contacts WHERE contact = 'h1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, group);
    }

    #[test]
    fn a_contact_recorded_later_is_linked_by_name() {
        let (l, f) = library(&[&[0.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(group, "Anna").unwrap();
        l.lib
            .upsert_contacts(&HashMap::from([("h2".to_string(), "anna".to_string())]))
            .unwrap();
        let linked: i64 = l
            .lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT person_id FROM person_contacts WHERE contact = 'h2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, group);
    }

    /// A face matched to a named person later is a suggestion, and does not move them.
    #[test]
    fn a_later_match_is_a_suggestion() {
        let (l, f) = library(&[&[0.0], &[10.0]]);
        // Only the first is grouped and named; the second arrives afterwards.
        set_ignored_by_hand(&l.lib, f[1], true);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let anna = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(anna, "Anna").unwrap();
        set_ignored_by_hand(&l.lib, f[1], false);
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (Some(anna), false));
        l.lib.confirm_faces(&[f[1]]).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (Some(anna), true));
    }

    /// A confirmed face is one that carries a name: a face of an unnamed group, or of no
    /// group, is not confirmed.
    #[test]
    fn only_a_named_persons_face_can_be_confirmed() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        set_ignored_by_hand(&l.lib, f[1], true);
        l.lib.group_ungrouped_faces(&never).unwrap();
        l.lib.confirm_faces(&[f[0], f[1]]).unwrap();
        assert!(!person_of(&l.lib, f[0]).1, "an unnamed group's face");
        assert!(!person_of(&l.lib, f[1]).1, "a face of no group");
    }

    /// A named person's average is their confirmed faces alone. Anna is confirmed at 0°
    /// and a suggestion at 50° joins her (0.64). A face at -40° is 0.77 from her confirmed
    /// face and joins; had the suggestion counted, her average would sit at 25°, 65° away
    /// (0.42), and the face would start a group of its own.
    #[test]
    fn suggestions_do_not_move_a_named_person() {
        let (l, f) = library(&[&[0.0], &[50.0], &[-40.0]]);
        set_ignored_by_hand(&l.lib, f[1], true);
        set_ignored_by_hand(&l.lib, f[2], true);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let anna = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(anna, "Anna").unwrap();
        set_ignored_by_hand(&l.lib, f[1], false);
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (Some(anna), false));
        // A later step reads the suggestion back from the library with the person's faces.
        set_ignored_by_hand(&l.lib, f[2], false);
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[2]), (Some(anna), false));
    }

    #[test]
    fn a_rejected_face_is_regrouped_elsewhere() {
        let (l, f) = library(&[&[0.0], &[10.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[1]).0.unwrap();
        l.lib.reject_faces(&[f[1]]).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (None, false));
        l.lib.group_ungrouped_faces(&never).unwrap();
        let now = person_of(&l.lib, f[1]).0.unwrap();
        assert_ne!(now, group);
    }

    /// A merge is into a named person, never into the group itself: merging a person into
    /// themselves would move their faces to themselves and then delete them, leaving every
    /// confirmed face with no person.
    #[test]
    fn merging_into_oneself_or_into_an_unnamed_group_is_refused() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (a, b) = (
            person_of(&l.lib, f[0]).0.unwrap(),
            person_of(&l.lib, f[1]).0.unwrap(),
        );
        l.lib.name_group(a, "Anna").unwrap();
        assert!(matches!(
            l.lib.merge_people(a, a),
            Err(Error::NotAPerson(_))
        ));
        assert!(matches!(
            l.lib.merge_people(a, b),
            Err(Error::NotAPerson(_))
        ));
        assert_eq!(person_of(&l.lib, f[0]), (Some(a), true), "unchanged");
    }

    /// A rejection follows the group into the person it is merged into: the face said "not
    /// this group" and the group is now Anna.
    #[test]
    fn a_rejection_survives_a_merge() {
        let (l, f) = library(&[&[0.0], &[10.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let group = person_of(&l.lib, f[0]).0.unwrap();
        let anna = person_of(&l.lib, f[2]).0.unwrap();
        l.lib.name_group(anna, "Anna").unwrap();
        l.lib.reject_faces(&[f[1]]).unwrap();
        l.lib.merge_people(group, anna).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_ne!(person_of(&l.lib, f[1]).0, Some(anna));
        assert_eq!(
            person_of(&l.lib, f[0]),
            (Some(anna), true),
            "merged faces are confirmed"
        );
    }

    #[test]
    fn a_named_person_cannot_be_ignored_but_a_group_can() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (a, b) = (
            person_of(&l.lib, f[0]).0.unwrap(),
            person_of(&l.lib, f[1]).0.unwrap(),
        );
        l.lib.name_group(a, "Anna").unwrap();
        assert!(matches!(
            l.lib.set_person_ignored(a, true),
            Err(Error::PersonNamed(_))
        ));
        l.lib.set_person_ignored(b, true).unwrap();
        assert!(ignored(&l.lib, b));
        l.lib.set_person_ignored(b, false).unwrap();
        assert!(!ignored(&l.lib, b));
    }

    fn ignored(lib: &Library, person: i64) -> bool {
        lib.reader()
            .unwrap()
            .query_row("SELECT ignored FROM people WHERE id = ?1", [person], |r| {
                r.get(0)
            })
            .unwrap()
    }

    #[test]
    fn an_ignored_face_leaves_its_group_and_comes_back() {
        let (l, f) = library(&[&[0.0], &[5.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        l.lib.set_faces_ignored(&[f[1]], true).unwrap();
        assert_eq!(person_of(&l.lib, f[1]), (None, false));
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[1]).0, None, "an ignored face stays out");
        l.lib.set_faces_ignored(&[f[1]], false).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(person_of(&l.lib, f[1]).0, person_of(&l.lib, f[0]).0);
    }

    #[test]
    fn deleting_a_person_makes_them_a_group_again() {
        let (l, f) = library(&[&[0.0]]);
        l.lib
            .upsert_contacts(&HashMap::from([("h1".to_string(), "Anna".to_string())]))
            .unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let p = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(p, "Anna").unwrap();
        l.lib.delete_person(p).unwrap();
        assert_eq!(person_of(&l.lib, f[0]), (Some(p), false));
        let (name, links): (Option<String>, i64) = l
            .lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT name, (SELECT count(*) FROM person_contacts) FROM people WHERE id = ?1",
                [p],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name, links), (None, 0));
    }

    #[test]
    fn an_emptied_group_goes_and_a_named_one_stays() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (a, b) = (
            person_of(&l.lib, f[0]).0.unwrap(),
            person_of(&l.lib, f[1]).0.unwrap(),
        );
        l.lib.name_group(b, "Ben").unwrap();
        l.lib.set_faces_ignored(&[f[0], f[1]], true).unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let ids: Vec<i64> = {
            let r = l.lib.reader().unwrap();
            let mut s = r.prepare("SELECT id FROM people ORDER BY id").unwrap();
            s.query_map([], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(ids, [b], "the unnamed group {a} is gone");
    }

    #[test]
    fn named_people_are_counted() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        assert_eq!(l.lib.named_people_count().unwrap(), 0);
        l.lib
            .name_group(person_of(&l.lib, f[0]).0.unwrap(), "Anna")
            .unwrap();
        assert_eq!(l.lib.named_people_count().unwrap(), 1);
    }
}
