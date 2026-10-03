//! Picasa's faces and contacts, mirrored into the library by the scanner's post-walk pass.
//!
//! Read only: photon never names a face or writes a contact. The INI is the authority,
//! exactly as it is for stars, and every row here is set from it on every scan of the
//! folder.

use super::Library;
use crate::Result;
use crate::face_detect::Rect;
use crate::picasa::Face;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// One person in the People list: a person the user named, or a Picasa contact no person is
/// linked to, with a face on a visible photo - for a person, a confirmed face of theirs or one
/// Picasa recorded under a contact linked to them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// The argument of the Person view: `p:<id>` for a person photon knows by name, `c:<hash>`
    /// for a Picasa contact no person is linked to.
    pub key: String,
    pub name: String,
    pub count: i64,
}

/// A named face on one photo, with its name resolved: a contact linked to a person carries
/// the person's key and name. The viewer adds a person's confirmed detections in this form.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFace {
    /// The key of the person the face is, as `Person::key`.
    pub key: String,
    pub name: String,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// `people_with_counts`' query, shared with its plan test. It is driven from `faces` and
/// reaches each photo by its id; without the `+` the planner walks `items_size` instead and
/// looks up each photo's faces from there (see `library/mod.rs`).
const PEOPLE_SQL: &str = "SELECT c.hash, c.name, count(DISTINCT f.item_id)
     FROM contacts c
     JOIN faces f ON f.contact = c.hash
     JOIN items i ON i.id = f.item_id
     WHERE +i.missing_since IS NULL AND i.hidden = 0
       AND c.hash NOT IN (SELECT contact FROM person_contacts)
     GROUP BY c.hash";

/// The named people of `people_with_counts`: the photos with a confirmed face of theirs, and
/// those where Picasa recorded a contact linked to them, counted once each. A suggestion
/// (`confirmed = 0`) is not a face of the person yet. Each half is driven from its faces and
/// reaches the photo by id; the live-and-visible test sits inside each half, since a `JOIN
/// items` outside the union made the planner scan every photo (no `+` is needed in this
/// shape: probed, it changes no plan).
const NAMED_PEOPLE_SQL: &str = "SELECT p.id, p.name, count(DISTINCT x.item_id)
     FROM (SELECT d.person_id AS person_id, d.item_id AS item_id
           FROM detected_faces d JOIN items i ON i.id = d.item_id
           WHERE d.confirmed = 1 AND d.person_id IS NOT NULL
             AND i.missing_since IS NULL AND i.hidden = 0
           UNION ALL
           SELECT pc.person_id, f.item_id
           FROM person_contacts pc
           JOIN faces f ON f.contact = pc.contact
           JOIN items i ON i.id = f.item_id
           WHERE i.missing_since IS NULL AND i.hidden = 0) x
     JOIN people p ON p.id = x.person_id
     WHERE p.name IS NOT NULL
     GROUP BY p.id";

/// `folders_with_faces`' query, shared with its plan test. Driven from `faces`, the small
/// side: a library has far fewer faces than photos, and each face reaches its photo by id.
/// The `CROSS JOIN` is what holds that order - SQLite never reorders one. With a plain
/// `JOIN` and no statistics the planner scans every photo through `items_folder` and looks
/// up each one's faces, once per scan.
const FOLDERS_WITH_FACES_SQL: &str =
    "SELECT DISTINCT i.folder_id FROM faces f CROSS JOIN items i ON i.id = f.item_id";

