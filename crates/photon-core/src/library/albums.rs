//! Albums: virtual collections of photos, of two kinds.
//!
//! photon's own albums live only in `library.db`, created, renamed and deleted here. Nothing
//! on disk changes when one does. Membership is by item id, which is the one limitation worth
//! knowing: a photo renamed on disk is a new row to the scanner, and the old row is purged two
//! scans later with its memberships.
//!
//! Picasa's albums are mirrored by the scan from each folder's `.picasa.ini`, token-bearing
//! (`picasa_token`) and read-only here: written only by the functions under "Picasa albums,
//! written only by the scan" below, never by the user-facing create/rename/delete calls above.

use super::Library;
use crate::{Error, Result};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: i64,
    pub name: String,
    pub created_ms: i64,
}

/// An album as the sidebar lists it, with how many live photos it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumSummary {
    pub id: i64,
    pub name: String,
    pub count: i64,
    /// Mirrored from Picasa's INI by the scan: listed and viewable, never edited here.
    pub picasa: bool,
}

/// A trimmed, non-empty album name, or the error the UI shows.
fn valid_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::EmptyAlbumName);
    }
    Ok(name)
}

/// Whether an INI written at `named_at` saying `name` renames an album stored as `stored`,
/// last named by an INI written at `stored_at`. Only a newer INI renames: the SQL in
/// `upsert_picasa_albums` says the same, and the two must not drift.
fn takes_name(stored: &str, stored_at: Option<i64>, name: &str, named_at: i64) -> bool {
    stored != name && stored_at.is_none_or(|at| named_at > at)
}

impl Library {
    pub fn create_album(&self, name: &str, now_ms: i64) -> Result<Album> {
        let name = valid_name(name)?;
        let conn = self.writer();
        conn.execute(
            "INSERT INTO albums (name, created_ms) VALUES (?1, ?2)",
            params![name, now_ms],
        )?;
        Ok(Album {
            id: conn.last_insert_rowid(),
            name: name.to_string(),
            created_ms: now_ms,
        })
    }

    /// Refuses anything but one of photon's own albums: `NotFound` for no album,
    /// `PicasaAlbum` for one the scan mirrors from Picasa. Every photon write to an album
    /// asks this first; the UI hides those actions on a Picasa album, and this is what makes
    /// that true over IPC. The scan writes a Picasa album's members through
    /// `set_picasa_album_items`, which never comes here.
    fn own_album(&self, id: i64) -> Result<()> {
        let token: Option<Option<String>> = self
            .reader()?
            .query_row(
                "SELECT picasa_token FROM albums WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        match token {
            None => Err(Error::NotFound(id)),
            Some(Some(_)) => Err(Error::PicasaAlbum(id)),
            Some(None) => Ok(()),
        }
    }

