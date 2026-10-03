//! People: the grouping step that places faces, the operations the user corrects them
//! with, and what the People page reads. The rule is `crate::people`.

use super::Library;
use super::items::edit_from_db;
use super::settings::face_detection_on;
use crate::edit::Edit;
use crate::face_detect::{Rect, merge};
use crate::face_embed::{DIM, from_blob};
use crate::grid::hex_key;
use crate::media::fingerprint;
use crate::people::{Choice, Group, choose, counts_toward_centroid};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

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

/// One face on the People page.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageFace {
    pub id: i64,
    pub item_id: i64,
    /// The photo's thumbnail key, `grid::hex_key` form: with the face id, what names the
    /// face's crop.
    pub thumb_key: String,
    pub confirmed: bool,
}

/// Picasa's name for a group: the contact, the name to offer (a linked contact's person's
/// name), and how many of the group's faces sit on that contact's faces.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub name: String,
    pub contact: String,
    pub faces: i64,
}

/// A group or person on the page: how many visible faces the section counts, and the first
/// of them.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGroup {
    pub id: i64,
    pub name: Option<String>,
    pub face_count: i64,
    pub faces: Vec<PageFace>,
    pub offer: Option<Offer>,
}

/// The People page, section by section. Every face in it is on a visible photo: a group
/// is placed by the faces the user can see.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeoplePage {
    /// Unnamed groups of two or more visible faces, largest first.
    pub unnamed: Vec<PageGroup>,
    /// The faces of unnamed groups of one, up to [`LISTED_FACES`].
    pub single_faces: Vec<PageFace>,
    pub single_count: i64,
    /// Each named person's unconfirmed faces, by name.
    pub suggestions: Vec<PageGroup>,
    /// Every named person, by name, with their confirmed visible faces - none, and a
    /// count of 0, for a person who has none.
    pub people: Vec<PageGroup>,
    pub ignored_groups: Vec<PageGroup>,
    /// Faces ignored one by one, up to [`LISTED_FACES`].
    pub ignored_faces: Vec<PageFace>,
}

/// Which of a person's faces `person_faces` pages through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FaceFilter {
    All,
    Confirmed,
    Unconfirmed,
}

/// How many single faces, and ignored faces, the page lists. They are a loose collection
/// rather than strips with "Show all", and a library can hold thousands of strangers.
const LISTED_FACES: usize = 200;

/// Every face on a visible photo with its group, in face order: what the page is built
/// from. Visible means not hidden and not missing; the grouping step is the one place
/// that sees hidden photos' faces.
///
/// Driven from `detected_faces`, each photo read by its id. The `+` is the whole-library
/// convention (`library/mod.rs`), kept, not load-bearing: today the bare term plans the
/// same, since the faces' own order is the `ORDER BY`. The plan test pins that plan, so a
/// change that would walk the photos instead is seen.
const PAGE_FACES_SQL: &str = "SELECT f.id, f.item_id, f.person_id, f.confirmed, f.ignored,
            f.left, f.top, f.right, f.bottom,
            i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
     FROM detected_faces f JOIN items i ON i.id = f.item_id
     WHERE (f.person_id IS NOT NULL OR f.ignored = 1)
       AND i.hidden = 0 AND +i.missing_since IS NULL
     ORDER BY f.id";

/// One person's visible faces, a page of them, through the person index. `?2` is the
/// confirmation asked for, or NULL for either.
const PERSON_FACES_SQL: &str = "SELECT f.id, f.item_id, f.person_id, f.confirmed, f.ignored,
            f.left, f.top, f.right, f.bottom,
            i.path, i.size, i.mtime_ms, i.edit_turns, i.edit_crop
     FROM detected_faces f JOIN items i ON i.id = f.item_id
     WHERE f.person_id = ?1 AND (?2 IS NULL OR f.confirmed = ?2)
       AND i.hidden = 0 AND i.missing_since IS NULL
     ORDER BY f.id LIMIT ?3 OFFSET ?4";

/// A row of either query.
struct FaceRow {
    face: PageFace,
    person_id: Option<i64>,
    ignored: bool,
    rect: Rect,
    edit: Edit,
}

