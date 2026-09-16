//! photon's own albums: virtual collections that live only in `library.db`.
//!
//! Nothing on disk changes when an album does. Membership is by item id, which is the one
//! limitation worth knowing: a photo renamed on disk is a new row to the scanner, and the
//! old row is purged two scans later with its memberships.

use super::Library;
use crate::{Error, Result};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

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
}

/// A trimmed, non-empty album name, or the error the UI shows.
fn valid_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::EmptyAlbumName);
    }
    Ok(name)
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

    pub fn rename_album(&self, id: i64, name: &str) -> Result<()> {
        let name = valid_name(name)?;
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
    /// `lower()` is ASCII-only without ICU). An empty album is listed with a count of 0.
    pub fn albums_with_counts(&self) -> Result<Vec<AlbumSummary>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT a.id, a.name,
                    (SELECT count(*) FROM album_items m JOIN items i ON i.id = m.item_id
                     WHERE m.album_id = a.id AND i.missing_since IS NULL)
             FROM albums a",
        )?;
        let mut albums = stmt
            .query_map([], |r| {
                Ok(AlbumSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    count: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        albums.sort_by_cached_key(|a| (a.name.to_lowercase(), a.name.clone(), a.id));
        Ok(albums)
    }

    /// Adds photos to an album. Already-members are left alone, so adding twice is not an
    /// error and does not move the photo's `added_ms`.
    pub fn add_to_album(&self, album_id: i64, item_ids: &[i64], now_ms: i64) -> Result<()> {
        if self.album(album_id)?.is_none() {
            return Err(Error::NotFound(album_id));
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GridView;
    use crate::testutil::{new_item, seed_folder, temp_library};
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
                    count: 0
                },
                AlbumSummary {
                    id: trip.id,
                    name: "Trip".into(),
                    count: 0
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
}