    pub fn rename_album(&self, id: i64, name: &str) -> Result<()> {
        let name = valid_name(name)?;
        self.own_album(id)?;
        let changed = self.writer().execute(
            "UPDATE albums SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    /// Deletes an album and its memberships. Photos are untouched, on disk and in the
    /// library.
    pub fn delete_album(&self, id: i64) -> Result<()> {
        self.own_album(id)?;
        let changed = self
            .writer()
            .execute("DELETE FROM albums WHERE id = ?1", params![id])?;
        if changed == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    pub fn album(&self, id: i64) -> Result<Option<Album>> {
        let album = self
            .reader()?
            .query_row(
                "SELECT id, name, created_ms FROM albums WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Album {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        created_ms: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(album)
    }

    /// Every album with its live photo count, sorted by name in Rust (case-insensitively;
    /// `lower()` is ASCII-only without ICU). An empty photon album is listed with a count of
    /// 0; an empty Picasa album is left out, since it has lost every photo it once mirrored.
    pub fn albums_with_counts(&self) -> Result<Vec<AlbumSummary>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT a.id, a.name, a.picasa_token IS NOT NULL,
                    (SELECT count(*) FROM album_items m JOIN items i ON i.id = m.item_id
                     WHERE m.album_id = a.id AND i.missing_since IS NULL AND i.hidden = 0)
             FROM albums a",
        )?;
        let mut albums = stmt
            .query_map([], |r| {
                Ok(AlbumSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    picasa: r.get(2)?,
                    count: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // A Picasa album with no live photo has left this library; its row stays, so a
        // folder that comes back finds the same album. An empty photon album is one the user
        // just made, and stays listed.
        albums.retain(|a| !a.picasa || a.count > 0);
        albums.sort_by_cached_key(|a| (a.name.to_lowercase(), a.name.clone(), a.id));
        Ok(albums)
    }

    /// Adds photos to an album. Already-members are left alone, so adding twice is not an
    /// error and does not move the photo's `added_ms`.
    pub fn add_to_album(&self, album_id: i64, item_ids: &[i64], now_ms: i64) -> Result<()> {
        self.own_album(album_id)?;
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO album_items (album_id, item_id, added_ms) VALUES (?1, ?2, ?3)",
            )?;
            for id in item_ids {
                stmt.execute(params![album_id, id, now_ms])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_from_album(&self, album_id: i64, item_ids: &[i64]) -> Result<()> {
        self.own_album(album_id)?;
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt =
                tx.prepare_cached("DELETE FROM album_items WHERE album_id = ?1 AND item_id = ?2")?;
            for id in item_ids {
                stmt.execute(params![album_id, id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The ids of the albums one photo is in.
    pub fn item_albums(&self, item_id: i64) -> Result<Vec<i64>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT album_id FROM album_items WHERE item_id = ?1 ORDER BY album_id",
        )?;
        let ids = stmt
            .query_map(params![item_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids)
    }

    // ---- Picasa albums, written only by the scan ----

    /// Inserts the albums one INI defines and the tokens its photos name, and returns every
    /// one's id by token, with how many albums it inserted or renamed.
    ///
    /// A defined album takes the INI's name, so a rename in Picasa is followed - but only
    /// from an INI newer than the one that named it last (`named_at`, the INI's mtime). Picasa
    /// rewrites every member folder's INI when it renames an album, so the rename is always
    /// the newest file, while a stale copy of a folder (a backup made before the rename)
    /// is older; without the age, the two took turns naming the album on every scan. A token
    /// that is only referenced is inserted under the token itself and never renames anything.
    /// A rescan of an unchanged INI writes nothing and counts 0 - counting it would rebuild
    /// the grid after every scan of a Picasa library.
    ///
    /// Reads before it writes: nearly every scan finds every album as it left it, and the
    /// writer is one connection that every other write queues behind. The write repeats the
    /// same conditions in SQL, so a scan racing another between the read and the write still
    /// applies the rule rather than whatever the read saw.
    pub fn upsert_picasa_albums(
        &self,
        defined: &HashMap<String, String>,
        referenced: &HashSet<String>,
        named_at: i64,
        now_ms: i64,
    ) -> Result<(HashMap<String, i64>, u64)> {
        let mut ids = HashMap::new();
        if defined.is_empty() && referenced.is_empty() {
            return Ok((ids, 0));
        }
        let mut must_write = false;
        {
            let conn = self.reader()?;
            let mut stmt = conn.prepare_cached(
                "SELECT id, name, picasa_named_at FROM albums WHERE picasa_token = ?1",
            )?;
            for token in defined.keys().chain(referenced) {
                if ids.contains_key(token) {
                    continue;
                }
                let row = stmt
                    .query_row(params![token], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<i64>>(2)?,
                        ))
                    })
                    .optional()?;
                match row {
                    None => must_write = true,
                    Some((id, stored, stored_at)) => {
                        if let Some(name) = defined.get(token)
                            && takes_name(&stored, stored_at, name, named_at)
                        {
                            must_write = true;
                        }
                        ids.insert(token.clone(), id);
                    }
                }
            }
        }
        if !must_write {
            return Ok((ids, 0));
        }
        let mut changed = 0u64;
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            // The same rule as `takes_name`, in SQL: the two must agree, or the read decides
            // to write and the write then does something the read did not predict.
            let mut define = tx.prepare_cached(
                "INSERT INTO albums (name, created_ms, picasa_token, picasa_named_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(picasa_token) DO UPDATE
                 SET name = excluded.name, picasa_named_at = excluded.picasa_named_at
                 WHERE name != excluded.name
                   AND (picasa_named_at IS NULL OR excluded.picasa_named_at > picasa_named_at)",
            )?;
            let mut refer = tx.prepare_cached(
                "INSERT INTO albums (name, created_ms, picasa_token) VALUES (?1, ?2, ?1)
                 ON CONFLICT(picasa_token) DO NOTHING",
            )?;
            let mut id_of = tx.prepare_cached("SELECT id FROM albums WHERE picasa_token = ?1")?;
            for (token, name) in defined {
                changed += define.execute(params![name, now_ms, token, named_at])? as u64;
            }
            for token in referenced.iter().filter(|t| !defined.contains_key(*t)) {
                changed += refer.execute(params![token, now_ms])? as u64;
            }
            ids.clear();
            for token in defined.keys().chain(referenced) {
                if !ids.contains_key(token) {
                    let id: i64 = id_of.query_row(params![token], |r| r.get(0))?;
                    ids.insert(token.clone(), id);
                }
            }
        }
        tx.commit()?;
        Ok((ids, changed))
    }

    /// The Picasa-album memberships of every live photo in one folder, by item id, for the
    /// Picasa pass to diff against what the INI says now. photon's own albums are left out:
    /// the pass never touches them.
    pub fn folder_picasa_albums(&self, folder_id: i64) -> Result<HashMap<i64, BTreeSet<i64>>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT m.item_id, m.album_id
             FROM album_items m
             JOIN items i ON i.id = m.item_id
             JOIN albums a ON a.id = m.album_id
             WHERE i.folder_id = ?1 AND i.missing_since IS NULL AND a.picasa_token IS NOT NULL",
        )?;
        let mut memberships: HashMap<i64, BTreeSet<i64>> = HashMap::new();
        for row in stmt.query_map(params![folder_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })? {
            let (item_id, album_id) = row?;
            memberships.entry(item_id).or_default().insert(album_id);
        }
        Ok(memberships)
    }

    /// Replaces each listed photo's Picasa-album memberships with the given set, in one
    /// transaction. Only Picasa albums' rows are deleted, so a photo keeps its photon albums.
    /// The album ids come from `upsert_picasa_albums`, which is why they are not checked
    /// here, and why this is the scan's writer and not a command's.
    pub fn set_picasa_album_items(
        &self,
        items: &[(i64, BTreeSet<i64>)],
        now_ms: i64,
    ) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut clear = tx.prepare_cached(
                "DELETE FROM album_items WHERE item_id = ?1
                 AND album_id IN (SELECT id FROM albums WHERE picasa_token IS NOT NULL)",
            )?;
            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO album_items (album_id, item_id, added_ms) VALUES (?1, ?2, ?3)",
            )?;
            for (item_id, albums) in items {
                clear.execute(params![item_id])?;
                for album_id in albums {
                    insert.execute(params![album_id, item_id, now_ms])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GridView;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::collections::{BTreeSet, HashMap, HashSet};
    use std::path::Path;

    #[test]
    fn albums_are_created_renamed_listed_and_deleted() {
        let (_dir, lib) = temp_library();
        let trip = lib.create_album("  Trip  ", 10).unwrap();
        assert_eq!(trip.name, "Trip", "trimmed");
        let alpha = lib.create_album("alpha", 11).unwrap();
        assert!(matches!(
            lib.create_album("   ", 12),
            Err(Error::EmptyAlbumName)
        ));

        assert_eq!(
            lib.albums_with_counts().unwrap(),
            vec![
                AlbumSummary {
                    id: alpha.id,
                    name: "alpha".into(),
                    count: 0,
                    picasa: false
                },
                AlbumSummary {
                    id: trip.id,
                    name: "Trip".into(),
                    count: 0,
                    picasa: false
                },
            ],
            "sorted case-insensitively, empties listed"
        );

        lib.rename_album(trip.id, "Zurich").unwrap();
        assert_eq!(lib.album(trip.id).unwrap().unwrap().name, "Zurich");
        assert!(matches!(
            lib.rename_album(trip.id, ""),
            Err(Error::EmptyAlbumName)
        ));
        assert!(matches!(
            lib.rename_album(999, "x"),
            Err(Error::NotFound(999))
        ));

        lib.delete_album(trip.id).unwrap();
        assert_eq!(lib.album(trip.id).unwrap(), None);
        assert!(matches!(lib.delete_album(trip.id), Err(Error::NotFound(_))));
    }

    #[test]
    fn membership_is_idempotent_and_counts_only_live_photos() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        let album = lib.create_album("Trip", 1).unwrap();

        lib.add_to_album(album.id, &ids, 5).unwrap();
        lib.add_to_album(album.id, &ids[..1], 6).unwrap();
        assert_eq!(lib.item_albums(ids[0]).unwrap(), vec![album.id]);
        assert_eq!(lib.albums_with_counts().unwrap()[0].count, 2);
        assert!(matches!(
            lib.add_to_album(999, &ids, 5),
            Err(Error::NotFound(999))
        ));

        lib.mark_missing(&[ids[1]], 1).unwrap();
        assert_eq!(lib.albums_with_counts().unwrap()[0].count, 1);

        lib.remove_from_album(album.id, &ids[..1]).unwrap();
        assert!(lib.item_albums(ids[0]).unwrap().is_empty());
        assert_eq!(lib.albums_with_counts().unwrap()[0].count, 0);
    }

    #[test]
    fn the_album_view_places_a_folder_by_its_oldest_member() {
        // Same property as Starred: alpha's oldest *member* (9) is newer than zulu's (5),
        // so alpha leads, though by its oldest photo overall (1) it would trail. An
        // implementation filtering only the outer WHERE passes every other album test and
        // fails this one.
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
                new_item(alpha, "/p/alpha/old.jpg", 1),
                new_item(alpha, "/p/alpha/new.jpg", 9),
                new_item(zulu, "/p/zulu/mid.jpg", 5),
            ])
            .unwrap();
        let album = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(album.id, &[ids[1], ids[2]], 1).unwrap();

        let folders: Vec<i64> = lib
            .entries_for(GridView::Album, &album.id.to_string())
            .unwrap()
            .iter()
            .map(|e| e.folder_id)
            .collect();
        assert_eq!(folders, [alpha, zulu]);
        assert!(
            lib.entries_for(GridView::Album, "not-an-id")
                .unwrap()
                .is_empty()
        );
        assert!(lib.entries_for(GridView::Album, "999").unwrap().is_empty());
    }

    #[test]
    fn purging_a_photo_removes_its_memberships() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let album = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(album.id, &[id], 1).unwrap();
        lib.purge_items(&[id]).unwrap();
        let rows: i64 = lib
            .reader()
            .unwrap()
            .query_row("SELECT count(*) FROM album_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    /// A Picasa album as the scan would leave it, before the scan's own writer exists.
    fn picasa_album(lib: &Library, token: &str, name: &str) -> i64 {
        let conn = lib.writer();
        conn.execute(
            "INSERT INTO albums (name, created_ms, picasa_token) VALUES (?1, 0, ?2)",
            params![name, token],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn a_picasa_album_cannot_be_renamed() {
        let (_dir, lib) = temp_library();
        let id = picasa_album(&lib, "t", "Holiday");
        assert!(matches!(
            lib.rename_album(id, "Mine"),
            Err(Error::PicasaAlbum(_))
        ));
        assert_eq!(lib.album(id).unwrap().unwrap().name, "Holiday");
        let own = lib.create_album("Trip", 1).unwrap();
        lib.rename_album(own.id, "Zurich").unwrap();
    }

    #[test]
    fn a_picasa_album_cannot_be_deleted() {
        let (_dir, lib) = temp_library();
        let id = picasa_album(&lib, "t", "Holiday");
        assert!(matches!(lib.delete_album(id), Err(Error::PicasaAlbum(_))));
        assert!(lib.album(id).unwrap().is_some());
        let own = lib.create_album("Trip", 1).unwrap();
        lib.delete_album(own.id).unwrap();
    }

    #[test]
    fn nothing_can_be_added_to_a_picasa_album() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        let id = picasa_album(&lib, "t", "Holiday");
        assert!(matches!(
            lib.add_to_album(id, &ids, 5),
            Err(Error::PicasaAlbum(_))
        ));
        assert!(lib.item_albums(ids[0]).unwrap().is_empty());
        let own = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(own.id, &ids, 5).unwrap();
    }

    #[test]
    fn nothing_can_be_removed_from_a_picasa_album() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        let id = picasa_album(&lib, "t", "Holiday");
        lib.writer()
            .execute(
                "INSERT INTO album_items (album_id, item_id, added_ms) VALUES (?1, ?2, 0)",
                params![id, ids[0]],
            )
            .unwrap();
        assert!(matches!(
            lib.remove_from_album(id, &ids),
            Err(Error::PicasaAlbum(_))
        ));
        assert_eq!(lib.item_albums(ids[0]).unwrap(), vec![id]);
        let own = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(own.id, &ids, 5).unwrap();
        lib.remove_from_album(own.id, &ids).unwrap();
    }

    #[test]
    fn albums_with_counts_leaves_out_an_empty_picasa_album_only() {
        // An empty Picasa album has lost every photo in this library and leaves the list; an
        // empty photon album is one the user just made and stays. A Picasa album whose only
        // member is hidden counts 0 like every hidden-aware count, and leaves too.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/h.jpg", 2),
            ])
            .unwrap();
        let (shown, hidden) = (ids[0], ids[1]);
        lib.set_hidden(&[hidden], true).unwrap();
        let empty_picasa = picasa_album(&lib, "e", "Gone");
        let hidden_only = picasa_album(&lib, "h", "Only hidden");
        let listed = picasa_album(&lib, "l", "Holiday");
        let own = lib.create_album("Trip", 1).unwrap();
        for (album, item) in [(listed, shown), (hidden_only, hidden)] {
            lib.writer()
                .execute(
                    "INSERT INTO album_items (album_id, item_id, added_ms) VALUES (?1, ?2, 0)",
                    params![album, item],
                )
                .unwrap();
        }

        let rows: Vec<(i64, bool, i64)> = lib
            .albums_with_counts()
            .unwrap()
            .into_iter()
            .map(|a| (a.id, a.picasa, a.count))
            .collect();
        assert_eq!(rows, vec![(listed, true, 1), (own.id, false, 0)]);
        assert!(
            lib.album(empty_picasa).unwrap().is_some(),
            "left out, not deleted"
        );
    }