impl Library {
    /// Records or renames contacts. Reads before it writes: a folder whose INI agrees with
    /// the library - nearly every folder on nearly every scan - takes no writer and no
    /// transaction. The writer is one connection every other write queues behind, and a
    /// folder INI that names a face carries a `[Contacts2]` section, rescanned every scan.
    ///
    /// The write repeats the comparison in SQL, so a race between the read and the write
    /// still only renames a name that differs.
    ///
    /// A contact carrying a person's name is that person: one recorded or renamed here is
    /// linked to the named person whose name it now has, unless it is linked already.
    pub fn upsert_contacts(&self, contacts: &HashMap<String, String>) -> Result<()> {
        if contacts.is_empty() {
            return Ok(());
        }
        let differ: Vec<(&String, &String)> = {
            let conn = self.reader()?;
            let mut stmt = conn.prepare_cached("SELECT name FROM contacts WHERE hash = ?1")?;
            let mut differ = Vec::new();
            for (hash, name) in contacts {
                let stored: Option<String> =
                    stmt.query_row(params![hash], |r| r.get(0)).optional()?;
                if stored.as_ref() != Some(name) {
                    differ.push((hash, name));
                }
            }
            differ
        };
        if differ.is_empty() {
            return Ok(());
        }
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO contacts (hash, name) VALUES (?1, ?2)
                 ON CONFLICT(hash) DO UPDATE SET name = excluded.name WHERE name != excluded.name",
            )?;
            for (hash, name) in differ {
                stmt.execute(params![hash, name])?;
            }
        }
        super::people::link_contacts_by_name(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// The folders holding at least one photo with a face, for the Picasa pass to skip the
    /// per-folder face query where neither the INI nor the library has any. Missing photos
    /// are counted too: a folder listed needlessly costs one query, a folder left out would
    /// keep faces its INI dropped.
    pub fn folders_with_faces(&self) -> Result<HashSet<i64>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(FOLDERS_WITH_FACES_SQL)?;
        let ids = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    /// The faces currently stored for every live item in one folder, keyed by item id and
    /// in insertion order, for the Picasa pass to diff against what the INI says now.
    pub fn folder_faces(&self, folder_id: i64) -> Result<HashMap<i64, Vec<Face>>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT f.item_id, f.contact, f.left, f.top, f.right, f.bottom
             FROM faces f JOIN items i ON i.id = f.item_id
             WHERE i.folder_id = ?1 AND i.missing_since IS NULL
             ORDER BY f.rowid",
        )?;
        let mut faces: HashMap<i64, Vec<Face>> = HashMap::new();
        for row in stmt.query_map(params![folder_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                Face {
                    contact: r.get(1)?,
                    left: r.get(2)?,
                    top: r.get(3)?,
                    right: r.get(4)?,
                    bottom: r.get(5)?,
                },
            ))
        })? {
            let (item_id, face) = row?;
            faces.entry(item_id).or_default().push(face);
        }
        Ok(faces)
    }

    /// Replaces the faces of each listed item with the given list, in one transaction.
    pub fn set_item_faces(&self, items: &[(i64, Vec<Face>)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut delete = tx.prepare_cached("DELETE FROM faces WHERE item_id = ?1")?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO faces (item_id, contact, left, top, right, bottom)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (item_id, faces) in items {
                delete.execute(params![item_id])?;
                for f in faces {
                    insert.execute(params![
                        item_id, f.contact, f.left, f.top, f.right, f.bottom
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every face Picasa recorded on one photo, in the unedited picture, with whether its
    /// contact has a name. `item_faces` is the named ones, with the name; this is what the
    /// merge with photon's own detections needs, where an unnamed face is still a face.
    pub fn item_picasa_faces(&self, item_id: i64) -> Result<Vec<(Rect, bool)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT f.left, f.top, f.right, f.bottom,
                    EXISTS (SELECT 1 FROM contacts c WHERE c.hash = f.contact)
             FROM faces f WHERE f.item_id = ?1 ORDER BY f.rowid",
        )?;
        let faces = stmt
            .query_map(params![item_id], |r| {
                Ok((
                    Rect {
                        left: r.get(0)?,
                        top: r.get(1)?,
                        right: r.get(2)?,
                        bottom: r.get(3)?,
                    },
                    r.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(faces)
    }

    /// The named faces on one photo, in the INI's order. A face whose contact no INI has
    /// named is left out: there is nothing to show for it but a hash. A contact linked to a
    /// person is that person: their key and their name.
    pub fn item_faces(&self, item_id: i64) -> Result<Vec<ItemFace>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT CASE WHEN p.id IS NULL THEN 'c:' || f.contact ELSE 'p:' || p.id END,
                    COALESCE(p.name, c.name), f.left, f.top, f.right, f.bottom
             FROM faces f JOIN contacts c ON c.hash = f.contact
             LEFT JOIN person_contacts pc ON pc.contact = f.contact
             LEFT JOIN people p ON p.id = pc.person_id AND p.name IS NOT NULL
             WHERE f.item_id = ?1
             ORDER BY f.rowid",
        )?;
        let faces = stmt
            .query_map(params![item_id], |r| {
                Ok(ItemFace {
                    key: r.get(0)?,
                    name: r.get(1)?,
                    left: r.get(2)?,
                    top: r.get(3)?,
                    right: r.get(4)?,
                    bottom: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(faces)
    }

    /// The confirmed faces of named people on one photo, in the picture as shown, with the
    /// person's id and name, in face order. A suggestion is not here: it is not yet the
    /// person's face.
    pub fn item_named_detected_faces(&self, item_id: i64) -> Result<Vec<(Rect, i64, String)>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT d.left, d.top, d.right, d.bottom, p.id, p.name
             FROM detected_faces d JOIN people p ON p.id = d.person_id
             WHERE d.item_id = ?1 AND d.confirmed = 1 AND p.name IS NOT NULL
             ORDER BY d.id",
        )?;
        let faces = stmt
            .query_map(params![item_id], |r| {
                Ok((
                    Rect {
                        left: r.get(0)?,
                        top: r.get(1)?,
                        right: r.get(2)?,
                        bottom: r.get(3)?,
                    },
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(faces)
    }

    /// Every named person and every contact no person is linked to, each with a face on a
    /// live photo, with how many photos, sorted by name in Rust (case-insensitively;
    /// `lower()` is ASCII-only without ICU).
    ///
    /// Counts photos, not faces: a person twice in one frame is one photo of them.
    pub fn people_with_counts(&self) -> Result<Vec<Person>> {
        let conn = self.reader()?;
        let mut people = conn
            .prepare(NAMED_PEOPLE_SQL)?
            .query_map([], |r| {
                Ok(Person {
                    key: format!("p:{}", r.get::<_, i64>(0)?),
                    name: r.get(1)?,
                    count: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let contacts = conn
            .prepare(PEOPLE_SQL)?
            .query_map([], |r| {
                Ok(Person {
                    key: format!("c:{}", r.get::<_, String>(0)?),
                    name: r.get(1)?,
                    count: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        people.extend(contacts);
        people.sort_by_cached_key(|p| (p.name.to_lowercase(), p.name.clone(), p.key.clone()));
        Ok(people)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GridView;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    fn face(contact: &str) -> Face {
        Face {
            contact: contact.into(),
            left: 0.1,
            top: 0.2,
            right: 0.3,
            bottom: 0.4,
        }
    }

    /// Every Picasa face on the photo, named or not: an unnamed one is still a face, and
    /// the merge has to know it is there or it would hide the detection lying over it.
    #[test]
    fn item_picasa_faces_includes_the_unnamed() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        lib.upsert_contacts(&HashMap::from([("ada".to_string(), "Ada".to_string())]))
            .unwrap();
        lib.set_item_faces(&[(ids[0], vec![face("ada"), face("nobody")])])
            .unwrap();

        let faces = lib.item_picasa_faces(ids[0]).unwrap();
        assert_eq!(faces.len(), 2);
        assert_eq!(
            faces.iter().map(|(_, named)| *named).collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(faces[1].0.left, 0.1);
    }

    #[test]
    fn faces_round_trip_and_resolve_their_names() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        lib.upsert_contacts(&HashMap::from([("ada".to_string(), "Ada".to_string())]))
            .unwrap();
        lib.set_item_faces(&[(ids[0], vec![face("ada"), face("nobody")])])
            .unwrap();

        assert_eq!(
            lib.item_faces(ids[0]).unwrap(),
            vec![ItemFace {
                key: "c:ada".into(),
                name: "Ada".into(),
                left: 0.1,
                top: 0.2,
                right: 0.3,
                bottom: 0.4,
            }],
            "the unnamed face is stored (folder_faces sees it) but not shown"
        );
        assert_eq!(lib.folder_faces(folder).unwrap()[&ids[0]].len(), 2);
        assert!(lib.item_faces(ids[1]).unwrap().is_empty());
    }

    #[test]
    fn people_count_photos_of_live_items_and_sort_by_name() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
                new_item(folder, "/p/c.jpg", 3),
            ])
            .unwrap();
        lib.upsert_contacts(&HashMap::from([
            ("zed".to_string(), "zed".to_string()),
            ("ada".to_string(), "Ada".to_string()),
            ("ghost".to_string(), "Ghost".to_string()),
        ]))
        .unwrap();
        lib.set_item_faces(&[
            // Ada twice in one frame is one photo of Ada.
            (ids[0], vec![face("ada"), face("ada"), face("zed")]),
            (ids[1], vec![face("ada")]),
            (ids[2], vec![face("zed")]),
        ])
        .unwrap();
        lib.mark_missing(&[ids[2]], 1).unwrap();

        let people = lib.people_with_counts().unwrap();
        assert_eq!(
            people,
            vec![
                Person {
                    key: "c:ada".into(),
                    name: "Ada".into(),
                    count: 2
                },
                Person {
                    key: "c:zed".into(),
                    name: "zed".into(),
                    count: 1
                },
            ],
            "sorted case-insensitively, a contact with no live face is absent"
        );
    }

    #[test]
    fn the_person_view_holds_that_contacts_photos_in_grid_order() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let old = lib.upsert_folder(watched, Some(root), "/p/old", 1).unwrap();
        let new = lib.upsert_folder(watched, Some(root), "/p/new", 1).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(old, "/p/old/a.jpg", 1),
                new_item(old, "/p/old/b.jpg", 2),
                new_item(new, "/p/new/c.jpg", 9),
            ])
            .unwrap();
        lib.set_item_faces(&[(ids[1], vec![face("ada")]), (ids[2], vec![face("ada")])])
            .unwrap();

        let view: Vec<i64> = lib
            .entries_for(GridView::Person, "c:ada")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(
            view,
            vec![ids[2], ids[1]],
            "the newer folder first, then old's member"
        );
        assert!(
            lib.entries_for(GridView::Person, "c:nobody")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn renaming_a_contact_updates_only_the_changed_name() {
        let (_dir, lib) = temp_library();
        lib.upsert_contacts(&HashMap::from([("ada".to_string(), "Ada".to_string())]))
            .unwrap();
        lib.upsert_contacts(&HashMap::from([("ada".to_string(), "Ada L.".to_string())]))
            .unwrap();
        let (_dir2, folder) = {
            let (dir, f) = seed_folder(&lib, Path::new("/p"));
            (dir, f)
        };
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        lib.set_item_faces(&[(id, vec![face("ada")])]).unwrap();
        assert_eq!(lib.item_faces(id).unwrap()[0].name, "Ada L.");
    }

    #[test]
    fn purging_an_item_takes_its_faces_with_it() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        lib.set_item_faces(&[(id, vec![face("ada")])]).unwrap();
        lib.purge_items(&[id]).unwrap();
        assert!(lib.folder_faces(folder).unwrap().is_empty());
        let orphans: i64 = lib
            .reader()
            .unwrap()
            .query_row("SELECT count(*) FROM faces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            orphans, 0,
            "ON DELETE CASCADE is on, and foreign keys are enforced"
        );
    }

    /// Pins the `+` in `PEOPLE_SQL`: the list reads only the photos that have a face, each
    /// by id. Without it the planner walks `items_size` over every photo in the library to
    /// find their faces (`library/mod.rs`). Whether `faces` or `contacts` drives is left to
    /// the planner; either reaches photos by id.
    #[test]
    fn the_people_list_reads_photos_by_id_not_through_the_size_index() {
        let (_dir, lib) = temp_library();
        let plan = lib.query_plan(PEOPLE_SQL, &[]);
        assert!(
            !plan.iter().any(|step| step.contains("items_size")),
            "walks the size index: {plan:?}"
        );
        assert!(
            plan.iter()
                .any(|step| step == "SEARCH i USING INTEGER PRIMARY KEY (rowid=?)"),
            "expected each photo to be found by id: {plan:?}"
        );
    }

    /// Pins the shape of `NAMED_PEOPLE_SQL`, as the test above does for the contacts half: the
    /// list reads only the photos that have a face of a person, each by id, and never scans
    /// or walks `items_size` over the whole library. With `items` joined outside the union
    /// the planner scans every photo and builds an automatic index on the union's result.
    #[test]
    fn the_named_people_are_counted_from_their_faces_not_the_size_index() {
        let (_dir, lib) = temp_library();
        let plan = lib.query_plan(NAMED_PEOPLE_SQL, &[]);
        assert!(
            !plan.iter().any(|step| step.starts_with("SCAN i")),
            "scans every photo: {plan:?}"
        );
        assert!(
            !plan.iter().any(|step| step.contains("items_size")),
            "walks the size index: {plan:?}"
        );
        assert!(
            plan.iter()
                .any(|step| step == "SEARCH i USING INTEGER PRIMARY KEY (rowid=?)"),
            "expected each photo to be found by id: {plan:?}"
        );
    }

    /// Every folder INI that names a face carries a `[Contacts2]` section, and taking
    /// the writer for each one queued every scan's folders behind one transaction apiece -
    /// even though the `WHERE` in the upsert then changed nothing. Reverting the read before
    /// the write takes the writer here once more.
    #[test]
    fn contacts_the_library_already_has_take_no_writer() {
        let (_dir, lib) = temp_library();
        let contacts = HashMap::from([
            ("ada".to_string(), "Ada".to_string()),
            ("bob".to_string(), "Bob".to_string()),
        ]);
        lib.upsert_contacts(&contacts).unwrap();

        let before = lib.writes_for_test();
        lib.upsert_contacts(&contacts).unwrap();
        assert_eq!(
            lib.writes_for_test(),
            before,
            "an agreeing list writes nothing"
        );

        let renamed = HashMap::from([("bob".to_string(), "Robert".to_string())]);
        lib.upsert_contacts(&renamed).unwrap();
        assert_eq!(lib.writes_for_test(), before + 1, "a rename still writes");
        let name: String = lib
            .reader()
            .unwrap()
            .query_row("SELECT name FROM contacts WHERE hash = 'bob'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(name, "Robert");
    }

    /// `folders_with_faces` reads every face in the library, once per scan. Pins the `CROSS
    /// JOIN`: with a plain one the planner scans every photo instead.
    #[test]
    fn the_folders_with_faces_are_found_from_the_faces() {
        let (_dir, lib) = temp_library();
        let plan = lib.query_plan(FOLDERS_WITH_FACES_SQL, &[]);
        assert!(
            plan.iter()
                .any(|step| step == "SEARCH i USING INTEGER PRIMARY KEY (rowid=?)"),
            "expected each photo to be found by id: {plan:?}"
        );
    }
}