fn face_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FaceRow> {
    let path: String = r.get(9)?;
    let edit = edit_from_db(r.get(12)?, r.get(13)?);
    Ok(FaceRow {
        face: PageFace {
            id: r.get(0)?,
            item_id: r.get(1)?,
            thumb_key: hex_key(edit.thumb_key(fingerprint(&path, r.get(10)?, r.get(11)?))),
            confirmed: r.get::<_, i64>(3)? == 1,
        },
        person_id: r.get(2)?,
        ignored: r.get::<_, i64>(4)? == 1,
        rect: Rect {
            left: r.get(5)?,
            top: r.get(6)?,
            right: r.get(7)?,
            bottom: r.get(8)?,
        },
        edit,
    })
}

fn page_group(id: i64, name: Option<String>, faces: &[&FaceRow], strip: usize) -> PageGroup {
    PageGroup {
        id,
        name,
        face_count: faces.len() as i64,
        faces: faces.iter().take(strip).map(|f| f.face.clone()).collect(),
        offer: None,
    }
}

/// Picasa's name for a group, when more than half of its faces that sit on a face Picasa
/// named are that contact's. A contact linked to a person offers the person's name.
fn offer(
    faces: &[&FaceRow],
    picasa: &HashMap<i64, Vec<(Rect, String)>>,
    names: &HashMap<String, String>,
) -> Option<Offer> {
    let mut votes: HashMap<&str, i64> = HashMap::new();
    let mut total = 0;
    for face in faces {
        let Some(on_photo) = picasa.get(&face.face.item_id) else {
            continue;
        };
        // Only a named contact's face counts, so it is looked for among those: an unnamed
        // Picasa face at the same place must not hide a named one. Picasa's rectangle is in
        // the unedited picture, the detection in the picture as shown.
        if let Some((_, contact)) = on_photo.iter().find(|(p, contact)| {
            names.contains_key(contact.as_str())
                && merge::shown(face.edit, *p)
                    .is_some_and(|shown| merge::same_face(&shown, &face.rect))
        }) {
            *votes.entry(contact.as_str()).or_default() += 1;
            total += 1;
        }
    }
    // Ties broken by contact, so the page does not change between two reads.
    let (contact, n) = votes
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))?;
    (2 * n > total).then(|| Offer {
        name: names[contact].clone(),
        contact: contact.to_string(),
        faces: n,
    })
}

/// Ids per `IN (...)` list, under SQLite's default limit on bound parameters.
const IN_CHUNK: usize = 500;

/// Picasa's faces on the given photos, in the unedited picture, by photo.
fn picasa_faces(
    conn: &Connection,
    items: &[i64],
) -> rusqlite::Result<HashMap<i64, Vec<(Rect, String)>>> {
    let mut out: HashMap<i64, Vec<(Rect, String)>> = HashMap::new();
    for chunk in items.chunks(IN_CHUNK) {
        let marks = vec!["?"; chunk.len()].join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT item_id, left, top, right, bottom, contact FROM faces
             WHERE item_id IN ({marks}) ORDER BY rowid"
        ))?;
        let mut rows = stmt.query(params_from_iter(chunk))?;
        while let Some(r) = rows.next()? {
            out.entry(r.get(0)?).or_default().push((
                Rect {
                    left: r.get(1)?,
                    top: r.get(2)?,
                    right: r.get(3)?,
                    bottom: r.get(4)?,
                },
                r.get(5)?,
            ));
        }
    }
    Ok(out)
}

/// Every named contact and the name to offer for it: its person's, when it is linked to
/// one, or Picasa's.
fn offered_names(conn: &Connection) -> rusqlite::Result<HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "SELECT c.hash, coalesce(p.name, c.name) FROM contacts c
         LEFT JOIN person_contacts pc ON pc.contact = c.hash
         LEFT JOIN people p ON p.id = pc.person_id",
    )?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

/// A name's place in a list: without case, then as typed, then by id.
fn by_name(g: &PageGroup) -> (String, String, i64) {
    let name = g.name.clone().unwrap_or_default();
    (name.to_lowercase(), name, g.id)
}