    #[test]
    fn a_referenced_token_never_replaces_a_real_name_in_either_order() {
        let named = HashMap::from([("t".to_string(), "Holiday".to_string())]);
        let only_t = HashSet::from(["t".to_string()]);

        // The definition first, then a folder whose photo only names the token.
        let (_dir, lib) = temp_library();
        lib.upsert_picasa_albums(&named, &HashSet::new(), 1, 1)
            .unwrap();
        let (ids, changed) = lib
            .upsert_picasa_albums(&HashMap::new(), &only_t, 2, 2)
            .unwrap();
        assert_eq!(changed, 0);
        assert_eq!(lib.album(ids["t"]).unwrap().unwrap().name, "Holiday");

        // The reference first: listed under its token until a definition names it.
        let (_dir2, lib2) = temp_library();
        let (first, changed) = lib2
            .upsert_picasa_albums(&HashMap::new(), &only_t, 1, 1)
            .unwrap();
        assert_eq!(changed, 1);
        assert_eq!(lib2.album(first["t"]).unwrap().unwrap().name, "t");
        let (second, changed) = lib2.upsert_picasa_albums(&named, &only_t, 2, 2).unwrap();
        assert_eq!(second["t"], first["t"], "the same album, not a second one");
        assert_eq!(changed, 1);
        assert_eq!(lib2.album(first["t"]).unwrap().unwrap().name, "Holiday");
    }

