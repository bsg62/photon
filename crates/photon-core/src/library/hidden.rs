//! Hidden photos: Picasa's Hide. A hidden photo leaves every view, count and group and
//! waits in the Hidden view; the file on disk is untouched. What "every view" means is
//! enforced where the grid is queried (`items::Shown`) and in each count's own query - this
//! is only the flag's write, its count and its view.

use super::Library;
use super::items::{GRID_COLUMNS, Shown, grid_query, map_grid_row};
use crate::Result;
use crate::grid::GridEntry;
use rusqlite::params;

impl Library {
    /// Hides or unhides photos, returning how many actually changed. A photo already in the
    /// state asked for is not counted, so the caller can tell a no-op from a change and skip
    /// the grid rebuild; a missing photo is skipped, as `set_stars` skips one.
    pub fn set_hidden(&self, item_ids: &[i64], hidden: bool) -> Result<usize> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let mut changed = 0;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE items SET hidden = ?2
                 WHERE id = ?1 AND hidden <> ?2 AND missing_since IS NULL",
            )?;
            for id in item_ids {
                changed += stmt.execute(params![id, hidden])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// How many live photos are hidden: the sidebar's Hidden row, which shows only while
    /// this is non-zero. Served by `items_hidden`.
    pub fn hidden_count(&self) -> Result<usize> {
        let conn = self.reader()?;
        let count: i64 = conn.query_row(HIDDEN_COUNT_SQL, [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// The Hidden view's rows, in the same folder-first order as every other filter view.
    pub(super) fn hidden_entries(&self) -> Result<Vec<GridEntry>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(&grid_query(GRID_COLUMNS, Shown::Hidden, ""))?;
        let rows = stmt
            .query_map([], map_grid_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// Shared with its plan test, so the two cannot drift onto different strings.
const HIDDEN_COUNT_SQL: &str =
    "SELECT COUNT(*) FROM items WHERE hidden = 1 AND missing_since IS NULL";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GridView;
    use crate::library::NewItem;
    use crate::picasa::Face;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::collections::HashMap;
    use std::path::Path;

    fn ids(entries: Vec<GridEntry>) -> Vec<i64> {
        entries.iter().map(|e| e.id).collect()
    }

    fn view(lib: &Library, view: GridView, arg: &str) -> Vec<i64> {
        ids(lib.entries_for(view, arg).unwrap())
    }

    /// Two photos that would each be in every view there is: starred, with a face, in an
    /// album, keyworded, recent, matching a search, and byte-identical to each other so
    /// both are in Duplicates and in each other's Copies. Returns the album id with them.
    fn everywhere(lib: &Library) -> (Vec<i64>, i64) {
        let (_w, folder) = seed_folder(lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/beach-1.jpg", 1),
                new_item(folder, "/p/beach-2.jpg", 2),
            ])
            .unwrap();
        lib.set_ratings(&[(ids[0], 3), (ids[1], 3)]).unwrap();
        lib.upsert_contacts(&HashMap::from([("ada".to_string(), "Ada".to_string())]))
            .unwrap();
        let face = || Face {
            contact: "ada".to_string(),
            left: 0.1,
            top: 0.1,
            right: 0.2,
            bottom: 0.2,
        };
        lib.set_item_faces(&[(ids[0], vec![face()]), (ids[1], vec![face()])])
            .unwrap();
        let album = lib.create_album("Trip", 1).unwrap();
        lib.add_to_album(album.id, &ids, 1).unwrap();
        lib.add_items_tag(&ids, "sea").unwrap();
        lib.writer()
            .execute("UPDATE items SET content_hash = x'0102'", [])
            .unwrap();
        (ids, album.id)
    }

    /// Every view a photo can be in, with the argument that puts these photos in it.
    fn views(ids: &[i64], album: i64) -> Vec<(GridView, String)> {
        vec![
            (GridView::All, String::new()),
            (GridView::Starred, String::new()),
            (GridView::Recent, String::new()),
            (GridView::Search, "beach".to_string()),
            (GridView::Person, "ada".to_string()),
            (GridView::Album, album.to_string()),
            (GridView::Tag, "sea".to_string()),
            (GridView::Duplicates, String::new()),
            (GridView::Copies, ids[1].to_string()),
        ]
    }

    #[test]
    fn a_hidden_photo_leaves_every_view() {
        let (_dir, lib) = temp_library();
        let (ids, album) = everywhere(&lib);
        // The fixture is only proof if each view holds the photo to begin with.
        for (v, arg) in views(&ids, album) {
            assert!(
                view(&lib, v, &arg).contains(&ids[0]),
                "{v:?} did not hold the photo before it was hidden"
            );
        }

        assert_eq!(lib.set_hidden(&[ids[0]], true).unwrap(), 1);
        for (v, arg) in views(&ids, album) {
            assert!(
                !view(&lib, v, &arg).contains(&ids[0]),
                "{v:?} still shows a hidden photo"
            );
        }
    }

    #[test]
    fn the_hidden_view_holds_only_hidden_photos() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
                new_item(folder, "/p/c.jpg", 3),
            ])
            .unwrap();
        assert!(view(&lib, GridView::Hidden, "").is_empty());

        lib.set_hidden(&[ids[0], ids[2]], true).unwrap();
        assert_eq!(view(&lib, GridView::Hidden, ""), vec![ids[0], ids[2]]);
        assert_eq!(view(&lib, GridView::All, ""), vec![ids[1]]);
        assert_eq!(lib.hidden_count().unwrap(), 2);

        lib.set_hidden(&[ids[0]], false).unwrap();
        assert_eq!(view(&lib, GridView::Hidden, ""), vec![ids[2]]);
        assert_eq!(view(&lib, GridView::All, ""), vec![ids[0], ids[1]]);
        assert_eq!(lib.hidden_count().unwrap(), 1);
    }

    /// The count is of real changes, so the engine can skip a rebuild for a no-op; and a
    /// missing photo is left alone.
    #[test]
    fn only_real_changes_are_counted() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        lib.mark_missing(&[ids[1]], 5).unwrap();

        assert_eq!(lib.set_hidden(&[ids[0], ids[1], 999], true).unwrap(), 1);
        assert_eq!(
            lib.set_hidden(&[ids[0]], true).unwrap(),
            0,
            "already hidden"
        );
        assert_eq!(lib.hidden_count().unwrap(), 1, "a missing photo was hidden");
        assert_eq!(lib.set_hidden(&[ids[0]], false).unwrap(), 1);
        assert_eq!(
            lib.set_hidden(&[ids[0]], false).unwrap(),
            0,
            "already shown"
        );
    }

    /// The duplicate cleanup this exists for: hiding one of two copies leaves its twin with
    /// no visible copy, so both leave Duplicates at once - for byte-identical twins and for
    /// look-alikes alike.
    #[test]
    fn hiding_a_copy_takes_its_twin_out_of_duplicates() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/a-copy.jpg", 2),
                new_item(folder, "/p/b.jpg", 3),
                new_item(folder, "/p/b-small.jpg", 4),
            ])
            .unwrap();
        lib.writer()
            .execute(
                "UPDATE items SET content_hash = x'0102' WHERE id IN (?1, ?2)",
                [ids[0], ids[1]],
            )
            .unwrap();
        lib.set_similar_groups(&[(ids[2], ids[2]), (ids[3], ids[2])])
            .unwrap();
        assert_eq!(view(&lib, GridView::Duplicates, ""), ids);
        assert_eq!(lib.duplicate_count().unwrap(), 4);
        assert_eq!(lib.copies_of(ids[0]).unwrap().len(), 1);
        assert_eq!(lib.similar_of(ids[2]).unwrap().len(), 1);

        lib.set_hidden(&[ids[1], ids[3]], true).unwrap();
        assert!(
            view(&lib, GridView::Duplicates, "").is_empty(),
            "a photo whose only copy is hidden is still a duplicate"
        );
        assert_eq!(lib.duplicate_count().unwrap(), 0);
        assert!(
            lib.entries_for(GridView::All, "")
                .unwrap()
                .iter()
                .all(|e| !e.has_copies),
            "a tile still carries the copy mark for a hidden copy"
        );
        assert!(lib.copies_of(ids[0]).unwrap().is_empty());
        assert!(lib.similar_of(ids[2]).unwrap().is_empty());
    }

    /// Three copies, one hidden: the other two are still each other's duplicates, and the
    /// hidden one is not counted among them - it has left every view but Hidden.
    #[test]
    fn a_hidden_copy_is_not_counted_while_its_twins_remain() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
                new_item(folder, "/p/c.jpg", 3),
                new_item(folder, "/p/x.jpg", 4),
                new_item(folder, "/p/y.jpg", 5),
                new_item(folder, "/p/z.jpg", 6),
            ])
            .unwrap();
        lib.writer()
            .execute(
                "UPDATE items SET content_hash = x'0102' WHERE id IN (?1, ?2, ?3)",
                [ids[0], ids[1], ids[2]],
            )
            .unwrap();
        lib.set_similar_groups(&[(ids[3], ids[3]), (ids[4], ids[3]), (ids[5], ids[3])])
            .unwrap();
        lib.set_hidden(&[ids[0], ids[3]], true).unwrap();
        assert_eq!(lib.duplicate_count().unwrap(), 4);
        assert!(
            lib.entries_for(GridView::Hidden, "")
                .unwrap()
                .iter()
                .all(|e| !e.has_copies),
            "a hidden photo is still counted among the duplicates"
        );
    }

    #[test]
    fn hidden_photos_are_not_counted() {
        let (_dir, lib) = temp_library();
        let (ids, album) = everywhere(&lib);
        lib.set_hidden(&[ids[0]], true).unwrap();

        assert_eq!(lib.starred_count().unwrap(), 1);
        assert_eq!(lib.people_with_counts().unwrap()[0].count, 1);
        let albums = lib.albums_with_counts().unwrap();
        assert_eq!(albums.iter().find(|a| a.id == album).unwrap().count, 1);
        assert_eq!(lib.tags_with_counts().unwrap()[0].count, 1);
        // The twin's only copy is hidden, so it is no longer a duplicate either.
        assert_eq!(lib.duplicate_count().unwrap(), 0);
    }

    /// A folder is placed in the Hidden view by its oldest *hidden* photo, as Starred places
    /// one by its oldest starred photo, so the sidebar's year groups agree with the grid.
    #[test]
    fn the_hidden_view_places_a_folder_by_its_oldest_hidden_photo() {
        let (_dir, lib) = temp_library();
        let (w, old) = seed_folder(&lib, Path::new("/p"));
        let new = lib.upsert_folder(w, Some(old), "/p/new", 2).unwrap();
        let ids = lib
            .insert_items(&[
                // An old folder whose only hidden photo is recent.
                new_item(old, "/p/ancient.jpg", 1),
                new_item(old, "/p/late.jpg", 300),
                // A newer folder whose hidden photo is older than that.
                new_item(new, "/p/new/mid.jpg", 200),
            ])
            .unwrap();
        lib.set_hidden(&[ids[1], ids[2]], true).unwrap();
        assert_eq!(
            view(&lib, GridView::Hidden, ""),
            vec![ids[1], ids[2]],
            "a folder was placed by a photo the Hidden view does not show"
        );
    }

    /// The flag is on the row, like a star, and a file rewritten in place keeps its row.
    #[test]
    fn a_rewritten_file_stays_hidden() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        lib.set_hidden(&ids, true).unwrap();
        let rewritten = NewItem {
            size: 999,
            mtime_ms: 999,
            ..new_item(folder, "/p/a.jpg", 1)
        };
        lib.update_items(&[(ids[0], rewritten)]).unwrap();
        assert_eq!(view(&lib, GridView::Hidden, ""), ids);
    }

    #[test]
    fn the_hidden_view_and_count_are_served_by_their_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        for sql in [
            HIDDEN_COUNT_SQL.to_string(),
            grid_query(GRID_COLUMNS, Shown::Hidden, ""),
        ] {
            let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let plan: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert!(
                plan.iter().any(|step| step.contains("items_hidden")),
                "expected items_hidden in {plan:?}"
            );
        }
    }
}