impl Library {
    /// The People page: every visible grouped or ignored face read once and sorted into
    /// the sections in Rust, each group with the first `strip` of its faces.
    pub fn people_page(&self, strip: usize) -> Result<PeoplePage> {
        let conn = self.reader()?;
        let people: Vec<(i64, Option<String>, bool)> = conn
            .prepare("SELECT id, name, ignored FROM people ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? == 1)))?
            .collect::<rusqlite::Result<_>>()?;
        let rows: Vec<FaceRow> = conn
            .prepare(PAGE_FACES_SQL)?
            .query_map([], face_row)?
            .collect::<rusqlite::Result<_>>()?;

        let mut page = PeoplePage::default();
        let mut by_person: BTreeMap<i64, Vec<&FaceRow>> = BTreeMap::new();
        for row in &rows {
            match row.person_id {
                Some(person) => by_person.entry(person).or_default().push(row),
                None if row.ignored && page.ignored_faces.len() < LISTED_FACES => {
                    page.ignored_faces.push(row.face.clone())
                }
                None => {}
            }
        }

        // The unnamed groups' faces, kept for the offer.
        let mut unnamed: Vec<(PageGroup, Vec<&FaceRow>)> = Vec::new();
        for (id, name, ignored) in people {
            let faces = by_person.remove(&id).unwrap_or_default();
            match name {
                // Every named person, with no visible face too: the page is where they are
                // renamed, merged and deleted, and a person whose faces were all rejected,
                // hidden or re-detected would otherwise be out of reach while still counted.
                Some(name) => {
                    let (confirmed, suggested): (Vec<&FaceRow>, Vec<&FaceRow>) =
                        faces.into_iter().partition(|f| f.face.confirmed);
                    page.people
                        .push(page_group(id, Some(name.clone()), &confirmed, strip));
                    if !suggested.is_empty() {
                        page.suggestions
                            .push(page_group(id, Some(name), &suggested, strip));
                    }
                }
                // A group with no visible face is in no section.
                None if faces.is_empty() => {}
                None if ignored => page
                    .ignored_groups
                    .push(page_group(id, None, &faces, strip)),
                None if faces.len() == 1 => {
                    page.single_count += 1;
                    if page.single_faces.len() < LISTED_FACES {
                        page.single_faces.push(faces[0].face.clone());
                    }
                }
                None => unnamed.push((page_group(id, None, &faces, strip), faces)),
            }
        }

        if !unnamed.is_empty() {
            let mut items: Vec<i64> = unnamed
                .iter()
                .flat_map(|(_, faces)| faces.iter().map(|f| f.face.item_id))
                .collect();
            items.sort_unstable();
            items.dedup();
            let picasa = picasa_faces(&conn, &items)?;
            if !picasa.is_empty() {
                let names = offered_names(&conn)?;
                for (group, faces) in &mut unnamed {
                    group.offer = offer(faces, &picasa, &names);
                }
            }
        }
        unnamed.sort_by_key(|(g, _)| (std::cmp::Reverse(g.face_count), g.id));
        page.unnamed = unnamed.into_iter().map(|(g, _)| g).collect();
        page.people.sort_by_cached_key(by_name);
        page.suggestions.sort_by_cached_key(by_name);
        Ok(page)
    }

    /// A page of one person's or group's visible faces, in face order: the rest of a strip.
    pub fn person_faces(
        &self,
        person: i64,
        which: FaceFilter,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<PageFace>> {
        let confirmed = match which {
            FaceFilter::All => None,
            FaceFilter::Confirmed => Some(1),
            FaceFilter::Unconfirmed => Some(0),
        };
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(PERSON_FACES_SQL)?;
        let faces = stmt
            .query_map(
                params![person, confirmed, limit as i64, offset as i64],
                face_row,
            )?
            .map(|row| row.map(|r| r.face))
            .collect::<rusqlite::Result<_>>()?;
        Ok(faces)
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

    /// A person renamed to their own name in another case is renamed, not merged into
    /// themselves: that merge would move the faces onto the same row and then delete it,
    /// leaving every confirmed face with no person.
    #[test]
    fn renaming_to_ones_own_name_in_another_case_renames() {
        let (l, f) = library(&[&[0.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let anna = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(anna, "anna").unwrap();
        assert_eq!(l.lib.rename_person(anna, "ANNA").unwrap(), anna);
        assert_eq!(person_of(&l.lib, f[0]), (Some(anna), true));
        let name: String = l
            .lib
            .reader()
            .unwrap()
            .query_row("SELECT name FROM people WHERE id = ?1", [anna], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(name, "ANNA");
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
        let page = l.lib.people_page(8).unwrap();
        assert!(
            page.unnamed.iter().all(|g| g.id != b)
                && page.single_faces.iter().all(|x| x.id != f[1])
        );
        assert!(page.ignored_groups.iter().any(|g| g.id == b));
        l.lib.set_person_ignored(b, false).unwrap();
        assert!(!ignored(&l.lib, b));
        assert!(l.lib.people_page(8).unwrap().ignored_groups.is_empty());
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

    fn ids(faces: &[PageFace]) -> Vec<i64> {
        faces.iter().map(|x| x.id).collect()
    }

    #[test]
    fn the_page_sorts_groups_into_its_sections() {
        let (l, f) = library(&[&[0.0], &[5.0], &[90.0], &[180.0], &[182.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let ben = person_of(&l.lib, f[3]).0.unwrap();
        l.lib.name_group(ben, "Ben").unwrap();
        // A later face for Ben, unconfirmed: put it there by hand as the step would.
        l.lib
            .writer()
            .execute(
                "UPDATE detected_faces SET confirmed = 0 WHERE id = ?1",
                [f[4]],
            )
            .unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert_eq!(page.unnamed.len(), 1, "the pair at 0° and 5°");
        assert_eq!(page.unnamed[0].face_count, 2);
        assert_eq!(page.unnamed[0].name, None);
        assert_eq!(page.single_count, 1, "the face at 90°");
        assert_eq!(ids(&page.single_faces), [f[2]]);
        assert_eq!(page.people.len(), 1);
        assert_eq!(page.people[0].name.as_deref(), Some("Ben"));
        assert_eq!(page.people[0].face_count, 1, "confirmed only");
        assert_eq!(ids(&page.people[0].faces), [f[3]]);
        assert!(page.people[0].faces[0].confirmed);
        assert_eq!(page.suggestions.len(), 1);
        assert_eq!(ids(&page.suggestions[0].faces), [f[4]]);
        assert!(!page.suggestions[0].faces[0].confirmed);
    }

    /// Largest group first; people by name, without case: "ada" before "Zoë", which byte
    /// order and id order would both put second.
    #[test]
    fn the_page_orders_groups_by_size_and_people_by_name() {
        let (l, f) = library(&[&[0.0], &[2.0], &[120.0], &[122.0], &[124.0], &[240.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert_eq!(
            page.unnamed
                .iter()
                .map(|g| g.face_count)
                .collect::<Vec<_>>(),
            [3, 2]
        );
        l.lib
            .name_group(person_of(&l.lib, f[0]).0.unwrap(), "Zoë")
            .unwrap();
        l.lib
            .name_group(person_of(&l.lib, f[5]).0.unwrap(), "ada")
            .unwrap();
        let page = l.lib.people_page(8).unwrap();
        let names: Vec<_> = page
            .people
            .iter()
            .map(|p| p.name.clone().unwrap())
            .collect();
        assert_eq!(names, ["ada", "Zoë"]);
    }

    /// A hidden photo's face is in no section and no count, and does not make a group
    /// "two or more"; a group whose only photo is hidden is in no section at all.
    #[test]
    fn a_hidden_photo_is_in_no_section() {
        let (l, f) = library(&[&[0.0], &[5.0], &[180.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        l.lib.set_hidden(&[l.items[1], l.items[2]], true).unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert!(
            page.unnamed.is_empty(),
            "one visible face is not a group of two"
        );
        assert_eq!(page.single_count, 1);
        assert!(page.single_faces.iter().all(|x| x.id != f[1]));
    }

    /// As for a hidden photo, for a missing one, in every section: a person whose only
    /// faces are on missing photos is listed with none.
    #[test]
    fn a_missing_photo_is_in_no_section() {
        let (l, f) = library(&[&[0.0], &[5.0], &[90.0], &[180.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        l.lib
            .name_group(person_of(&l.lib, f[2]).0.unwrap(), "Ben")
            .unwrap();
        l.lib
            .set_person_ignored(person_of(&l.lib, f[3]).0.unwrap(), true)
            .unwrap();
        l.lib
            .mark_missing(&[l.items[1], l.items[2], l.items[3]], 5)
            .unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert!(page.unnamed.is_empty());
        assert_eq!(ids(&page.single_faces), [f[0]]);
        assert!(page.ignored_groups.is_empty());
        // Ben is still listed, to be renamed or deleted, with nothing to show.
        assert_eq!(page.people.len(), 1);
        assert_eq!(
            (page.people[0].face_count, page.people[0].faces.len()),
            (0, 0)
        );
    }

    /// A named person with no visible confirmed face is still in People, with a count of
    /// 0: one whose only face was rejected, one whose only photo is hidden. Out of the page,
    /// they could not be renamed, merged or deleted, yet would still be counted as named.
    #[test]
    fn a_named_person_with_no_visible_face_is_still_listed() {
        let (l, f) = library(&[&[0.0], &[90.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let (anna, ben) = (
            person_of(&l.lib, f[0]).0.unwrap(),
            person_of(&l.lib, f[1]).0.unwrap(),
        );
        l.lib.name_group(anna, "Anna").unwrap();
        l.lib.name_group(ben, "Ben").unwrap();
        l.lib.reject_faces(&[f[0]]).unwrap();
        l.lib.set_hidden(&[l.items[1]], true).unwrap();
        let page = l.lib.people_page(8).unwrap();
        let listed: Vec<_> = page
            .people
            .iter()
            .map(|p| (p.id, p.face_count, p.faces.len()))
            .collect();
        assert_eq!(listed, [(anna, 0, 0), (ben, 0, 0)]);
        assert!(page.suggestions.is_empty());
    }

    #[test]
    fn ignored_faces_are_listed_on_their_own() {
        let (l, f) = library(&[&[0.0], &[5.0], &[10.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        l.lib.set_faces_ignored(&[f[1], f[2]], true).unwrap();
        l.lib.set_hidden(&[l.items[2]], true).unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert_eq!(ids(&page.ignored_faces), [f[1]]);
        assert_eq!(ids(&page.single_faces), [f[0]]);
    }

    #[test]
    fn a_strip_is_the_first_faces_and_the_rest_page() {
        let (l, f) = library(&[&[0.0], &[1.0], &[2.0], &[3.0], &[4.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let page = l.lib.people_page(2).unwrap();
        assert_eq!(page.unnamed[0].faces.len(), 2);
        assert_eq!(page.unnamed[0].face_count, 5);
        let g = page.unnamed[0].id;
        let rest = l.lib.person_faces(g, FaceFilter::All, 2, 10).unwrap();
        assert_eq!(ids(&rest), f[2..]);
    }

    #[test]
    fn a_persons_faces_page_by_confirmation_and_leave_out_hidden_and_missing() {
        let (l, f) = library(&[&[0.0], &[1.0], &[2.0], &[3.0], &[4.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        let anna = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(anna, "Anna").unwrap();
        l.lib
            .writer()
            .execute(
                "UPDATE detected_faces SET confirmed = 0 WHERE id IN (?1, ?2)",
                [f[3], f[4]],
            )
            .unwrap();
        l.lib.set_hidden(&[l.items[1]], true).unwrap();
        l.lib.mark_missing(&[l.items[4]], 5).unwrap();
        let faces = |which| ids(&l.lib.person_faces(anna, which, 0, 10).unwrap());
        assert_eq!(faces(FaceFilter::All), [f[0], f[2], f[3]]);
        assert_eq!(faces(FaceFilter::Confirmed), [f[0], f[2]]);
        assert_eq!(faces(FaceFilter::Unconfirmed), [f[3]]);
    }

    /// The key of the photo as shown: an edited photo's thumbnail is cached under a key
    /// the edit is part of, and the bare fingerprint names the unedited one.
    #[test]
    fn a_page_face_names_its_photos_thumbnail() {
        let (l, f) = library(&[&[0.0]]);
        l.lib.group_ungrouped_faces(&never).unwrap();
        l.lib
            .writer()
            .execute(
                "UPDATE items SET edit_turns = 1 WHERE id = ?1",
                [l.items[0]],
            )
            .unwrap();
        let item = l.lib.item(l.items[0]).unwrap().unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert_eq!(page.single_faces[0].item_id, l.items[0]);
        assert_eq!(page.single_faces[0].id, f[0]);
        assert_eq!(
            page.single_faces[0].thumb_key,
            crate::grid::hex_key(item.thumb_key())
        );
    }

    /// Picasa's name is offered when more than half of a group's faces that sit on a
    /// named Picasa face are that contact's.
    #[test]
    fn picasa_names_are_offered_by_majority() {
        let (l, f) = library(&[&[0.0], &[3.0], &[6.0]]);
        l.lib
            .upsert_contacts(&HashMap::from([
                ("h1".to_string(), "Anna".to_string()),
                ("h2".to_string(), "Ben".to_string()),
            ]))
            .unwrap();
        l.lib
            .set_item_faces(&[
                (l.items[0], vec![at_the_face("h1")]),
                (l.items[1], vec![at_the_face("h1")]),
                (l.items[2], vec![at_the_face("h2")]),
            ])
            .unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let page = l.lib.people_page(8).unwrap();
        let offer = page.unnamed[0].offer.as_ref().unwrap();
        assert_eq!(
            (offer.name.as_str(), offer.contact.as_str(), offer.faces),
            ("Anna", "h1", 2)
        );
        // Two against one is a majority; one against one is not.
        l.lib.set_item_faces(&[(l.items[1], vec![])]).unwrap();
        assert!(l.lib.people_page(8).unwrap().unnamed[0].offer.is_none());
        assert_eq!(f.len(), 3);
    }

    /// The detected face sits at left 0, top 0.1, right 0.08, bottom 0.2 on each photo.
    fn at_the_face(contact: &str) -> crate::picasa::Face {
        crate::picasa::Face {
            contact: contact.into(),
            left: 0.0,
            top: 0.1,
            right: 0.08,
            bottom: 0.2,
        }
    }

    /// Only faces on a named contact's face vote: one face on Anna's and one on a face no
    /// INI has named is a majority of one, not a tie. The unnamed face is first on its
    /// photo, at the same place as Anna's, so it must not hide hers either.
    #[test]
    fn an_unnamed_picasa_face_does_not_vote() {
        let (l, _f) = library(&[&[0.0], &[3.0], &[6.0]]);
        l.lib
            .upsert_contacts(&HashMap::from([("h1".to_string(), "Anna".to_string())]))
            .unwrap();
        l.lib
            .set_item_faces(&[
                (l.items[0], vec![at_the_face("nobody"), at_the_face("h1")]),
                (l.items[1], vec![at_the_face("nobody")]),
            ])
            .unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let page = l.lib.people_page(8).unwrap();
        let offer = page.unnamed[0].offer.as_ref().unwrap();
        assert_eq!((offer.contact.as_str(), offer.faces), ("h1", 1));
    }

    /// A contact linked to a person offers the person's name, which accepting would merge
    /// the group into, and not the name Picasa had.
    #[test]
    fn a_linked_contact_offers_its_persons_name() {
        let (l, f) = library(&[&[0.0], &[90.0], &[93.0]]);
        l.lib
            .upsert_contacts(&HashMap::from([("h1".to_string(), "Anna".to_string())]))
            .unwrap();
        l.lib
            .set_item_faces(&[
                (l.items[1], vec![at_the_face("h1")]),
                (l.items[2], vec![at_the_face("h1")]),
            ])
            .unwrap();
        l.lib.group_ungrouped_faces(&never).unwrap();
        let anna = person_of(&l.lib, f[0]).0.unwrap();
        l.lib.name_group(anna, "Anna").unwrap();
        l.lib.rename_person(anna, "Anna Smith").unwrap();
        let page = l.lib.people_page(8).unwrap();
        let offer = page.unnamed[0].offer.as_ref().unwrap();
        assert_eq!(
            (offer.name.as_str(), offer.contact.as_str()),
            ("Anna Smith", "h1")
        );
    }

    /// The Picasa rectangle is in the unedited picture and the detection in the picture as
    /// shown: a turned photo's face is matched through the edit.
    #[test]
    fn an_offer_reads_picasas_faces_through_the_edit() {
        let (l, _f) = library(&[&[0.0], &[3.0]]);
        l.lib
            .upsert_contacts(&HashMap::from([("h1".to_string(), "Anna".to_string())]))
            .unwrap();
        // A quarter turn clockwise takes (x, y) to (1 - y, x): this rectangle comes out at
        // the detection's place, left 0, top 0.1, right 0.08, bottom 0.2.
        let before_the_turn = crate::picasa::Face {
            contact: "h1".into(),
            left: 0.1,
            top: 0.92,
            right: 0.2,
            bottom: 1.0,
        };
        l.lib
            .set_item_faces(&[
                (l.items[0], vec![before_the_turn.clone()]),
                (l.items[1], vec![before_the_turn]),
            ])
            .unwrap();
        for item in &l.items {
            l.lib
                .writer()
                .execute("UPDATE items SET edit_turns = 1 WHERE id = ?1", [item])
                .unwrap();
        }
        l.lib.group_ungrouped_faces(&never).unwrap();
        let page = l.lib.people_page(8).unwrap();
        assert_eq!(page.unnamed[0].offer.as_ref().map(|o| o.faces), Some(2));
    }

    /// The page is driven from the faces, each photo read by its id: a library has far
    /// fewer faces than photos, and the faces come out in id order with no sort.
    #[test]
    fn the_page_is_driven_from_the_faces() {
        let (_dir, lib) = temp_library();
        let plan = lib.query_plan(PAGE_FACES_SQL, &[]);
        assert_eq!(
            plan,
            ["SCAN f", "SEARCH i USING INTEGER PRIMARY KEY (rowid=?)"],
            "{plan:?}"
        );
    }

    #[test]
    fn a_persons_faces_are_found_through_the_person_index() {
        let (_dir, lib) = temp_library();
        let plan = lib.query_plan(PERSON_FACES_SQL, &[&1, &None::<i64>, &10, &0]);
        assert!(
            plan.iter().any(|s| s.contains("detected_faces_person")),
            "{plan:?}"
        );
        assert!(!plan.iter().any(|s| s.contains("TEMP B-TREE")), "{plan:?}");
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

    /// A library of five photos with one detected face each: 0 and 1 are Anna's confirmed
    /// faces, 2 is a suggestion for her, 3 carries Picasa's contact "anna" (linked to her by
    /// name) and 4 Picasa's contact "Ben", whom no person is linked to.
    fn anna_and_ben() -> (L, i64) {
        let (l, f) = library(&[&[0.0], &[20.0], &[40.0], &[60.0], &[80.0]]);
        let anna = {
            let w = l.lib.writer();
            w.execute("INSERT INTO people (name) VALUES ('Anna')", [])
                .unwrap();
            w.last_insert_rowid()
        };
        for (face, confirmed) in [(f[0], 1), (f[1], 1), (f[2], 0)] {
            l.lib
                .writer()
                .execute(
                    "UPDATE detected_faces SET person_id = ?2, confirmed = ?3 WHERE id = ?1",
                    rusqlite::params![face, anna, confirmed],
                )
                .unwrap();
        }
        l.lib
            .upsert_contacts(&HashMap::from([
                ("h-anna".to_string(), "anna".to_string()),
                ("h-ben".to_string(), "Ben".to_string()),
            ]))
            .unwrap();
        let picasa = |contact: &str| crate::picasa::Face {
            contact: contact.into(),
            left: 0.5,
            top: 0.5,
            right: 0.6,
            bottom: 0.6,
        };
        l.lib
            .set_item_faces(&[
                (l.items[3], vec![picasa("h-anna")]),
                (l.items[4], vec![picasa("h-ben")]),
            ])
            .unwrap();
        (l, anna)
    }

    fn view_ids(lib: &Library, view: crate::grid::GridView, arg: &str) -> Vec<i64> {
        let mut ids: Vec<i64> = lib
            .entries_for(view, arg)
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn the_people_list_has_named_people_and_unlinked_contacts() {
        let (l, anna) = anna_and_ben();
        assert_eq!(
            l.lib.people_with_counts().unwrap(),
            vec![
                crate::library::Person {
                    key: format!("p:{anna}"),
                    name: "Anna".into(),
                    count: 3
                },
                crate::library::Person {
                    key: "c:h-ben".into(),
                    name: "Ben".into(),
                    count: 1
                },
            ],
            "Anna counts her two confirmed photos and her contact's, not the suggestion's; \
             her contact is not listed on its own"
        );
    }

    #[test]
    fn the_person_view_has_confirmed_faces_and_linked_contacts() {
        let (l, anna) = anna_and_ben();
        assert_eq!(
            view_ids(&l.lib, crate::grid::GridView::Person, &format!("p:{anna}")),
            vec![l.items[0], l.items[1], l.items[3]],
            "the suggestion's photo is not Anna's"
        );
    }

    #[test]
    fn a_contact_key_is_the_picasa_view_and_anything_else_is_empty() {
        let (l, _) = anna_and_ben();
        let person = |arg: &str| view_ids(&l.lib, crate::grid::GridView::Person, arg);
        assert_eq!(person("c:h-ben"), vec![l.items[4]]);
        assert_eq!(
            person("c:h-anna"),
            vec![l.items[3]],
            "a linked contact still opens as the contact"
        );
        assert!(person("h-ben").is_empty(), "no prefix names no one");
        assert!(person("p:x").is_empty());
        assert!(person("").is_empty());
    }

    #[test]
    fn person_search_finds_a_photon_name() {
        let (l, _) = anna_and_ben();
        // Photo 3 is found through the contact's name too, so the two sources agree on it.
        assert_eq!(
            view_ids(&l.lib, crate::grid::GridView::Search, "person:anna"),
            vec![l.items[0], l.items[1], l.items[3]],
            "confirmed faces, not the suggestion's"
        );
        assert_eq!(
            view_ids(&l.lib, crate::grid::GridView::Search, "person:ben"),
            vec![l.items[4]]
        );
    }

    /// Picasa's own photos of a linked contact are the person's, under the person's name:
    /// after a rename the contact still says "anna" and the person says "Anna Smith".
    #[test]
    fn person_search_finds_a_linked_contacts_photos_by_the_persons_name() {
        let (l, anna) = anna_and_ben();
        l.lib.rename_person(anna, "Anna Smith").unwrap();
        assert_eq!(
            view_ids(&l.lib, crate::grid::GridView::Search, "person:smith"),
            vec![l.items[0], l.items[1], l.items[3]],
            "photo 3 has only Picasa's face, recorded under the contact's old name"
        );
    }

    #[test]
    fn hidden_photos_are_in_no_person_reader() {
        let (l, anna) = anna_and_ben();
        l.lib.set_hidden(&[l.items[0]], true).unwrap();
        let list = l.lib.people_with_counts().unwrap();
        assert_eq!(list[0].count, 2, "the list's count drops it");
        assert_eq!(
            view_ids(&l.lib, crate::grid::GridView::Person, &format!("p:{anna}")),
            vec![l.items[1], l.items[3]]
        );
        assert_eq!(
            view_ids(&l.lib, crate::grid::GridView::Search, "person:anna"),
            vec![l.items[1], l.items[3]]
        );
        // With every photo of hers hidden Anna has nothing to count, so the sidebar's list
        // leaves her out (the People page, which lists every named person, does not).
        l.lib.set_hidden(&[l.items[1], l.items[3]], true).unwrap();
        assert!(
            l.lib
                .people_with_counts()
                .unwrap()
                .iter()
                .all(|p| p.key != format!("p:{anna}")),
            "a named person with no visible photo has no row in the sidebar's list"
        );
    }
}