    #[test]
    fn upserting_an_unchanged_definition_counts_nothing_and_a_rename_counts_one() {
        // Counting a no-op would make every scan of a Picasa library rebuild the grid.
        let (_dir, lib) = temp_library();
        let holiday = HashMap::from([("t".to_string(), "Holiday".to_string())]);
        assert_eq!(
            lib.upsert_picasa_albums(&holiday, &HashSet::new(), 1, 1)
                .unwrap()
                .1,
            1
        );
        assert_eq!(
            lib.upsert_picasa_albums(&holiday, &HashSet::new(), 2, 2)
                .unwrap()
                .1,
            0
        );
        let summer = HashMap::from([("t".to_string(), "Summer".to_string())]);
        let (ids, changed) = lib
            .upsert_picasa_albums(&summer, &HashSet::new(), 3, 3)
            .unwrap();
        assert_eq!(changed, 1);
        assert_eq!(lib.album(ids["t"]).unwrap().unwrap().name, "Summer");
    }

    #[test]
    fn the_scans_membership_writer_never_touches_photons_albums() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        let own = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(own.id, &ids, 1).unwrap();
        let (picasa, _) = lib
            .upsert_picasa_albums(
                &HashMap::from([("t".to_string(), "Holiday".to_string())]),
                &HashSet::new(),
                1,
                1,
            )
            .unwrap();

