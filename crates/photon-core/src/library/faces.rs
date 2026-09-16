//! Picasa's faces and contacts, mirrored into the library by the scanner's post-walk pass.
//!
//! Read only: photon never names a face or writes a contact. The INI is the authority,
//! exactly as it is for stars, and every row here is set from it on every scan of the
//! folder.

use super::Library;
use crate::Result;
use crate::picasa::Face;
use rusqlite::params;
use serde::Serialize;
use std::collections::HashMap;

/// One person in the People list: a contact with at least one face on a live photo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// Picasa's contact hash, the argument of the Person view.
    pub hash: String,
    pub name: String,
    pub count: i64,
}

/// A face on one photo, with the contact's name resolved.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFace {
    pub hash: String,
    pub name: String,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Library {
    /// Records or renames contacts. A name that has not changed is not written, so a
    /// folder whose INI agrees with the library costs no transaction here.
    pub fn upsert_contacts(&self, contacts: &HashMap<String, String>) -> Result<()> {
        if contacts.is_empty() {
            return Ok(());
        }
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO contacts (hash, name) VALUES (?1, ?2)
                 ON CONFLICT(hash) DO UPDATE SET name = excluded.name WHERE name != excluded.name",
            )?;
            for (hash, name) in contacts {
                stmt.execute(params![hash, name])?;
            }
        }
        tx.commit()?;
        Ok(())
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

    /// The named faces on one photo, in the INI's order. A face whose contact no INI has
    /// named is left out: there is nothing to show for it but a hash.
    pub fn item_faces(&self, item_id: i64) -> Result<Vec<ItemFace>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT f.contact, c.name, f.left, f.top, f.right, f.bottom
             FROM faces f JOIN contacts c ON c.hash = f.contact
             WHERE f.item_id = ?1
             ORDER BY f.rowid",
        )?;
        let faces = stmt
            .query_map(params![item_id], |r| {
                Ok(ItemFace {
                    hash: r.get(0)?,
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

    /// Every named contact with a face on a live photo, with how many photos, sorted by
    /// name in Rust (case-insensitively; `lower()` is ASCII-only without ICU).
    ///
    /// Counts photos, not faces: a person twice in one frame is one photo of them.
    pub fn people_with_counts(&self) -> Result<Vec<Person>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT c.hash, c.name, count(DISTINCT f.item_id)
             FROM contacts c
             JOIN faces f ON f.contact = c.hash
             JOIN items i ON i.id = f.item_id
             WHERE i.missing_since IS NULL
             GROUP BY c.hash",
        )?;
        let mut people = stmt
            .query_map([], |r| {
                Ok(Person {
                    hash: r.get(0)?,
                    name: r.get(1)?,
                    count: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        people.sort_by_cached_key(|p| (p.name.to_lowercase(), p.name.clone(), p.hash.clone()));
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
                hash: "ada".into(),
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
                    hash: "ada".into(),
                    name: "Ada".into(),
                    count: 2
                },
                Person {
                    hash: "zed".into(),
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
            .entries_for(GridView::Person, "ada")
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
            lib.entries_for(GridView::Person, "nobody")
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
}