        lib.set_picasa_album_items(&[(ids[0], BTreeSet::from([picasa["t"]]))], 2)
            .unwrap();
        assert_eq!(lib.item_albums(ids[0]).unwrap(), vec![own.id, picasa["t"]]);
        assert_eq!(
            lib.folder_picasa_albums(folder).unwrap(),
            HashMap::from([(ids[0], BTreeSet::from([picasa["t"]]))]),
            "photon's album is not reported to the pass"
        );

        lib.set_picasa_album_items(&[(ids[0], BTreeSet::new())], 3)
            .unwrap();
        assert_eq!(lib.item_albums(ids[0]).unwrap(), vec![own.id]);
    }

    #[test]
    fn an_older_definition_never_renames_a_newer_one() {
        // A stale copy of a folder (a backup written before a rename in Picasa) must not take
        // turns with the renamed folders: the INI written last is the one Picasa renamed.
        let summer = HashMap::from([("t".to_string(), "Summer".to_string())]);
        let holiday = HashMap::from([("t".to_string(), "Holiday".to_string())]);

        // The newer INI first, the stale one second.
        let (_dir, lib) = temp_library();
        let (ids, _) = lib
            .upsert_picasa_albums(&summer, &HashSet::new(), 200, 1)
            .unwrap();
        // The stale INI also names a new token, so the write happens and the SQL, not the
        // read, is what has to refuse the rename.
        let (_, changed) = lib
            .upsert_picasa_albums(&holiday, &HashSet::from(["u".to_string()]), 100, 2)
            .unwrap();
        assert_eq!(changed, 1, "only the new token is inserted");
        assert_eq!(lib.album(ids["t"]).unwrap().unwrap().name, "Summer");

        // The stale one first: the newer INI still wins.
        let (_dir2, lib2) = temp_library();
        let (ids, _) = lib2
            .upsert_picasa_albums(&holiday, &HashSet::new(), 100, 1)
            .unwrap();
        let (_, changed) = lib2
            .upsert_picasa_albums(&summer, &HashSet::new(), 200, 2)
            .unwrap();
        assert_eq!(changed, 1);
        assert_eq!(lib2.album(ids["t"]).unwrap().unwrap().name, "Summer");

        // Two INIs of the same age: the name read first stays, so nothing flips.
        let (_, changed) = lib2
            .upsert_picasa_albums(&holiday, &HashSet::new(), 200, 3)
            .unwrap();
        assert_eq!(changed, 0);
        assert_eq!(lib2.album(ids["t"]).unwrap().unwrap().name, "Summer");
    }

    #[test]
    fn an_unchanged_definition_takes_no_write_lock() {
        // Every scan applies every walked folder's INI, so a Picasa library of thousands of
        // folders used to open thousands of empty write transactions per scan, each one
        // queueing behind whatever else held the single writer.
        let (_dir, lib) = temp_library();
        let holiday = HashMap::from([("t".to_string(), "Holiday".to_string())]);
        let only_u = HashSet::from(["u".to_string()]);
        lib.upsert_picasa_albums(&holiday, &only_u, 1, 1).unwrap();
        let before = lib.writes_for_test();
        let (ids, changed) = lib.upsert_picasa_albums(&holiday, &only_u, 1, 2).unwrap();
        assert_eq!(changed, 0);
        assert_eq!(
            lib.writes_for_test(),
            before,
            "nothing to write, so no writer"
        );
        assert_eq!(ids.len(), 2, "the ids still come back, from the read");

        // A stale INI naming it differently has nothing to write either: it cannot rename.
        let older = HashMap::from([("t".to_string(), "Old name".to_string())]);
        lib.upsert_picasa_albums(&older, &only_u, 0, 3).unwrap();
        assert_eq!(
            lib.writes_for_test(),
            before,
            "an older INI takes no writer"
        );

        let summer = HashMap::from([("t".to_string(), "Summer".to_string())]);
        lib.upsert_picasa_albums(&summer, &only_u, 2, 4).unwrap();
        assert_eq!(lib.writes_for_test(), before + 1, "a rename still writes");
        assert_eq!(lib.album(ids["t"]).unwrap().unwrap().name, "Summer");
    }
}
