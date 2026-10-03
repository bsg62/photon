use super::Library;
use super::duplicates::{COPIES_FILTER, CopiesArg, DUPLICATE_FILTER, duplicate_ids};
use super::tags::{EFFECTIVE_TAGS, TAG_FILTER};
use crate::Result;
use crate::edit::{Crop, Edit};
use crate::grid::{GridEntry, GridView};
use crate::media::{MediaKind, ThumbState, fingerprint};
use crate::metadata::{CameraMeta, EXIF_VERSION, Gps, oriented_dims, write_date_text};
use crate::search::{Haystacks, Query, fold_into};
use crate::sort::{Sort, SortKey};
use rusqlite::{Connection, OptionalExtension, Row, ToSql, params};
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::fmt::Write;

/// A file discovered by the scanner, ready to be inserted or to replace an existing row.
#[derive(Clone, Debug, PartialEq)]
pub struct NewItem {
    pub folder_id: i64,
    pub path: String,
    pub file_name: String,
    pub kind: MediaKind,
    pub size: i64,
    pub mtime_ms: i64,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
    /// `None` when the file has not been read for a rating yet.
    pub rating: Option<u8>,
    pub camera: CameraMeta,
    /// Keywords read from the file's own XMP and IPTC.
    pub tags: Vec<String>,
    /// The caption the file carries (`keywords::read_embedded`), `None` for none.
    pub caption: Option<String>,
    /// A video's running time; `None` for a photo, or a video whose container did not say.
    pub duration_ms: Option<i64>,
}

/// What the scanner needs to know about an indexed file to detect changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownItem {
    pub id: i64,
    pub size: i64,
    pub mtime_ms: i64,
    pub missing: bool,
    /// The generation of `metadata::read_image_meta` that last read this file; behind
    /// `EXIF_VERSION`, the scanner re-reads it even though the file is unchanged.
    pub exif_version: i64,
}

/// One live photo in a folder, as the scanner's Picasa pass compares it with the INI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderItem {
    pub id: i64,
    /// The file name, lowercased the way the INI's names are.
    pub name: String,
    pub rating: Option<i64>,
    /// What the INI said about hiding it when the pass last read it; `None` until then.
    pub picasa_hidden: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: i64,
    pub folder_id: i64,
    pub path: String,
    pub kind: MediaKind,
    pub size: i64,
    pub mtime_ms: i64,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
    pub thumb_state: ThumbState,
    pub thumb_error: Option<String>,
    pub missing_since: Option<i64>,
    /// `None` until the Picasa pass has read the folder; see `is_starred`.
    pub rating: Option<i64>,
    pub camera: CameraMeta,
    /// What the user has done to the photo in photon; `Edit::default()` for nearly all.
    pub edit: Edit,
    /// Whether the user has hidden the photo (`library/hidden.rs`).
    pub hidden: bool,
    /// A video's running time; `None` for a photo, or a video whose container did not say.
    pub duration_ms: Option<i64>,
}

impl Item {
    /// The key this photo's thumbnails are cached under: the file's fingerprint, mixed with
    /// the edit when there is one. Everything that names a thumbnail - the cache, the grid,
    /// the viewer, the garbage collector - must go through this or `edit_from_db` +
    /// `Edit::thumb_key`; a bare `fingerprint` names the *unedited* photo's thumbnail.
    pub fn thumb_key(&self) -> u64 {
        self.edit
            .thumb_key(fingerprint(&self.path, self.size, self.mtime_ms))
    }
}

/// The edit held in a row's `edit_turns` and `edit_crop`. A row that does not make a valid
/// edit reads as untouched rather than failing the query it is part of: the photo then shows
/// as it is on disk, which is never wrong.
pub(super) fn edit_from_db(turns: i64, crop: Option<i64>) -> Edit {
    Edit::new(turns.rem_euclid(4) as u8, crop.map(Crop::from_db)).unwrap_or_default()
}

/// Grid order, shared by every query that walks items the way the grid shows them.
///
/// Folders run newest first by the date of their **oldest** photo, then each folder's photos
/// run oldest to newest. That is the same axis the sidebar groups by, so the list beside the
/// grid is an index of it rather than a second, unrelated ordering — before this the grid ran
/// alphabetically by path while the sidebar ran by year, and scrolling one bore no relation
/// to reading the other.
///
/// `o.oldest` and `o.fpath` come from [`folder_order`], the per-folder driver every grid
/// query is built on. [`grid_query`] pairs the two structurally so a caller cannot take one
/// without the other; `pending_thumb_ids` is the one hand-assembled query and says why.
///
/// In the Starred view the driver is given the same filter as the outer `WHERE`, so a folder
/// is placed by its oldest *starred* photo: the sidebar's sections come from that same
/// filtered index, so the two agree. Search filters in Rust after the query and so places a
/// folder by its oldest photo overall, exactly as it always has.
///
/// `fpath` breaks ties — two folders whose oldest photos share a timestamp would otherwise
/// interleave, the same hazard `sort_key` collisions used to pose.
///
/// Two earlier shapes are recorded here so nobody goes back to them. A window function,
/// `MIN(i.taken_at) OVER (PARTITION BY i.folder_id)`, made SQLite materialise and sort the
/// whole row set twice: ~88ms for `startup_grid_100k`. A `GROUP BY` join sorted it once:
/// ~60ms. Driving from the ordered *folder* list instead sorts ~1,000 folders and then walks
/// each folder's rows through `items_folder`, sorting only within a folder: ~47ms. The row
/// order is byte-identical across all three, verified on the 100k bench library.
pub(crate) const GRID_ORDER: &str = "ORDER BY o.oldest DESC, o.fpath, i.taken_at, i.file_name";

/// Which photos a grid query may return, by the user's Hide.
///
/// An argument of its own rather than a clause each view folds into its filter, so that
/// every caller of [`grid_query`] has to *say* which set it wants: a new view that forgot
/// would show the photos the user put away, which is the one thing a hide must never do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shown {
    /// Every view but Hidden.
    Visible,
    /// The Hidden view.
    Hidden,
    /// Bookkeeping that serves both - the thumbnail queue, whose Hidden view needs
    /// thumbnails too.
    Either,
}

impl Shown {
    fn sql(self) -> &'static str {
        match self {
            Self::Visible => "AND i.hidden = 0",
            Self::Hidden => "AND i.hidden = 1",
            Self::Either => "",
        }
    }
}

/// The grid's driver: every folder with a matching live photo, placed by its oldest one and
/// its path, already in grid order. Aliased `o` for [`GRID_ORDER`].
///
/// `filter` is the same `AND …` fragment on `i` the caller's outer `WHERE` uses, so the
/// minimum is taken over the rows the view shows rather than the whole folder; it may name
/// only `items` columns, since inside this subquery `i` is the subquery's own alias. `shown`
/// is part of that same filter, for the same reason: a folder is placed by its oldest photo
/// the view actually shows.
fn folder_order(shown: Shown, filter: &str) -> String {
    let shown = shown.sql();
    format!(
        "(SELECT i.folder_id, MIN(i.taken_at) AS oldest, f.path AS fpath
          FROM items i JOIN folders f ON f.id = i.folder_id
          WHERE i.missing_since IS NULL {shown} {filter}
          GROUP BY i.folder_id
          ORDER BY oldest DESC, fpath) o"
    )
}

/// A whole grid query: `select` over the live items matching `filter`, in grid order, with
/// `f` (the item's folder) joined for callers that read a folder column. The one place the
/// driver's filter and the outer filter are spelled, so they cannot drift apart - a
/// driver placed by one set of rows and a result holding another is how Starred would
/// silently sort by the wrong photo.
pub(super) fn grid_query(select: &str, shown: Shown, filter: &str) -> String {
    let driver = folder_order(shown, filter);
    let shown = shown.sql();
    format!(
        "SELECT {select}
         FROM {driver}
         JOIN items i ON i.folder_id = o.folder_id
         JOIN folders f ON f.id = i.folder_id
         WHERE i.missing_since IS NULL {shown} {filter}
         {GRID_ORDER}"
    )
}

/// The Recent view's query. Shared with the test that checks its plan, so the `ORDER BY`
/// the `items_recent` index was built for cannot drift from the one actually run.
fn recent_sql() -> String {
    format!(
        "SELECT {GRID_COLUMNS}
         FROM items i
         WHERE i.missing_since IS NULL AND i.hidden = 0
         ORDER BY i.taken_at DESC, i.file_name DESC, i.id DESC
         LIMIT {RECENT_LIMIT}"
    )
}

/// How many photos the Recent view shows. Picasa's equivalent list was a fixed-size window
/// onto the newest photos rather than a filter, so there is nothing to derive this from: it
/// is a chosen number, large enough to cover a few trips' worth of photos and small enough
/// that the view stays a shortlist rather than a second All view.
pub const RECENT_LIMIT: usize = 500;

/// The grid's columns, in the order `map_grid_row` reads them. Both query paths select
/// this same prefix so one mapping serves both.
///
/// `search_entries` appends more columns after this prefix and reads them by index
/// starting at `GRID_COLUMN_COUNT`: adding a column here shifts those indices, so keep
/// the two in sync.
///
/// The last column is `has_copies`: membership of the same set the Duplicates view
/// filters on (`duplicate_ids!`). The subquery does not mention `i`, so SQLite builds the
/// set once per query and probes it per row, rather than re-running it for each photo.
/// Measured on `startup_grid_100k` (2026-09-22): 49ms -> 55ms with no hashes at all, and
/// 70ms (`..._with_duplicates`) with one photo in ten a byte-identical pair, against the
/// one-second startup budget.
pub(super) const GRID_COLUMNS: &str = concat!(
    "i.id, i.folder_id, i.taken_at, i.width, i.height, i.orientation, i.kind, i.path, \
     i.size, i.mtime_ms, i.rating, i.edit_turns, i.edit_crop, i.id IN (",
    duplicate_ids!(),
    ")",
    ", i.duration_ms"
);

/// The Starred view's filter, shared with its plan test.
const STARRED_FILTER: &str = "AND i.rating >= 1";

/// The Videos view's filter, shared with `video_count` so the row's number and the grid it
/// opens cannot count different things.
const VIDEO_FILTER: &str = "AND i.kind = 1";

/// `video_count`'s query, shared with its plan test. The bare `missing_since IS NULL` is
/// what lets it read `items_videos`, which holds only the videos: a `+` there, which once
/// kept it off `items_size` (`library/mod.rs`), would now keep it off its own index too and
/// send it back to a scan of every row.
fn video_count_sql() -> String {
    format!(
        "SELECT COUNT(*) FROM items i WHERE i.missing_since IS NULL AND i.hidden = 0 {VIDEO_FILTER}"
    )
}

/// `starred_count`'s query, shared with its plan test.
const STARRED_COUNT_SQL: &str =
    "SELECT COUNT(*) FROM items WHERE rating >= 1 AND missing_since IS NULL AND hidden = 0";

/// `file_names`' query, shared with its plan test. The `+` keeps it a scan in table order
/// rather than a walk of `items_size`; see `library/mod.rs`.
const FILE_NAMES_SQL: &str = "SELECT id, file_name FROM items WHERE +missing_since IS NULL";

/// Number of columns selected by `GRID_COLUMNS`. `search_entries` uses this rather than a
/// bare `15` so a future column added to `GRID_COLUMNS` can't silently shift `file_name`
/// and `folder name` into the wrong indices without also touching this constant.
const GRID_COLUMN_COUNT: usize = 15;

pub(super) fn map_grid_row(r: &Row<'_>) -> rusqlite::Result<GridEntry> {
    let edit = edit_from_db(r.get(11)?, r.get(12)?);
    let (w, h) = oriented_dims(r.get(3)?, r.get(4)?, r.get(5)?);
    let (w, h) = edit.dims(w, h);
    let (size, mtime_ms) = (r.get(8)?, r.get(9)?);
    Ok(GridEntry {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        taken_at: r.get(2)?,
        aspect: if w == 0 || h == 0 {
            1.0
        } else {
            w as f32 / h as f32
        },
        kind: MediaKind::from_db(r.get(6)?).unwrap_or(MediaKind::Image),
        starred: is_starred(r.get(10)?),
        has_copies: r.get(13)?,
        thumb_key: edit.thumb_key(fingerprint(&r.get::<_, String>(7)?, size, mtime_ms)),
        duration_ms: r.get(14)?,
        size,
        mtime_ms,
    })
}

/// A text column borrowed from the row rather than copied into a `String`: search reads
/// several per photo over the whole library, and needs each only long enough to fold it.
fn text<'r>(r: &'r Row<'_>, idx: usize) -> rusqlite::Result<Option<&'r str>> {
    let value = r.get_ref(idx)?;
    value
        .as_str_or_null()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, value.data_type(), Box::new(e)))
}

/// Every photo's keywords as search matches them: the rule-applied ones (`EFFECTIVE_TAGS`),
/// lowercased, joined by a space, by item id.
///
/// One pass over the keywords, where search used to run a correlated subquery per photo:
/// that was about 250ms of a 510ms search at 300k photos, while this is one scan and a
/// hash probe per photo. Joined by a space as `group_concat(tag, ' ')` joined them, so a
/// quoted phrase can still span two keywords, in the order the query yields them - the
/// order neither form ever specified, since `group_concat` has none. Each keyword is folded on its own and the results joined, which
/// equals folding the joined string: a space is neither a letter nor ignorable to the one
/// context-sensitive rule `to_lowercase` has (the final sigma), so neither side of it
/// changes how the other lowercases.
///
/// Every photo's keywords, hidden and missing ones too: the rows that need none cost one
/// entry each, and filtering here would repeat the grid query's filter in a second place.
fn search_tags(conn: &Connection) -> Result<HashMap<i64, String>> {
    search_names(
        conn,
        &format!("SELECT e.item_id, e.tag FROM ({EFFECTIVE_TAGS}) e"),
    )
}

/// The names of the people on each photo, for `person:`, as `search_tags` gives the
/// keywords: Picasa's contacts, and photon's own people by their confirmed faces. A face
/// whose contact no INI has named has no name to find, and a suggestion is not yet the
/// person's face.
const SEARCH_PEOPLE_SQL: &str =
    "SELECT f.item_id, c.name FROM faces f JOIN contacts c ON c.hash = f.contact
     UNION ALL
     SELECT d.item_id, p.name FROM detected_faces d JOIN people p ON p.id = d.person_id
     WHERE d.confirmed = 1 AND p.name IS NOT NULL
     UNION ALL
     SELECT f.item_id, p.name FROM faces f
     JOIN person_contacts pc ON pc.contact = f.contact
     JOIN people p ON p.id = pc.person_id WHERE p.name IS NOT NULL";

/// The names of the albums each photo is in, photon's and Picasa's, for `album:`.
const SEARCH_ALBUMS_SQL: &str =
    "SELECT m.item_id, a.name FROM album_items m JOIN albums a ON a.id = m.album_id";

/// The `(item id, name)` rows of `sql` as one lowercased string per photo, its names joined
/// by a space.
fn search_names(conn: &Connection, sql: &str) -> Result<HashMap<i64, String>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    let mut tags: HashMap<i64, String> = HashMap::new();
    while let Some(r) = rows.next()? {
        // Every name column is NOT NULL; skipped all the same, as `group_concat` skips one.
        let Some(tag) = text(r, 1)? else {
            continue;
        };
        match tags.entry(r.get(0)?) {
            Entry::Occupied(mut joined) => {
                let joined = joined.get_mut();
                joined.push(' ');
                fold_into(joined, tag);
            }
            Entry::Vacant(slot) => {
                let mut joined = String::new();
                fold_into(&mut joined, tag);
                slot.insert(joined);
            }
        }
    }
    Ok(tags)
}

/// Shared by `known_items` and `known_items_under`, whose two queries select the same six
/// columns in the same order and differ only in how they scope the rows.
fn row_to_known(r: &Row<'_>) -> rusqlite::Result<(String, KnownItem)> {
    Ok((
        r.get::<_, String>(0)?,
        KnownItem {
            id: r.get(1)?,
            size: r.get(2)?,
            mtime_ms: r.get(3)?,
            missing: r.get(4)?,
            exif_version: r.get(5)?,
        },
    ))
}

/// The camera columns, in the order `camera_from_row` reads them from `base` onwards.
const CAMERA_COLUMNS: &str =
    "make, model, lens, focal_mm, aperture, exposure_s, iso, gps_lat, gps_lon";

/// A position from its two columns, which are both set or both NULL.
fn gps_from_db(lat: Option<f64>, lon: Option<f64>) -> Option<Gps> {
    Some(Gps {
        lat: lat?,
        lon: lon?,
    })
}

fn camera_from_row(r: &Row<'_>, base: usize) -> rusqlite::Result<CameraMeta> {
    Ok(CameraMeta {
        make: r.get(base)?,
        model: r.get(base + 1)?,
        lens: r.get(base + 2)?,
        focal_mm: r.get(base + 3)?,
        aperture: r.get(base + 4)?,
        exposure_s: r.get(base + 5)?,
        iso: r.get(base + 6)?,
        gps: gps_from_db(r.get(base + 7)?, r.get(base + 8)?),
    })
}

fn row_to_item(r: &Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        path: r.get(2)?,
        kind: MediaKind::from_db(r.get(3)?).unwrap_or(MediaKind::Image),
        size: r.get(4)?,
        mtime_ms: r.get(5)?,
        width: r.get(6)?,
        height: r.get(7)?,
        orientation: r.get(8)?,
        taken_at: r.get(9)?,
        thumb_state: ThumbState::from_db(r.get(10)?),
        thumb_error: r.get(11)?,
        missing_since: r.get(12)?,
        rating: r.get(13)?,
        camera: camera_from_row(r, 14)?,
        edit: edit_from_db(r.get(23)?, r.get(24)?),
        hidden: r.get(25)?,
        duration_ms: r.get(26)?,
    })
}

/// What a `rating` means as a star. `NULL` is "not read yet" and `0` is "read, unstarred";
/// Picasa's single star is `1`, and the column keeps its 0-5 range for a future source with
/// real ratings. The grid and the viewer both go through here so they cannot disagree.
pub fn is_starred(rating: Option<i64>) -> bool {
    rating.unwrap_or(0) >= 1
}

/// Replaces one item's keywords inside the caller's transaction. Every writer of an item
/// row goes through here so the table cannot fall behind the columns. The rows are the
/// file's keywords verbatim; the user's renames and removals are applied on read
/// (`library/tags.rs`).
fn write_tags(
    tx: &rusqlite::Transaction<'_>,
    item_id: i64,
    tags: &[String],
) -> rusqlite::Result<()> {
    tx.prepare_cached("DELETE FROM item_tags WHERE item_id = ?1")?
        .execute(params![item_id])?;
    let mut insert =
        tx.prepare_cached("INSERT OR IGNORE INTO item_tags (item_id, tag) VALUES (?1, ?2)")?;
    for tag in tags {
        insert.execute(params![item_id, tag])?;
    }
    Ok(())
}

impl Library {
    pub fn insert_items(&self, items: &[NewItem]) -> Result<Vec<i64>> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let mut ids = Vec::with_capacity(items.len());
        {
            let mut stmt = tx.prepare_cached(
                // `hidden` comes from the folder: a photo added to a folder the user hid -
                // a new file, or one renamed or moved in, which is a new row - arrives hidden
                // (`library/hidden.rs`, Hide folder).
                "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at, rating,
                                    make, model, lens, focal_mm, aperture, exposure_s, iso, exif_version, caption, duration_ms,
                                    gps_lat, gps_lon, hidden)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21,
                         ?22, ?23,
                         coalesce((SELECT hidden FROM folders WHERE id = ?1), 0))",
            )?;
            for it in items {
                let c = &it.camera;
                stmt.execute(params![
                    it.folder_id,
                    it.path,
                    it.file_name,
                    it.kind.to_db(),
                    it.size,
                    it.mtime_ms,
                    it.width,
                    it.height,
                    it.orientation,
                    it.taken_at,
                    it.rating,
                    c.make,
                    c.model,
                    c.lens,
                    c.focal_mm,
                    c.aperture,
                    c.exposure_s,
                    c.iso,
                    EXIF_VERSION,
                    it.caption,
                    it.duration_ms,
                    c.gps.map(|g| g.lat),
                    c.gps.map(|g| g.lon),
                ])?;
                let id = tx.last_insert_rowid();
                write_tags(&tx, id, &it.tags)?;
                ids.push(id);
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Replaces changed (or reappeared) items. Their thumbnails must be rebuilt.
    /// So must their content hash: it described bytes the file no longer holds.
    ///
    /// Deliberately does not touch `rating`: a star is not a property of the file (see
    /// `set_ratings`), and `NewItem.rating` is always `None` for a scanned file. Writing it
    /// here would `NULL` out a folder's existing stars on the next size/mtime change, and a
    /// folder the Picasa pass could not read that scan would have no way to restore it.
    pub fn update_items(&self, items: &[(i64, NewItem)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        // The row's fingerprint changes with its size or mtime, so the thumbnails written
        // under the old one are orphaned by this write.
        super::settings::bump_thumb_gc_epoch(&tx)?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE items SET folder_id = ?2, path = ?3, file_name = ?4, kind = ?5, size = ?6, mtime_ms = ?7,
                        width = ?8, height = ?9, orientation = ?10, taken_at = ?11,
                        make = ?12, model = ?13, lens = ?14, focal_mm = ?15, aperture = ?16, exposure_s = ?17, iso = ?18,
                        exif_version = ?19, caption = ?20, duration_ms = ?21,
                        gps_lat = ?22, gps_lon = ?23,
                        thumb_state = 0, thumb_error = NULL, missing_since = NULL,
                        content_hash = NULL, percep_hash = NULL, similar_group = NULL,
                        face_version = NULL
                 WHERE id = ?1",
            )?;
            let mut clear_faces =
                tx.prepare_cached("DELETE FROM detected_faces WHERE item_id = ?1")?;
            for (id, it) in items {
                let c = &it.camera;
                stmt.execute(params![
                    id,
                    it.folder_id,
                    it.path,
                    it.file_name,
                    it.kind.to_db(),
                    it.size,
                    it.mtime_ms,
                    it.width,
                    it.height,
                    it.orientation,
                    it.taken_at,
                    c.make,
                    c.model,
                    c.lens,
                    c.focal_mm,
                    c.aperture,
                    c.exposure_s,
                    c.iso,
                    EXIF_VERSION,
                    it.caption,
                    it.duration_ms,
                    c.gps.map(|g| g.lat),
                    c.gps.map(|g| g.lon),
                ])?;
                // The detections were of the old file's picture, as the hashes cleared above.
                clear_faces.execute(params![id])?;
                write_tags(&tx, *id, &it.tags)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Rewrites only what `metadata::read_image_meta` and `keywords::read_embedded`
    /// produce (the camera columns, the keywords, the caption and `exif_version`) for files
    /// the scanner found unchanged but whose stored metadata predates the current reader.
    ///
    /// Deliberately not `update_items`: that resets the thumbnail and bumps the garbage
    /// epoch because the file's fingerprint changed, and here it has not. Nor does this touch
    /// `rating`, which the Picasa pass owns.
    ///
    /// `taken_at` is written, because readers before `EXIF_VERSION` 2 believed any EXIF date
    /// and this is the only path that can re-date a photo whose file never changes again.
    /// For every photo whose date was sane the value written is the one already stored (the
    /// file is unchanged, so both the EXIF date and the mtime fallback are), and the photo
    /// does not move.
    pub fn update_item_meta(&self, items: &[(i64, NewItem)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE items SET make = ?2, model = ?3, lens = ?4, focal_mm = ?5, aperture = ?6,
                        exposure_s = ?7, iso = ?8, exif_version = ?9, taken_at = ?10, caption = ?11,
                        gps_lat = ?12, gps_lon = ?13
                 WHERE id = ?1",
            )?;
            for (id, it) in items {
                let c = &it.camera;
                stmt.execute(params![
                    id,
                    c.make,
                    c.model,
                    c.lens,
                    c.focal_mm,
                    c.aperture,
                    c.exposure_s,
                    c.iso,
                    EXIF_VERSION,
                    it.taken_at,
                    it.caption,
                    c.gps.map(|g| g.lat),
                    c.gps.map(|g| g.lon),
                ])?;
                write_tags(&tx, *id, &it.tags)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Test-only: makes a row look as it does after the metadata migration on a library
    /// indexed before it - camera columns empty, `exif_version` at the migration's default -
    /// so the scanner's backfill can be exercised on a file that has not changed.
    #[cfg(test)]
    pub(crate) fn forget_metadata_for_test(&self, id: i64) -> Result<()> {
        self.writer().execute(
            "UPDATE items SET make = NULL, model = NULL, lens = NULL, focal_mm = NULL,
                    aperture = NULL, exposure_s = NULL, iso = NULL, gps_lat = NULL, gps_lon = NULL,
                    exif_version = 0
             WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Test-only: makes a row look as it does in a library indexed by a reader that
    /// believed any EXIF date - the given `taken_at`, and a version behind the current one.
    #[cfg(test)]
    pub(crate) fn misdate_for_test(&self, id: i64, taken_at: i64) -> Result<()> {
        self.writer().execute(
            "UPDATE items SET taken_at = ?2, exif_version = 1 WHERE id = ?1",
            params![id, taken_at],
        )?;
        Ok(())
    }

    /// Test-only: makes a row look as it does after the caption column shipped, on a library
    /// indexed under `EXIF_VERSION` 2 - caption unread, version behind - so the scanner's
    /// backfill can be exercised on a file that has not changed.
    #[cfg(test)]
    pub(crate) fn forget_caption_for_test(&self, id: i64) -> Result<()> {
        self.writer().execute(
            "UPDATE items SET caption = NULL, exif_version = 2 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Test-only: makes a row look as it does after the position columns shipped, on a
    /// library indexed under `EXIF_VERSION` 3 - position unread, version behind.
    #[cfg(test)]
    pub(crate) fn forget_position_for_test(&self, id: i64) -> Result<()> {
        self.writer().execute(
            "UPDATE items SET gps_lat = NULL, gps_lon = NULL, exif_version = 3 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Every live item in one folder, with what the Picasa pass compares against its INI.
    ///
    /// The name is lowercased here because Picasa's INI may disagree in case with the files
    /// on disk, and this codebase folds case in Rust rather than in SQL: there is no `COLLATE
    /// NOCASE` on `file_name` and `lower()` is ASCII-only in SQLite without the ICU extension,
    /// which is a native dependency photon does not take. The current rating and the INI's
    /// last hidden answer are included so the pass can write only the rows that actually
    /// change, and read the folder's rows once for both.
    pub fn folder_item_names(&self, folder_id: i64) -> Result<Vec<FolderItem>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, file_name, rating, picasa_hidden
             FROM items WHERE folder_id = ?1 AND missing_since IS NULL",
        )?;
        let rows = stmt
            .query_map(params![folder_id], |r| {
                Ok(FolderItem {
                    id: r.get(0)?,
                    name: r.get::<_, String>(1)?.to_lowercase(),
                    rating: r.get(2)?,
                    picasa_hidden: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Sets the rating on specific items, leaving every other column alone.
    ///
    /// Deliberately not part of `update_items`: that rewrites a row from a rescanned file and
    /// resets its thumbnail, which is wrong for a star that changed while the photo did not.
    pub fn set_ratings(&self, ratings: &[(i64, u8)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE items SET rating = ?2 WHERE id = ?1")?;
            for (id, rating) in ratings {
                stmt.execute(params![id, rating])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Soft-deletes items; they stay hidden until a later scan purges or restores them.
    pub fn mark_missing(&self, ids: &[i64], now_ms: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE items SET missing_since = ?2 WHERE id = ?1 AND missing_since IS NULL",
            )?;
            for id in ids {
                stmt.execute(params![id, now_ms])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn purge_items(&self, ids: &[i64]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        super::settings::bump_thumb_gc_epoch(&tx)?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM items WHERE id = ?1")?;
            for id in ids {
                stmt.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every item under a watched folder, keyed by path, including soft-deleted ones.
    pub fn known_items(&self, watched_id: i64) -> Result<HashMap<String, KnownItem>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT i.path, i.id, i.size, i.mtime_ms, i.missing_since IS NOT NULL, i.exif_version
             FROM items i JOIN folders f ON f.id = i.folder_id WHERE f.watched_id = ?1",
        )?;
        let rows = stmt
            .query_map(params![watched_id], row_to_known)?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }

    /// Every item in `dir`'s folder and all folders beneath it, keyed by path, including
    /// soft-deleted ones. Membership comes from the folder tree rather than a path-prefix
    /// match, so `%` and `_` in a filename need no escaping and a directory with no folder
    /// row simply yields nothing.
    pub fn known_items_under(
        &self,
        watched_id: i64,
        dir: &str,
    ) -> Result<HashMap<String, KnownItem>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "WITH RECURSIVE sub(id) AS (
                 SELECT id FROM folders WHERE watched_id = ?1 AND path = ?2
                 UNION ALL
                 SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id
             )
             SELECT i.path, i.id, i.size, i.mtime_ms, i.missing_since IS NOT NULL, i.exif_version
             FROM items i WHERE i.folder_id IN (SELECT id FROM sub)",
        )?;
        let rows = stmt
            .query_map(params![watched_id, dir], row_to_known)?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }

    pub fn item(&self, id: i64) -> Result<Option<Item>> {
        let item = self
            .reader()?
            .query_row(
                &format!(
                    "SELECT id, folder_id, path, kind, size, mtime_ms, width, height, orientation, taken_at,
                            thumb_state, thumb_error, missing_since, rating, {CAMERA_COLUMNS},
                            edit_turns, edit_crop, hidden, duration_ms
                     FROM items WHERE id = ?1"
                ),
                params![id],
                row_to_item,
            )
            .optional()?;
        Ok(item)
    }

    /// The photo's caption, for the viewer. Not a field of `Item`: only the viewer reads it,
    /// and every other query that builds an `Item` would carry it for nothing.
    pub fn item_caption(&self, id: i64) -> Result<Option<String>> {
        Ok(self
            .reader()?
            .query_row(
                "SELECT caption FROM items WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Records the user's edit of one photo and sends its thumbnails back to be made.
    /// Returns `false`, writing nothing, when the photo already has exactly this edit or
    /// does not exist.
    ///
    /// The thumbnails cached under the old key are orphaned by this write, so it bumps the
    /// garbage epoch in the same transaction, like every other write that can orphan one.
    /// `update_items` leaves these columns alone: a photo re-saved by another program is
    /// still the photo the user turned.
    pub fn set_item_edit(&self, id: i64, edit: Edit) -> Result<bool> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        // The perceptual hash goes with the thumbnail it was taken from. An edit changes
        // the picture photon shows - and a look-alike is a fact about the photo as shown -
        // so a hash kept across one would describe whichever picture happened to be current
        // when the row was first hashed: crop then hash and it is the crop, hash then crop
        // and it is the uncropped frame. Clearing makes the row a candidate again, and the
        // next pass re-hashes it from the thumbnail this write just invalidated.
        let changed = tx.execute(
            "UPDATE items SET edit_turns = ?2, edit_crop = ?3, thumb_state = 0,
                 thumb_error = NULL, percep_hash = NULL, similar_group = NULL,
                 face_version = NULL
             WHERE id = ?1 AND NOT (edit_turns = ?2 AND edit_crop IS ?3)",
            params![id, edit.turns, edit.crop.map(Crop::to_db)],
        )?;
        if changed > 0 {
            // The detections describe the picture as shown, as the perceptual hash does:
            // a turn or a crop makes them rectangles on a picture photon no longer shows.
            tx.execute("DELETE FROM detected_faces WHERE item_id = ?1", params![id])?;
            super::settings::bump_thumb_gc_epoch(&tx)?;
        }
        tx.commit()?;
        Ok(changed > 0)
    }

    pub fn set_thumb_state(&self, id: i64, state: ThumbState, error: Option<&str>) -> Result<()> {
        self.writer().execute(
            "UPDATE items SET thumb_state = ?2, thumb_error = ?3 WHERE id = ?1",
            params![id, state.to_db(), error],
        )?;
        Ok(())
    }

    /// Like `set_thumb_state`, but only if the row still matches `item` (path/size/mtime
    /// and the edit). Returns `false` without writing if the item changed since it was read,
    /// so a worker processing a stale snapshot can't clobber a rescan's reset to `Pending` -
    /// or an edit's: a worker that rendered the photo as it was before the user turned it
    /// would otherwise mark it `Ready` with no thumbnail under the new key.
    pub fn set_thumb_state_if_unchanged(
        &self,
        item: &Item,
        state: ThumbState,
        error: Option<&str>,
    ) -> Result<bool> {
        let changed = self.writer().execute(
            "UPDATE items SET thumb_state = ?2, thumb_error = ?3
             WHERE id = ?1 AND path = ?4 AND size = ?5 AND mtime_ms = ?6
               AND edit_turns = ?7 AND edit_crop IS ?8",
            params![
                item.id,
                state.to_db(),
                error,
                item.path,
                item.size,
                item.mtime_ms,
                item.edit.turns,
                item.edit.crop.map(Crop::to_db),
            ],
        )?;
        Ok(changed > 0)
    }

    /// Items still waiting for thumbnails, in grid order. Items under an offline watched
    /// folder are skipped: their files can't be read until the folder comes back.
    ///
    /// "Grid order" means the All view's: folders placed by their oldest photo overall, so
    /// the queue works through folders in the order the grid shows them. The join is
    /// therefore unfiltered, unlike Starred's.
    ///
    /// Assembled by hand rather than through `grid_query`, because its outer filter reads
    /// `w.online`, which the driver cannot see. The driver is therefore unfiltered, and
    /// the planner walks from `items_pending` regardless, so the shape costs nothing.
    ///
    /// One `kind` at a time, because the two are made by different hands: a worker decodes
    /// an image, the webview draws a video's frame, and each drains its own queue.
    pub fn pending_thumb_ids(&self, kind: MediaKind) -> Result<Vec<i64>> {
        let conn = self.reader()?;
        let driver = folder_order(Shown::Either, "");
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id
             FROM {driver}
             JOIN items i ON i.folder_id = o.folder_id
             JOIN folders f ON f.id = i.folder_id
             JOIN watched_folders w ON w.id = f.watched_id
             WHERE i.thumb_state = 0 AND i.missing_since IS NULL AND w.online = 1
               AND i.kind = ?1 {GRID_ORDER}"
        ))?;
        let ids = stmt
            .query_map(params![kind.to_db()], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids)
    }

    /// Thumbnail keys of every indexed item; thumbnails for anything else are garbage. The
    /// key includes the edit, so the thumbnails of a photo as it looked before its current
    /// edit are garbage too, and the untouched photo's are again once the edit is reset.
    pub fn live_fingerprints(&self) -> Result<HashSet<u64>> {
        let conn = self.reader()?;
        let mut stmt =
            conn.prepare("SELECT path, size, mtime_ms, edit_turns, edit_crop FROM items")?;
        let set = stmt
            .query_map([], |r| {
                let file = fingerprint(&r.get::<_, String>(0)?, r.get(1)?, r.get(2)?);
                Ok(edit_from_db(r.get(3)?, r.get(4)?).thumb_key(file))
            })?
            .collect::<rusqlite::Result<HashSet<u64>>>()?;
        Ok(set)
    }

    /// Every visible item in grid order: newest folder first by its oldest photo, then each
    /// folder's photos oldest to newest.
    pub fn grid_entries(&self) -> Result<Vec<GridEntry>> {
        self.entries_for(GridView::All, "")
    }

    /// The grid's rows for one view, in `sort`'s order (`sort::Sort::arrange`): what the
    /// engine builds its index from. The file names a name sort compares are read by a
    /// query of their own rather than carried on every `GridEntry`, which is `Copy` because
    /// a whole library of them stays in memory.
    pub fn sorted_entries(&self, view: GridView, arg: &str, sort: Sort) -> Result<Vec<GridEntry>> {
        let mut entries = self.entries_for(view, arg)?;
        let names = match sort.key {
            SortKey::Name => self.file_names()?,
            _ => HashMap::new(),
        };
        sort.arrange(&mut entries, |id| names.get(&id).map_or("", String::as_str));
        Ok(entries)
    }

    /// Compiles, without running, the All view's grid query. What `Engine::open` checks the
    /// library with instead of building the first grid, which it leaves to the startup
    /// thread: a library whose schema that query cannot be prepared against - a column gone,
    /// a table renamed - still fails to open, with the error dialog, rather than opening
    /// onto a grid that never arrives. Preparing resolves every table and column and reads
    /// no rows, so it costs the same at 300k photos as at none.
    ///
    /// The name sort's side query (`FILE_NAMES_SQL`) is not prepared as well: every column
    /// it reads, `file_name` included (`GRID_ORDER`), this query reads too.
    pub fn check_grid_query(&self) -> Result<()> {
        self.reader()?
            .prepare(&grid_query(GRID_COLUMNS, Shown::Visible, ""))?;
        Ok(())
    }

    /// Every live item's file name, by id.
    fn file_names(&self) -> Result<HashMap<i64, String>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(FILE_NAMES_SQL)?;
        let names = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<HashMap<i64, String>>>()?;
        Ok(names)
    }

    /// The grid's rows for one view. `arg` is the view's argument - the query for `Search`,
    /// a person key for `Person`, an album id for `Album`, a keyword for `Tag` - and the
    /// other views ignore it. One entry point rather than one per view, because `GridView`
    /// is matched exhaustively and an arm that could not see the argument would have to lie.
    ///
    /// The three membership views filter with `IN (SELECT …)` on the driver as well as the
    /// outer `WHERE`, exactly as Starred does, so a folder is placed by its oldest *member*
    /// and the sidebar's year groups keep agreeing with the grid.
    pub fn entries_for(&self, view: GridView, arg: &str) -> Result<Vec<GridEntry>> {
        match view {
            GridView::All => self.entries_filtered("", &[]),
            GridView::Starred => self.entries_filtered(STARRED_FILTER, &[]),
            GridView::Videos => self.entries_filtered(VIDEO_FILTER, &[]),
            GridView::Hidden => self.hidden_entries(),
            GridView::Recent => self.recent_entries(),
            GridView::Search => self.search_entries(arg),
            GridView::Person => {
                // The argument is a key (`Person::key`). Neither prefix names no one, so a
                // bare contact hash is never read as one and the grid is empty, as for an
                // unknown album id.
                if let Some(person) = arg.strip_prefix("p:") {
                    let Ok(person) = person.parse::<i64>() else {
                        return Ok(Vec::new());
                    };
                    self.entries_filtered(
                        "AND i.id IN (SELECT item_id FROM detected_faces
                                      WHERE person_id = ?1 AND confirmed = 1
                                      UNION
                                      SELECT f.item_id FROM faces f
                                      JOIN person_contacts pc ON pc.contact = f.contact
                                      WHERE pc.person_id = ?1)",
                        &[&person],
                    )
                } else if let Some(contact) = arg.strip_prefix("c:") {
                    self.entries_filtered(
                        "AND i.id IN (SELECT item_id FROM faces WHERE contact = ?1)",
                        &[&contact],
                    )
                } else {
                    Ok(Vec::new())
                }
            }
            GridView::Album => {
                // An argument that is not an id names no album; an empty grid says so
                // rather than an error that would roll the view back to the previous one.
                let album_id: i64 = arg.parse().unwrap_or(-1);
                self.entries_filtered(
                    "AND i.id IN (SELECT item_id FROM album_items WHERE album_id = ?1)",
                    &[&album_id],
                )
            }
            GridView::Tag => self.entries_filtered(TAG_FILTER, &[&arg]),
            GridView::Duplicates => self.entries_filtered(DUPLICATE_FILTER, &[]),
            GridView::Copies => {
                // Like Album: an argument that is not an id names nothing, and an empty grid
                // says so rather than an error that would roll the view back.
                let Some(copies) = CopiesArg::parse(arg) else {
                    return Ok(Vec::new());
                };
                let hash = copies.hash.map(|h| h.to_vec());
                self.entries_filtered(COPIES_FILTER, &[&copies.anchor, &hash])
            }
        }
    }

    /// The grid's rows for a `WHERE` filter fragment, applied both to the rows returned and
    /// to the per-folder placement the order is built on (see `grid_query`). Starred and Videos
    /// are each served by a partial index on `(folder_id, taken_at)` holding exactly their
    /// rows (`items_starred`, `items_videos`, schema 22), which the driver and the per-folder
    /// walk both read; All reads `items_folder`, since an index of every visible row would
    /// tie with those two and take them over (CLAUDE.md, "Schema").
    ///
    /// `params` bind the filter's `?N` placeholders. The fragment appears twice in the
    /// query (driver and outer filter), which is why placeholders are numbered: the same
    /// value binds at both sites. Binding, rather than formatting the value into the SQL,
    /// is what keeps a keyword or contact hash from ever being read as SQL.
    fn entries_filtered(&self, filter: &str, params: &[&dyn ToSql]) -> Result<Vec<GridEntry>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(&grid_query(GRID_COLUMNS, Shown::Visible, filter))?;
        let rows = stmt
            .query_map(params, map_grid_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The newest `RECENT_LIMIT` photos, newest capture date first.
    ///
    /// The one view that does not use `GRID_ORDER`: ordering by folder would make "newest"
    /// mean "in the newest folder", and the whole point of this list is the individual
    /// photos.
    ///
    /// The consequence is that the rows are not folder runs: wherever folders overlap in time
    /// a folder reappears every few photos, which is why this view is laid out flat
    /// (`GridView::layout`) rather than one section per folder.
    ///
    /// `file_name` and `id` break ties so the cut at `RECENT_LIMIT` is deterministic:
    /// without them two photos sharing a capture time could swap across the boundary
    /// between rebuilds and the view would flicker for no reason.
    ///
    /// No join to `folders`: unlike `GRID_ORDER`, nothing here reads a folder column.
    fn recent_entries(&self) -> Result<Vec<GridEntry>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(&recent_sql())?;
        let rows = stmt
            .query_map([], map_grid_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Photos matching `query` in their file name, folder name or the folder's alias,
    /// camera, lens, keywords, caption or capture date, case-insensitively. The alias is a
    /// haystack beside the name rather than in place of it: the name is still on screen, in
    /// the grid header's path.
    ///
    /// Words narrow, `OR` widens, `camera:` and `lens:` confine a term to that field,
    /// `from:` and `to:` bound the capture date, and
    /// the matching runs in Rust rather than as SQL `LIKE`; `search::Query` holds the
    /// grammar and the reasons for it. This is one pass over the same rows an index rebuild
    /// already reads, with a handful of short string compares per term added per row.
    ///
    /// The numeric fields are spelled the way a person types them - `50mm`, `f/1.8`,
    /// `iso400` - and the date as `YYYY-MM-DD`, so "2024" and "2024-06" work without a folder
    /// named so. `search_finds_a_photo_by_its_camera_lens_keyword_and_date` pins each.
    ///
    /// Every debounced keystroke runs this over the whole library, so it is written to cost
    /// little per photo (measured on `search_100k`; the commit that made it so has the
    /// numbers). The text is borrowed from the row rather than copied out, and folded into
    /// one [`Haystacks`] reused from photo to photo; a folder's name and alias are folded
    /// once per folder rather than once per photo.
    fn search_entries(&self, query: &str) -> Result<Vec<GridEntry>> {
        let query = Query::parse(query);
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.reader()?;
        // One snapshot for the keywords and the rows, as the single statement this replaced
        // had: without it a tag written between the two reads could be matched against the
        // other read's photos.
        let tx = conn.unchecked_transaction()?;
        let tags = search_tags(&tx)?;
        // Read only for a query that names a person, an album or a face count, which most
        // do not.
        let needs = query.needs();
        let people = if needs.people {
            search_names(&tx, SEARCH_PEOPLE_SQL)?
        } else {
            HashMap::new()
        };
        let albums = if needs.albums {
            search_names(&tx, SEARCH_ALBUMS_SQL)?
        } else {
            HashMap::new()
        };
        let faces = if needs.faces {
            super::detected_faces::search_face_counts(&tx)?
        } else {
            HashMap::new()
        };
        let mut stmt = tx.prepare(&grid_query(
            &format!(
                "{GRID_COLUMNS}, i.file_name, f.name, i.make, i.model, i.lens, i.focal_mm, i.aperture, i.iso,
                 i.caption, f.alias, i.gps_lat, i.gps_lon"
            ),
            Shown::Visible,
            "",
        ))?;
        let base = GRID_COLUMN_COUNT;
        let mut haystacks = Haystacks::default();
        // The folder whose name and alias `haystacks` holds up to `folder_mark`. Rows arrive
        // in folder runs, so this refolds once per folder; it compares every row all the
        // same, so a folder that recurred would be refolded rather than misread.
        let mut folder: Option<i64> = None;
        let mut folder_mark = haystacks.mark();
        let mut scratch = String::new();
        let mut hits = Vec::new();
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let folder_id: i64 = r.get(1)?;
            if folder != Some(folder_id) {
                haystacks.clear();
                let name = text(r, base + 1)?.unwrap_or("");
                let alias = text(r, base + 9)?;
                haystacks.push(name);
                if let Some(alias) = alias {
                    haystacks.push(alias);
                }
                haystacks.set_folder(name, alias);
                folder = Some(folder_id);
                folder_mark = haystacks.mark();
            }
            haystacks.truncate(folder_mark);
            haystacks.push(text(r, base)?.unwrap_or(""));
            let make = text(r, base + 2)?;
            let model = text(r, base + 3)?;
            let lens = text(r, base + 4)?;
            for field in [make, model, lens].into_iter().flatten() {
                haystacks.push(field);
            }
            haystacks.set_camera(make, model);
            haystacks.set_lens(lens);
            // Digits, `mm`, `f/`, `iso` and a date are lowercase as written, so they go
            // straight into the buffer. The aperture is an `f64`, whose `Display` can spell
            // `NaN` with capitals; SQLite stores a NaN as NULL, but it is folded all the same
            // rather than trusted, since that costs next to nothing.
            if let Some(focal) = r.get::<_, Option<f64>>(base + 5)? {
                haystacks.push_with(|out| {
                    let _ = write!(out, "{}mm", focal.round() as i64);
                });
            }
            if let Some(aperture) = r.get::<_, Option<f64>>(base + 6)? {
                scratch.clear();
                let _ = write!(scratch, "f/{aperture}");
                haystacks.push(&scratch);
            }
            if let Some(iso) = r.get::<_, Option<i64>>(base + 7)? {
                haystacks.push_with(|out| {
                    let _ = write!(out, "iso{iso}");
                });
            }
            let id: i64 = r.get(0)?;
            let tags = tags.get(&id).map(String::as_str);
            if let Some(tags) = tags {
                haystacks.push_with(|out| out.push_str(tags));
            }
            haystacks.set_tags_folded(tags);
            haystacks.set_people_folded(people.get(&id).map(String::as_str));
            haystacks.set_albums_folded(albums.get(&id).map(String::as_str));
            haystacks.starred = r
                .get::<_, Option<i64>>(10)?
                .is_some_and(|rating| rating >= 1);
            haystacks.faces = faces.get(&id).copied().unwrap_or(0);
            haystacks.gps = gps_from_db(r.get(base + 10)?, r.get(base + 11)?);
            haystacks.edited = !edit_from_db(r.get(11)?, r.get(12)?).is_identity();
            if let Some(caption) = text(r, base + 8)? {
                haystacks.push_caption(caption);
            }
            let taken: i64 = r.get(2)?;
            haystacks.push_with(|out| write_date_text(out, taken));
            haystacks.taken = Some(taken);
            haystacks.kind = MediaKind::from_db(r.get(6)?);
            if query.matches_folded(&haystacks) {
                hits.push(map_grid_row(r)?);
            }
        }
        Ok(hits)
    }

    /// How many visible videos there are: the sidebar's Videos row, shown only above 0.
    ///
    /// Served by `items_videos`, which holds only the videos (schema 22). Before it, the count
    /// scanned every row, 18-25ms at 300k photos; and before its `+`, which that index
    /// made unnecessary, it walked `items_size` in random order, 232ms (`library/mod.rs`).
    pub fn video_count(&self) -> Result<usize> {
        let conn = self.reader()?;
        let count: i64 = conn.query_row(&video_count_sql(), [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// How many photos carry at least one star. Served by the `items_starred` partial index.
    ///
    /// `rating >= 1` also excludes unread rows without a second clause: a comparison
    /// against NULL is never true in SQL.
    pub fn starred_count(&self) -> Result<usize> {
        let conn = self.reader()?;
        let count: i64 = conn.query_row(STARRED_COUNT_SQL, [], |r| r.get(0))?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    #[test]
    fn insert_and_read_back_item() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 10)])
            .unwrap();

        let item = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(item.path, "/p/a.jpg");
        assert_eq!(item.folder_id, folder);
        assert_eq!(
            (item.width, item.height, item.orientation, item.taken_at),
            (400, 300, 1, 10)
        );
        assert_eq!(item.thumb_state, ThumbState::Pending);
        assert_eq!(item.missing_since, None);
        assert_eq!(item.thumb_key(), fingerprint("/p/a.jpg", 100, 1_000));
        assert!(lib.item(9_999).unwrap().is_none());
    }

    #[test]
    fn a_video_row_keeps_its_kind_and_duration() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/photos"));
        let mut video = new_item(folder, "/photos/clip.mp4", 100);
        video.kind = MediaKind::Video;
        video.duration_ms = Some(83_000);
        let id = lib.insert_items(&[video.clone()]).unwrap()[0];

        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.kind, MediaKind::Video);
        assert_eq!(item.duration_ms, Some(83_000));
        let row = lib
            .grid_entries()
            .unwrap()
            .into_iter()
            .find(|e| e.id == id)
            .unwrap();
        assert_eq!(row.kind, MediaKind::Video);
        assert_eq!(row.duration_ms, Some(83_000));

        // A rewrite carries the new running time; a photo's stays NULL.
        video.duration_ms = Some(90_000);
        lib.update_items(&[(id, video)]).unwrap();
        assert_eq!(lib.item(id).unwrap().unwrap().duration_ms, Some(90_000));
        let photo = lib
            .insert_items(&[new_item(folder, "/photos/a.jpg", 100)])
            .unwrap()[0];
        assert_eq!(lib.item(photo).unwrap().unwrap().duration_ms, None);
    }

    #[test]
    fn insert_items_stores_the_caption() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut captioned = new_item(folder, "/p/a.jpg", 1);
        captioned.caption = Some("Grandma's 80th".into());
        let ids = lib
            .insert_items(&[captioned, new_item(folder, "/p/b.jpg", 2)])
            .unwrap();
        assert_eq!(
            lib.item_caption(ids[0]).unwrap().as_deref(),
            Some("Grandma's 80th")
        );
        assert_eq!(lib.item_caption(ids[1]).unwrap(), None);
    }

    #[test]
    fn update_items_rewrites_the_caption_including_to_none() {
        // A file edited to remove its caption is a changed file; its row must lose it.
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut it = new_item(folder, "/p/a.jpg", 1);
        it.caption = Some("old".into());
        let id = lib.insert_items(&[it.clone()]).unwrap()[0];
        it.caption = Some("new".into());
        lib.update_items(&[(id, it.clone())]).unwrap();
        assert_eq!(lib.item_caption(id).unwrap().as_deref(), Some("new"));
        it.caption = None;
        lib.update_items(&[(id, it)]).unwrap();
        assert_eq!(lib.item_caption(id).unwrap(), None);
    }

    #[test]
    fn update_item_meta_writes_the_caption() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut it = new_item(folder, "/p/a.jpg", 1);
        let id = lib.insert_items(&[it.clone()]).unwrap()[0];
        it.caption = Some("backfilled".into());
        lib.update_item_meta(&[(id, it)]).unwrap();
        assert_eq!(lib.item_caption(id).unwrap().as_deref(), Some("backfilled"));
    }

    #[test]
    fn every_item_writer_stores_the_position() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let munich = Gps {
            lat: 48.137_4,
            lon: 11.575_5,
        };
        let sydney = Gps {
            lat: -33.868_8,
            lon: 151.209_3,
        };
        let gps = |id| lib.item(id).unwrap().unwrap().camera.gps;
        let mut it = new_item(folder, "/p/a.jpg", 1);
        it.camera.gps = Some(munich);
        let ids = lib
            .insert_items(&[it.clone(), new_item(folder, "/p/b.jpg", 2)])
            .unwrap();
        assert_eq!(gps(ids[0]), Some(munich));
        assert_eq!(gps(ids[1]), None);

        // The backfill writes it, on a row that had none.
        let mut placed = new_item(folder, "/p/b.jpg", 2);
        placed.camera.gps = Some(sydney);
        lib.update_item_meta(&[(ids[1], placed)]).unwrap();
        assert_eq!(gps(ids[1]), Some(sydney));

        // A changed file's position is whatever the file says now, nothing included.
        it.camera.gps = Some(sydney);
        lib.update_items(&[(ids[0], it.clone())]).unwrap();
        assert_eq!(gps(ids[0]), Some(sydney));
        it.camera.gps = None;
        lib.update_items(&[(ids[0], it)]).unwrap();
        assert_eq!(gps(ids[0]), None);
    }

    #[test]
    fn search_finds_photos_by_where_they_were_taken() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let at = |name: &str, taken: i64, gps: Option<Gps>| {
            let mut it = new_item(folder, name, taken);
            it.camera.gps = gps;
            it
        };
        let ids = lib
            .insert_items(&[
                at(
                    "/p/marienplatz.jpg",
                    1,
                    Some(Gps {
                        lat: 48.137_4,
                        lon: 11.575_5,
                    }),
                ),
                // About 9 km from Marienplatz.
                at(
                    "/p/nymphenburg.jpg",
                    2,
                    Some(Gps {
                        lat: 48.158_3,
                        lon: 11.503_3,
                    }),
                ),
                at("/p/nowhere.jpg", 3, None),
            ])
            .unwrap();
        let hits = |query: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, query)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(hits("has:gps"), vec![ids[0], ids[1]]);
        assert_eq!(hits("-has:gps"), vec![ids[2]]);
        assert_eq!(hits("near:48.1374,11.5755"), vec![ids[0]]);
        assert_eq!(hits("near:48.1374,11.5755,10km"), vec![ids[0], ids[1]]);
    }

    #[test]
    fn known_items_track_missing_update_and_purge() {
        let (_dir, lib) = temp_library();
        let (watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        let (a, b) = (ids[0], ids[1]);

        let known = lib.known_items(watched).unwrap();
        assert_eq!(known.len(), 2);
        assert_eq!(
            known["/p/a.jpg"],
            KnownItem {
                id: a,
                size: 100,
                mtime_ms: 1_000,
                missing: false,
                exif_version: EXIF_VERSION,
            }
        );

        lib.mark_missing(&[a], 50).unwrap();
        assert!(lib.known_items(watched).unwrap()["/p/a.jpg"].missing);
        assert_eq!(lib.item(a).unwrap().unwrap().missing_since, Some(50));

        lib.set_thumb_state(a, ThumbState::Ready, None).unwrap();
        let changed = NewItem {
            size: 200,
            ..new_item(folder, "/p/a.jpg", 1)
        };
        lib.update_items(&[(a, changed)]).unwrap();
        let item = lib.item(a).unwrap().unwrap();
        assert_eq!(
            (item.size, item.missing_since, item.thumb_state),
            (200, None, ThumbState::Pending)
        );

        lib.purge_items(&[b]).unwrap();
        assert!(lib.item(b).unwrap().is_none());
    }

    #[test]
    fn folder_item_names_lowercases_excludes_missing_rows_and_reports_the_current_answers() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/DSC_0001.JPG", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        let (a, b) = (ids[0], ids[1]);
        lib.mark_missing(&[b], 50).unwrap();
        lib.set_ratings(&[(a, 1)]).unwrap();
        lib.apply_picasa_hidden(&[(a, true, false)]).unwrap();

        let names = lib.folder_item_names(folder).unwrap();
        assert_eq!(
            names,
            vec![FolderItem {
                id: a,
                name: "dsc_0001.jpg".to_string(),
                rating: Some(1),
                picasa_hidden: Some(true),
            }]
        );
    }

    #[test]
    fn item_reports_its_rating() {
        // `viewer_item` and `set_star` read the star through `Library::item`, so the row
        // has to carry the column; a `SELECT` that leaves it out compiles and reads `None`
        // forever, which the viewer would show as never starred.
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        assert_eq!(lib.item(id).unwrap().unwrap().rating, None);
        lib.set_ratings(&[(id, 1)]).unwrap();
        assert_eq!(lib.item(id).unwrap().unwrap().rating, Some(1));
    }

    #[test]
    fn set_ratings_changes_only_the_rating() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let before = lib.item(id).unwrap().unwrap();

        lib.set_ratings(&[(id, 1)]).unwrap();

        let after = lib.item(id).unwrap().unwrap();
        assert_eq!(
            (
                after.path.clone(),
                after.size,
                after.mtime_ms,
                after.width,
                after.height,
                after.orientation,
                after.taken_at,
                after.thumb_state,
                after.missing_since
            ),
            (
                before.path,
                before.size,
                before.mtime_ms,
                before.width,
                before.height,
                before.orientation,
                before.taken_at,
                before.thumb_state,
                before.missing_since
            )
        );
        assert_eq!(lib.starred_count().unwrap(), 1);
    }

    #[test]
    fn thumb_state_records_errors() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        lib.set_thumb_state(id, ThumbState::Failed, Some("corrupt"))
            .unwrap();
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Failed);
        assert_eq!(item.thumb_error.as_deref(), Some("corrupt"));
    }

    fn crop_of(left: f64, top: f64, right: f64, bottom: f64) -> Crop {
        let u = |v: f64| (v * crate::edit::CROP_UNIT as f64).round() as u16;
        Crop {
            left: u(left),
            top: u(top),
            right: u(right),
            bottom: u(bottom),
        }
    }

    #[test]
    fn an_edit_renames_the_thumbnail_and_sends_it_back_to_be_made() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let wide = NewItem {
            width: 400,
            height: 200,
            ..new_item(folder, "/p/a.jpg", 1)
        };
        let id = lib.insert_items(&[wide]).unwrap()[0];
        lib.set_thumb_state(id, ThumbState::Ready, None).unwrap();
        let before = lib.item(id).unwrap().unwrap();
        assert_eq!(before.edit, Edit::default());
        assert_eq!(before.thumb_key(), fingerprint("/p/a.jpg", 100, 1_000));

        let edit = Edit::new(1, Some(crop_of(0.0, 0.0, 1.0, 0.5))).unwrap();
        assert!(lib.set_item_edit(id, edit).unwrap());

        let after = lib.item(id).unwrap().unwrap();
        assert_eq!(after.edit, edit, "the edit reads back as written");
        assert_ne!(after.thumb_key(), before.thumb_key());
        assert_eq!(
            after.thumb_state,
            ThumbState::Pending,
            "nothing is cached under the new key yet"
        );
        // The grid and the garbage collector derive the key on their own, from columns:
        // both must arrive at the item's.
        let entry = lib.grid_entries().unwrap()[0];
        assert_eq!(entry.thumb_key, after.thumb_key());
        // Turned, the 400x200 photo is 200x400; the top half of that is 200x200.
        assert_eq!(entry.aspect, 1.0);
        let live = lib.live_fingerprints().unwrap();
        assert!(live.contains(&after.thumb_key()));
        assert!(
            !live.contains(&before.thumb_key()),
            "the old look is garbage now"
        );

        assert!(
            !lib.set_item_edit(id, edit).unwrap(),
            "the same edit is no write"
        );
        assert!(
            !lib.set_item_edit(9_999, edit).unwrap(),
            "nor is an unknown photo"
        );
        assert!(lib.set_item_edit(id, Edit::default()).unwrap());
        assert_eq!(
            lib.item(id).unwrap().unwrap().thumb_key(),
            before.thumb_key()
        );
    }

    #[test]
    fn a_rewritten_file_keeps_its_edit() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let edit = Edit::new(3, None).unwrap();
        lib.set_item_edit(id, edit).unwrap();
        let changed = NewItem {
            size: 999,
            ..new_item(folder, "/p/a.jpg", 1)
        };
        lib.update_items(&[(id, changed)]).unwrap();
        assert_eq!(lib.item(id).unwrap().unwrap().edit, edit);
    }

    #[test]
    fn a_worker_that_rendered_the_photo_before_an_edit_cannot_mark_it_ready() {
        // The render it holds was stored under the old key. Marking the row Ready would
        // leave it Ready with nothing cached under the new one, and `Ready` rows are never
        // queued again.
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let stale = lib.item(id).unwrap().unwrap();
        lib.set_item_edit(id, Edit::new(1, None).unwrap()).unwrap();

        assert!(
            !lib.set_thumb_state_if_unchanged(&stale, ThumbState::Ready, None)
                .unwrap()
        );
        assert_eq!(
            lib.item(id).unwrap().unwrap().thumb_state,
            ThumbState::Pending
        );
        let fresh = lib.item(id).unwrap().unwrap();
        assert!(
            lib.set_thumb_state_if_unchanged(&fresh, ThumbState::Ready, None)
                .unwrap()
        );
    }

    #[test]
    fn guarded_thumb_state_write_skips_stale_snapshots() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let stale = lib.item(id).unwrap().unwrap();

        let changed = NewItem {
            size: 999,
            ..new_item(folder, "/p/a.jpg", 1)
        };
        lib.update_items(&[(id, changed)]).unwrap();

        let ok = lib
            .set_thumb_state_if_unchanged(&stale, ThumbState::Failed, Some("stale"))
            .unwrap();
        assert!(!ok);

        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.size, 999);
        assert_eq!(item.thumb_state, ThumbState::Pending);
        assert_eq!(item.thumb_error, None);
    }

    #[test]
    fn pending_ids_follow_grid_order_and_skip_done_or_missing() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(b, "/p/b/1.jpg", 1),
                new_item(a, "/p/a/2.jpg", 5),
                new_item(a, "/p/a/1.jpg", 2),
            ])
            .unwrap();
        let (b1, a2, a1) = (ids[0], ids[1], ids[2]);
        // Grid order, which the thumbnail queue follows so tiles render roughly in the order
        // they will be scrolled past. Folder `a` starts at 2 and `b` at 1, so `a` — the newer
        // folder by its oldest photo — comes first, and within it 2 before 5.
        assert_eq!(
            lib.pending_thumb_ids(MediaKind::Image).unwrap(),
            [a1, a2, b1]
        );

        lib.set_thumb_state(a1, ThumbState::Ready, None).unwrap();
        lib.mark_missing(&[b1], 99).unwrap();
        assert_eq!(lib.pending_thumb_ids(MediaKind::Image).unwrap(), [a2]);
    }

    #[test]
    fn pending_ids_skip_offline_watched_folders() {
        let (_dir, lib) = temp_library();
        let (on_w, on_f) = seed_folder(&lib, Path::new("/on"));
        let (off_w, off_f) = seed_folder(&lib, Path::new("/off"));
        let ids = lib
            .insert_items(&[
                new_item(on_f, "/on/a.jpg", 1),
                new_item(off_f, "/off/b.jpg", 1),
            ])
            .unwrap();
        lib.set_watched_online(off_w, false).unwrap();
        assert_eq!(lib.pending_thumb_ids(MediaKind::Image).unwrap(), [ids[0]]);
        lib.set_watched_online(off_w, true).unwrap();
        lib.set_watched_online(on_w, false).unwrap();
        assert_eq!(lib.pending_thumb_ids(MediaKind::Image).unwrap(), [ids[1]]);
    }

    #[test]
    fn pending_ids_are_of_one_kind() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let mut video = new_item(folder, "/p/b.mp4", 2);
        video.kind = MediaKind::Video;
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1), video])
            .unwrap();
        assert_eq!(lib.pending_thumb_ids(MediaKind::Image).unwrap(), [ids[0]]);
        assert_eq!(lib.pending_thumb_ids(MediaKind::Video).unwrap(), [ids[1]]);
    }

    #[test]
    fn folders_run_newest_first_by_their_oldest_photo_not_alphabetically() {
        // THE test for the grid's ordering. Path order and date order are made to disagree:
        // `alpha` sorts first by name but holds the older photos, so under the old
        // `ORDER BY f.sort_key` rule it led the grid while the sidebar — grouped by year,
        // newest first — listed `zulu` above it. Scrolling the grid then bore no relation to
        // reading the list beside it.
        //
        // Every other ordering test in this file happens to produce the same sequence under
        // both rules, so without this one the whole change is unpinned: reverting
        // `GRID_ORDER` to the path form leaves the suite green.
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
                new_item(alpha, "/p/alpha/old.jpg", 1_000),
                new_item(zulu, "/p/zulu/new.jpg", 9_000),
            ])
            .unwrap();
        let (alpha_old, zulu_new) = (ids[0], ids[1]);

        let order: Vec<i64> = lib.grid_entries().unwrap().iter().map(|e| e.id).collect();
        assert_eq!(
            order,
            [zulu_new, alpha_old],
            "the folder whose oldest photo is newer comes first, regardless of its name"
        );
    }

    #[test]
    fn folders_starting_on_the_same_photo_date_stay_contiguous() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        // Both folders' oldest photo is at 1, so the primary sort key ties and only `f.path`
        // keeps their photos from interleaving. Case-sensitive filesystems allow both names.
        let lower = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let upper = lib.upsert_folder(watched, Some(root), "/p/A", 1).unwrap();
        lib.insert_items(&[
            new_item(lower, "/p/a/1.jpg", 1),
            new_item(upper, "/p/A/2.jpg", 1),
            new_item(lower, "/p/a/3.jpg", 3),
            new_item(upper, "/p/A/4.jpg", 4),
        ])
        .unwrap();

        let folders: Vec<i64> = lib
            .grid_entries()
            .unwrap()
            .iter()
            .map(|e| e.folder_id)
            .collect();
        assert_eq!(
            folders,
            [upper, upper, lower, lower],
            "a tie on the folder's oldest photo must not interleave two folders' photos"
        );
        let listed: Vec<i64> = lib.folders().unwrap().iter().map(|f| f.id).collect();
        assert_eq!(listed, [root, upper, lower]);
    }

    #[test]
    fn grid_entries_are_ordered_oriented_and_skip_missing() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let rotated = NewItem {
            orientation: 6,
            ..new_item(a, "/p/a/2.jpg", 5)
        };
        let unknown = NewItem {
            width: 0,
            height: 0,
            ..new_item(b, "/p/b/1.jpg", 1)
        };
        let ids = lib
            .insert_items(&[
                unknown,
                rotated,
                new_item(a, "/p/a/1.jpg", 2),
                new_item(a, "/p/a/3.jpg", 9),
            ])
            .unwrap();
        lib.mark_missing(&[ids[3]], 1).unwrap();

        let entries = lib.grid_entries().unwrap();
        let order: Vec<i64> = entries.iter().map(|e| e.id).collect();
        // Folder `a`'s oldest live photo is at 2 and `b`'s at 1, so `a` leads; within `a`,
        // 2 before 5. The missing item at 9 is excluded and so cannot decide `a`'s position.
        assert_eq!(order, [ids[2], ids[1], ids[0]]);
        assert_eq!(entries[0].aspect, 400.0 / 300.0);
        assert_eq!(entries[1].aspect, 300.0 / 400.0);
        assert_eq!(entries[2].aspect, 1.0);
        assert_eq!(entries[0].folder_id, a);
        let expected = lib.item(ids[2]).unwrap().unwrap().thumb_key();
        assert_eq!(entries[0].thumb_key, expected);
    }

    #[test]
    fn known_items_under_covers_only_that_subtree() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let deep = lib.upsert_folder(watched, Some(a), "/p/a/deep", 1).unwrap();
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        lib.insert_items(&[
            new_item(root, "/p/top.jpg", 1),
            new_item(a, "/p/a/one.jpg", 2),
            new_item(deep, "/p/a/deep/two.jpg", 3),
            new_item(b, "/p/b/three.jpg", 4),
        ])
        .unwrap();

        let under_a = lib.known_items_under(watched, "/p/a").unwrap();
        let mut paths: Vec<&str> = under_a.keys().map(String::as_str).collect();
        paths.sort();
        assert_eq!(paths, ["/p/a/deep/two.jpg", "/p/a/one.jpg"]);

        assert_eq!(lib.known_items_under(watched, "/p").unwrap().len(), 4);
        assert!(
            lib.known_items_under(watched, "/p/missing")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn known_items_under_includes_soft_deleted_items() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let ids = lib.insert_items(&[new_item(a, "/p/a/one.jpg", 1)]).unwrap();
        lib.mark_missing(&ids, 99).unwrap();

        let under = lib.known_items_under(watched, "/p/a").unwrap();
        assert!(under["/p/a/one.jpg"].missing);
    }

    #[test]
    fn live_fingerprints_cover_all_items() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap()[0];
        let expected = lib.item(id).unwrap().unwrap().thumb_key();
        assert_eq!(lib.live_fingerprints().unwrap(), HashSet::from([expected]));
    }

    /// `new_item` builds a row with `rating: None`; this is the same row with a rating, as
    /// the scanner produces once its Picasa INI pass has confirmed a star.
    fn rated(folder: i64, path: &str, taken_at: i64, rating: u8) -> NewItem {
        NewItem {
            rating: Some(rating),
            ..new_item(folder, path, taken_at)
        }
    }

    #[test]
    fn grid_entries_report_whether_each_photo_is_starred() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[
            rated(folder, "/p/starred.jpg", 1, 3),
            rated(folder, "/p/unrated.jpg", 2, 0),
            new_item(folder, "/p/unread.jpg", 3),
        ])
        .unwrap();

        let mut starred: Vec<(String, bool)> = lib
            .grid_entries()
            .unwrap()
            .iter()
            .map(|e| (lib.item(e.id).unwrap().unwrap().path, e.starred))
            .collect();
        starred.sort();
        assert_eq!(
            starred,
            [
                ("/p/starred.jpg".to_string(), true),
                ("/p/unrated.jpg".to_string(), false),
                ("/p/unread.jpg".to_string(), false),
            ]
        );
    }

    #[test]
    fn only_photos_rated_at_least_one_star_are_counted() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[
            rated(folder, "/p/a.jpg", 1, 3),
            rated(folder, "/p/b.jpg", 2, 0),
            rated(folder, "/p/c.jpg", 3, 1),
        ])
        .unwrap();

        // Three rated photos, two of them starred: zero stars is a read rating, not a star.
        assert_eq!(lib.starred_count().unwrap(), 2);
    }

    #[test]
    fn an_unread_rating_is_not_counted_as_starred() {
        // `new_item` leaves `rating` NULL, which is what an unread row looks like. NULL is
        // not >= 1, so it must not reach the Starred count — SQL comparisons against NULL
        // are never true, and this pins that rather than trusting it.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn a_missing_item_is_not_counted_as_starred() {
        // A soft-deleted photo must not inflate the Starred count, the same way it does
        // not appear in the grid.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[rated(folder, "/p/a.jpg", 1, 4)])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 1);
        lib.mark_missing(&ids, 1).unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn a_changed_photo_keeps_the_rating_a_later_set_ratings_call_wrote() {
        // `update_items` runs when a file's size or mtime changed, and it must leave
        // `rating` alone: it is the Picasa pass, via `set_ratings`, that owns the column,
        // not a rescan of the file itself.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[rated(folder, "/p/a.jpg", 1, 0)])
            .unwrap();
        assert_eq!(lib.starred_count().unwrap(), 0);

        lib.set_ratings(&[(ids[0], 5)]).unwrap();
        lib.update_items(&[(ids[0], new_item(folder, "/p/a.jpg", 1))])
            .unwrap();
        assert_eq!(
            lib.starred_count().unwrap(),
            1,
            "a rescan of the file must not clear a rating set_ratings wrote"
        );
    }

    /// A rewritten file must lose both derived hashes, exactly as it loses `content_hash`.
    /// A row that kept a stale perceptual hash would never be a candidate again, and one
    /// that kept its group would stay grouped with photos it no longer resembles.
    #[test]
    fn replacing_a_file_clears_its_similarity_columns() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[new_item(folder, "/pics/a.jpg", 1)])
            .unwrap();
        let id = ids[0];
        lib.writer()
            .execute(
                "UPDATE items SET percep_hash = 42, similar_group = 7 WHERE id = ?1",
                [id],
            )
            .unwrap();

        // The same row, with new bytes: a size the scanner would report as changed.
        let replaced = NewItem {
            size: 999,
            ..new_item(folder, "/pics/a.jpg", 1)
        };
        lib.update_items(&[(id, replaced)]).unwrap();

        let (ph, sg): (Option<i64>, Option<i64>) = lib
            .reader()
            .unwrap()
            .query_row(
                "SELECT percep_hash, similar_group FROM items WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(ph, None, "percep_hash survived a replacement");
        assert_eq!(sg, None, "similar_group survived a replacement");
    }

    /// A look-alike is a fact about the photo as photon shows it, and an edit changes that.
    /// Kept across one, the stored hash would describe whichever picture was current when
    /// the row was first hashed - so whether a crop is in the hash would depend on the
    /// order the user did things in.
    #[test]
    fn an_edit_clears_the_photos_look_alike_hash() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/pics"));
        let ids = lib
            .insert_items(&[new_item(folder, "/pics/a.jpg", 1)])
            .unwrap();
        let id = ids[0];
        let hashed = |lib: &crate::library::Library| -> (Option<i64>, Option<i64>) {
            lib.reader()
                .unwrap()
                .query_row(
                    "SELECT percep_hash, similar_group FROM items WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        };
        let stamp = |lib: &crate::library::Library| {
            lib.writer()
                .execute(
                    "UPDATE items SET percep_hash = 42, similar_group = 7 WHERE id = ?1",
                    [id],
                )
                .unwrap();
        };

        stamp(&lib);
        assert!(lib.set_item_edit(id, Edit::new(1, None).unwrap()).unwrap());
        assert_eq!(hashed(&lib), (None, None), "a turn kept the old hash");

        // And a crop, which changes the picture rather than only its orientation.
        stamp(&lib);
        let crop = Edit::new(
            1,
            Some(Crop {
                left: 0,
                top: 0,
                right: (crate::edit::CROP_UNIT / 2) as u16,
                bottom: crate::edit::CROP_UNIT as u16,
            }),
        )
        .unwrap();
        assert!(lib.set_item_edit(id, crop).unwrap());
        assert_eq!(hashed(&lib), (None, None), "a crop kept the old hash");

        // The same edit again writes nothing at all, so nothing to clear.
        stamp(&lib);
        assert!(!lib.set_item_edit(id, crop).unwrap());
        assert_eq!(
            hashed(&lib),
            (Some(42), Some(7)),
            "an edit that changed nothing cleared the hash anyway"
        );
    }

    #[test]
    fn the_starred_view_contains_exactly_the_starred_photos() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
                new_item(folder, "/p/c.jpg", 3),
            ])
            .unwrap();
        lib.set_ratings(&[(ids[0], 0), (ids[1], 1), (ids[2], 5)])
            .unwrap();

        let all: Vec<i64> = lib
            .entries_for(GridView::All, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        let starred: Vec<i64> = lib
            .entries_for(GridView::Starred, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(all, ids);
        assert_eq!(
            starred,
            vec![ids[1], ids[2]],
            "unrated and zero-rated are excluded"
        );
    }

    #[test]
    fn the_starred_view_places_a_folder_by_its_oldest_starred_photo() {
        // The ordering's per-folder minimum is taken over the rows the view actually shows,
        // not over the whole folder: the sidebar's sections come from this same filtered
        // index, so a folder whose only star is recent must sort as a recent folder in
        // Starred even though its unstarred photos go back years. An implementation that
        // computed each folder's oldest photo once, over every live row, would pass every
        // other ordering test and fail this one.
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let alpha = lib
            .upsert_folder(watched, Some(root), "/p/alpha", 1)
            .unwrap();
        let zulu = lib
            .upsert_folder(watched, Some(root), "/p/zulu", 1)
            .unwrap();
        lib.insert_items(&[
            rated(alpha, "/p/alpha/old-unstarred.jpg", 1, 0),
            rated(alpha, "/p/alpha/new-starred.jpg", 9, 1),
            rated(zulu, "/p/zulu/starred.jpg", 5, 1),
        ])
        .unwrap();

        let folders: Vec<i64> = lib
            .entries_for(GridView::Starred, "")
            .unwrap()
            .iter()
            .map(|e| e.folder_id)
            .collect();
        assert_eq!(
            folders,
            [alpha, zulu],
            "alpha's oldest *starred* photo (9) is newer than zulu's (5), so alpha leads; \
             by its oldest photo overall (1) it would trail"
        );
    }

    #[test]
    fn the_recent_view_is_the_newest_photos_first_across_folders() {
        let (_dir, lib) = temp_library();
        let (watched, older_folder) = seed_folder(&lib, Path::new("/p/older"));
        let newer_folder = lib.upsert_folder(watched, None, "/p/newer", 1).unwrap();
        // The older *folder* (by its oldest photo) holds the newest single photo, so a
        // result in folder order would put `/p/older/new.jpg` last instead of first.
        let ids = lib
            .insert_items(&[
                new_item(older_folder, "/p/older/old.jpg", 1),
                new_item(older_folder, "/p/older/new.jpg", 40),
                new_item(newer_folder, "/p/newer/a.jpg", 20),
                new_item(newer_folder, "/p/newer/b.jpg", 30),
            ])
            .unwrap();

        let recent: Vec<i64> = lib
            .entries_for(GridView::Recent, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();

        assert_eq!(recent, vec![ids[1], ids[3], ids[2], ids[0]]);
    }

    /// The Tag view probes `item_tags_tag` for each keyword that answers to the name, and
    /// `item_user_tags_tag` for tags the user added under it. A filter rewritten through
    /// `EFFECTIVE_TAGS` returns the same rows, but its `coalesce` cannot use either index,
    /// and every Tag view click would scan every keyword.
    #[test]
    fn the_tag_view_is_served_by_its_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                grid_query(GRID_COLUMNS, Shown::Visible, TAG_FILTER)
            ))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map(["x"], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|step| step.contains("item_tags_tag")),
            "expected an index probe, got {plan:?}"
        );
        assert!(
            plan.iter().any(|step| step.contains("item_user_tags_tag")),
            "expected the user tags arm to probe its index, got {plan:?}"
        );
        // Every step is a probe today. Keyword scans show under the table's alias
        // (`SCAN t`), so the check is for any scan rather than for one table's name.
        assert!(
            !plan.iter().any(|step| step.starts_with("SCAN")),
            "no scan: {plan:?}"
        );
    }

    /// Pins that the Recent query is actually served by `items_recent` rather than by a
    /// scan and sort. An index whose columns or direction drift from the `ORDER BY` still
    /// exists and still passes the migration test, but SQLite silently stops using it.
    #[test]
    fn the_recent_view_is_served_by_its_index() {
        let (_dir, lib) = temp_library();
        let conn = lib.reader().unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {}", recent_sql()))
            .unwrap();
        let plan: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|step| step.contains("items_recent")),
            "expected an index walk, got {plan:?}"
        );
        assert!(
            !plan.iter().any(|step| step.contains("TEMP B-TREE")),
            "the order must come from the index, not a sort: {plan:?}"
        );
    }

    /// Pins that the Starred and Videos grid queries read only the rows they show, through
    /// `items_starred` and `items_videos`: both the driver (a folder's oldest shown photo) and
    /// the outer per-folder walk. Without them both read `items_folder`, every row of the
    /// library, to return a few percent of it. All is pinned to `items_folder`, so a new
    /// partial index its WHERE implies - one that would tie with the view indexes and take
    /// them over (CLAUDE.md, "Schema") - fails here rather than slowing Starred silently.
    #[test]
    fn the_all_starred_and_videos_views_are_served_by_their_indexes() {
        let (_dir, lib) = temp_library();
        for (filter, index) in [
            ("", "items_folder"),
            (STARRED_FILTER, "items_starred"),
            (VIDEO_FILTER, "items_videos"),
        ] {
            let plan = lib.query_plan(&grid_query(GRID_COLUMNS, Shown::Visible, filter), &[]);
            // The driver and the walk are each one step on `i`, and a bare `SCAN i` is one
            // of them too; `has_copies`' subquery reads `items` under no alias, through its
            // own indexes.
            let on_i: Vec<&String> = plan
                .iter()
                .filter(|step| step.split(' ').nth(1) == Some("i"))
                .collect();
            assert_eq!(on_i.len(), 2, "{index}: the driver and the walk: {plan:?}");
            assert!(
                on_i.iter()
                    .all(|step| step.split(' ').nth(4) == Some(index)),
                "{index}: every read of the view's rows goes through it: {plan:?}"
            );
        }
    }

    /// Pins the Starred and Videos counts to their indexes: the count reads the view's rows
    /// and nothing else. The Videos count lost its `+` to reach `items_videos`, which is what
    /// keeps it off `items_size` now (`library/mod.rs`).
    #[test]
    fn the_starred_and_video_counts_are_served_by_their_indexes() {
        let (_dir, lib) = temp_library();
        for (sql, index) in [
            (STARRED_COUNT_SQL.to_string(), "items_starred"),
            (video_count_sql(), "items_videos"),
        ] {
            let plan = lib.query_plan(&sql, &[]);
            assert_eq!(plan.len(), 1, "{index}: one step: {plan:?}");
            assert_eq!(
                plan[0].split(' ').nth(4),
                Some(index),
                "{index}: expected the count to read its index: {plan:?}"
            );
        }
    }

    /// Pins the `+` in `FILE_NAMES_SQL`, which a name sort reads on every rebuild: without
    /// it the planner walks `items_size` (`library/mod.rs`).
    #[test]
    fn the_file_names_are_read_by_a_scan_not_through_the_size_index() {
        let (_dir, lib) = temp_library();
        let plan = lib.query_plan(FILE_NAMES_SQL, &[]);
        assert!(
            !plan.iter().any(|step| step.contains("items_size")),
            "walks the size index: {plan:?}"
        );
        assert_eq!(plan, ["SCAN items"], "expected a scan in table order");
    }

    #[test]
    fn the_recent_view_keeps_only_the_newest_recent_limit_photos() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let items: Vec<NewItem> = (0..RECENT_LIMIT + 10)
            .map(|i| new_item(folder, &format!("/p/{i:04}.jpg"), i as i64))
            .collect();
        let ids = lib.insert_items(&items).unwrap();

        let recent: Vec<i64> = lib
            .entries_for(GridView::Recent, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();

        assert_eq!(recent.len(), RECENT_LIMIT);
        // The ten oldest are the ones dropped, and the newest is still first.
        assert_eq!(recent[0], *ids.last().unwrap());
        assert_eq!(*recent.last().unwrap(), ids[10]);
    }

    #[test]
    fn a_missing_photo_is_not_in_the_recent_view() {
        // A soft-deleted photo must not occupy one of the slots, the same way it does not
        // appear in the grid or the Starred count.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        lib.mark_missing(&ids[1..], 50).unwrap();

        let recent: Vec<i64> = lib
            .entries_for(GridView::Recent, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(recent, vec![ids[0]]);
    }

    #[test]
    fn search_matches_a_substring_of_the_file_name() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/sunset-beach.jpg", 1),
                new_item(folder, "/p/mountain.jpg", 2),
            ])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "beach")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(hits, vec![ids[0]]);
    }

    /// `search::Query` knows `video` and `photo`; this pins that the library hands it each
    /// row's real kind (`GRID_COLUMNS`' seventh column), not a default. Neither file name
    /// contains either word, so only the kind can answer.
    #[test]
    fn search_filters_on_the_rows_kind() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let clip = NewItem {
            kind: MediaKind::Video,
            ..new_item(folder, "/p/b.mp4", 2)
        };
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1), clip])
            .unwrap();
        let hits = |query: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, query)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(hits("video"), vec![ids[1]]);
        assert_eq!(hits("photo"), vec![ids[0]]);
    }

    #[test]
    fn search_reads_the_confined_fields_from_the_library() {
        // Each query can be answered only by the field its prefix names reaching the
        // matcher: the names match nothing, and `b.jpg` has none of it.
        use crate::edit::Edit;
        use crate::picasa::Face;
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let tagged = NewItem {
            tags: vec!["Zoo".into()],
            ..new_item(folder, "/p/a.jpg", 10)
        };
        let ids = lib
            .insert_items(&[tagged, new_item(folder, "/p/b.jpg", 20)])
            .unwrap();
        lib.upsert_contacts(&HashMap::from([(
            "c1".to_string(),
            "Anna Schmidt".to_string(),
        )]))
        .unwrap();
        lib.set_item_faces(&[(
            ids[0],
            vec![Face {
                contact: "c1".into(),
                left: 0.1,
                top: 0.1,
                right: 0.2,
                bottom: 0.2,
            }],
        )])
        .unwrap();
        let album = lib.create_album("Best Of", 1).unwrap();
        lib.add_to_album(album.id, &[ids[0]], 1).unwrap();
        lib.set_ratings(&[(ids[0], 1)]).unwrap();
        lib.set_item_edit(ids[0], Edit::new(1, None).unwrap())
            .unwrap();
        lib.set_folder_alias(folder, Some("Holiday")).unwrap();

        let hits = |query: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, query)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };
        for query in [
            "tag:zoo",
            "person:anna",
            "person:\"anna schmidt\"",
            "album:best",
            "is:starred",
            "is:edited",
        ] {
            assert_eq!(hits(query), vec![ids[0]], "{query}");
        }
        for query in [
            "-tag:zoo",
            "-person:anna",
            "-album:best",
            "-is:starred",
            "-is:edited",
        ] {
            assert_eq!(hits(query), vec![ids[1]], "{query}");
        }
        // The folder by its name and by its alias; both photos are in it.
        assert_eq!(hits("folder:holiday").len(), 2);
        assert_eq!(hits("folder:p").len(), 2);
        assert!(hits("folder:zoo").is_empty());
        // A person or an album is found by its prefix only.
        assert!(hits("anna").is_empty());
        assert!(hits("best").is_empty());
    }

    #[test]
    fn search_finds_a_photo_by_its_camera_lens_keyword_and_date() {
        // One haystack per field, each pinned by a query only it can answer. The file and
        // folder names are chosen to match none of the queries, so a hit proves the field
        // reached the matcher and was spelled the way a person types it.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let tagged = NewItem {
            camera: CameraMeta {
                make: Some("Canon".into()),
                model: Some("EOS 5D".into()),
                lens: Some("EF50mm f/1.8 STM".into()),
                focal_mm: Some(50.0),
                aperture: Some(1.8),
                exposure_s: Some(0.004),
                iso: Some(3200),
                gps: None,
            },
            tags: vec!["Zoo".into(), "family".into()],
            ..new_item(folder, "/p/a.jpg", 1_718_454_645) // 2024-06-15
        };
        let ids = lib
            .insert_items(&[tagged, new_item(folder, "/p/b.jpg", 1)])
            .unwrap();

        for query in [
            "canon",
            "5d",
            "stm",
            "50mm",
            "f/1.8",
            "iso3200",
            "zoo",
            "family",
            "2024-06-15",
            "2024-06",
            "2024",
        ] {
            let hits: Vec<i64> = lib
                .entries_for(GridView::Search, query)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect();
            assert_eq!(hits, vec![ids[0]], "{query:?} finds the tagged photo alone");
        }
        assert!(
            lib.entries_for(GridView::Search, "nikon")
                .unwrap()
                .is_empty(),
            "a camera it was not shot with finds nothing"
        );
    }

    #[test]
    fn search_narrows_by_capture_date_with_from_and_to() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1_560_556_800), // 2019-06-15
                new_item(folder, "/p/b.jpg", 1_577_836_800), // 2020-01-01
            ])
            .unwrap();
        let found = |q: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, q)
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(found("to:2019"), vec![ids[0]]);
        assert_eq!(found("from:2020"), vec![ids[1]]);
        assert_eq!(found("from:2019-06 to:2019-06"), vec![ids[0]]);
    }

    #[test]
    fn search_finds_a_photo_by_a_word_of_its_caption() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut captioned = new_item(folder, "/p/IMG_1.jpg", 1);
        captioned.caption = Some("Grandma's 80th, Lisbon".into());
        let ids = lib
            .insert_items(&[captioned, new_item(folder, "/p/IMG_2.jpg", 2)])
            .unwrap();
        let found = |q: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, q)
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(found("lisbon"), vec![ids[0]], "a caption word, any case");
        assert_eq!(
            found("grandma LISBON"),
            vec![ids[0]],
            "words AND across the caption"
        );
        assert!(found("madrid").is_empty());
    }

    #[test]
    fn a_quoted_phrase_matches_a_caption_across_a_stored_line_break() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut captioned = new_item(folder, "/p/IMG_1.jpg", 1);
        captioned.caption = Some("on the\nterrace".into());
        let ids = lib.insert_items(&[captioned]).unwrap();
        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "\"on the terrace\"")
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(hits, vec![ids[0]]);
    }

    #[test]
    fn search_matches_a_folders_alias_and_still_its_name() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/dcim-0412"));
        let ids = lib
            .insert_items(&[new_item(folder, "/dcim-0412/a.jpg", 1)])
            .unwrap();
        lib.set_folder_alias(folder, Some("Easter")).unwrap();

        let search = |q: &str| -> Vec<i64> {
            let entries = lib.entries_for(GridView::Search, q).unwrap();
            entries.iter().map(|e| e.id).collect()
        };
        assert_eq!(search("easter"), ids, "the alias did not match");
        assert_eq!(search("dcim"), ids, "the directory name stopped matching");
    }

    /// Search folds a folder's name and alias once per folder rather than once per photo,
    /// and each photo reuses them. A folder that was not refolded would lend its name to the
    /// next folder's photos, and a folder with an alias followed by one without (Gamma, then
    /// Beta: the newest folder comes first) is what shows an alias left behind. Every other
    /// search test passes with a cache that is never refreshed.
    #[test]
    fn search_reads_each_folders_own_name_and_alias_across_folder_runs() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/lib"));
        let alpha = lib
            .upsert_folder(watched, Some(root), "/lib/Alpha", 1)
            .unwrap();
        let beta = lib
            .upsert_folder(watched, Some(root), "/lib/Beta", 1)
            .unwrap();
        let gamma = lib
            .upsert_folder(watched, Some(root), "/lib/Gamma", 1)
            .unwrap();
        lib.set_folder_alias(alpha, Some("Spring")).unwrap();
        lib.set_folder_alias(gamma, Some("Autumn")).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(alpha, "/lib/Alpha/a1.jpg", 1),
                new_item(alpha, "/lib/Alpha/a2.jpg", 2),
                new_item(beta, "/lib/Beta/b1.jpg", 10),
                new_item(beta, "/lib/Beta/b2.jpg", 11),
                new_item(gamma, "/lib/Gamma/g1.jpg", 20),
                new_item(gamma, "/lib/Gamma/g2.jpg", 21),
            ])
            .unwrap();
        let found = |q: &str| -> Vec<i64> {
            let mut hits: Vec<i64> = lib
                .entries_for(GridView::Search, q)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect();
            hits.sort_unstable();
            hits
        };
        assert_eq!(found("alpha"), ids[0..2]);
        assert_eq!(found("beta"), ids[2..4]);
        assert_eq!(found("gamma"), ids[4..6]);
        assert_eq!(found("spring"), ids[0..2]);
        assert_eq!(found("autumn"), ids[4..6]);
    }

    #[test]
    fn search_matches_a_substring_of_the_folder_name() {
        let (_dir, lib) = temp_library();
        let (_watched, holiday) = seed_folder(&lib, Path::new("/holiday-2024"));
        let ids = lib
            .insert_items(&[new_item(holiday, "/holiday-2024/a.jpg", 1)])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "holiday")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(
            hits, ids,
            "the folder's name matches even though the file's does not"
        );
    }

    #[test]
    fn a_query_matching_neither_name_returns_nothing() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();

        assert!(
            lib.entries_for(GridView::Search, "zzz").unwrap().is_empty(),
            "no match is an empty result, not the whole library"
        );
    }

    #[test]
    fn search_matches_any_word_of_a_multi_word_query() {
        // The case the single-needle matcher missed: "lake bell" is not a substring of
        // "lake_bell.jpg", because the file's separator is an underscore.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/lake_bell.jpg", 1),
                new_item(folder, "/p/mountain.jpg", 2),
            ])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "lake bell")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(hits, vec![ids[0]]);
    }

    #[test]
    fn search_narrows_with_each_added_word_and_widens_on_or() {
        // Words are AND-ed, each free to match in a different field: "trip" is only in
        // the folder name and "lake" only in a file name. Under the OR this replaced,
        // "trip lake" returned all three photos; an implementation that wants both words
        // in one haystack returns none.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/trip"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/trip/lake.jpg", 1),
                new_item(folder, "/trip/bell.jpg", 2),
                new_item(folder, "/trip/mountain.jpg", 3),
            ])
            .unwrap();

        let hits = |q: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, q)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };

        assert_eq!(hits("trip"), ids);
        assert_eq!(hits("trip lake"), vec![ids[0]]);
        assert_eq!(hits("lake bell"), Vec::<i64>::new());
        assert_eq!(hits("lake OR bell"), vec![ids[0], ids[1]]);
    }

    #[test]
    fn search_confines_a_prefixed_term_to_the_camera_or_the_lens() {
        // The folder is named after a camera maker, so the bare word finds both photos
        // and only the prefix tells them apart. Proves the make, model and lens columns
        // reach `Fields` rather than only the catch-all haystacks.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/canon"));
        let shot = NewItem {
            camera: CameraMeta {
                make: Some("Canon".into()),
                model: Some("EOS 5D".into()),
                lens: Some("EF50mm f/1.8 STM".into()),
                ..CameraMeta::default()
            },
            ..new_item(folder, "/canon/a.jpg", 1)
        };
        let ids = lib
            .insert_items(&[shot, new_item(folder, "/canon/b.jpg", 2)])
            .unwrap();
        let hits = |q: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, q)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };

        assert_eq!(hits("canon"), ids);
        assert_eq!(hits("camera:canon"), vec![ids[0]]);
        assert_eq!(hits("camera:\"canon eos 5d\""), vec![ids[0]]);
        assert_eq!(
            hits("camera:5d"),
            vec![ids[0]],
            "the model, not only the make"
        );
        assert_eq!(hits("lens:stm"), vec![ids[0]]);
        assert_eq!(hits("lens:canon"), Vec::<i64>::new());
        assert_eq!(hits("camera:stm"), Vec::<i64>::new());
    }

    #[test]
    fn search_spans_folders_in_grid_order() {
        // Results cross folders and keep the grid's order. `/p/new` and `/p/old` each hold
        // a photo matched by one side of "lake OR bell", plus a photo in `/p/old` matched
        // by neither.
        let (_dir, lib) = temp_library();
        let (watched, old_folder) = seed_folder(&lib, Path::new("/p/old"));
        let new_folder = lib.upsert_folder(watched, None, "/p/new", 1).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(old_folder, "/p/old/lake.jpg", 1),
                new_item(old_folder, "/p/old/mountain.jpg", 5),
                new_item(new_folder, "/p/new/bell.jpg", 10),
            ])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "lake OR bell")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();

        // `GRID_ORDER` places folders by their oldest photo descending, regardless of
        // whether that photo matches: `/p/old`'s oldest is `lake.jpg` at 1, `/p/new`'s
        // oldest (its only photo) is `bell.jpg` at 10. 10 > 1, so `/p/new` sorts first.
        // `mountain.jpg` matches neither side and is dropped, leaving one row per folder,
        // so within-folder order does not come into play here.
        assert_eq!(hits, vec![ids[2], ids[0]]);
    }

    /// Each key reads its own column - a size sort ordered by mtime would pass a test whose
    /// two columns agreed, so here they run opposite ways - and the name sort crosses
    /// folders, ignores case and reads numbers.
    #[test]
    fn sorted_entries_order_every_photo_by_the_key_across_folders() {
        use crate::sort::{Sort, SortKey};
        let (_dir, lib) = temp_library();
        let (watched, old_folder) = seed_folder(&lib, Path::new("/p/old"));
        let new_folder = lib.upsert_folder(watched, None, "/p/new", 1).unwrap();
        let sized = |folder, path: &str, taken_at, size, mtime_ms| NewItem {
            size,
            mtime_ms,
            ..new_item(folder, path, taken_at)
        };
        let ids = lib
            .insert_items(&[
                sized(old_folder, "/p/old/IMG_10.jpg", 1, 300, 1_000),
                sized(old_folder, "/p/old/beach.jpg", 2, 100, 3_000),
                sized(new_folder, "/p/new/img_2.jpg", 10, 200, 2_000),
            ])
            .unwrap();
        let order = |key, reverse| -> Vec<i64> {
            lib.sorted_entries(GridView::All, "", Sort { key, reverse })
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };
        // By date, the view's own order: `/p/new` first, by its oldest photo.
        assert_eq!(order(SortKey::Date, false), [ids[2], ids[0], ids[1]]);
        assert_eq!(order(SortKey::Date, true), [ids[1], ids[0], ids[2]]);
        assert_eq!(order(SortKey::Name, false), [ids[1], ids[2], ids[0]]);
        assert_eq!(order(SortKey::Size, false), [ids[0], ids[2], ids[1]]);
        assert_eq!(order(SortKey::Modified, false), [ids[1], ids[2], ids[0]]);
        assert_eq!(order(SortKey::Modified, true), [ids[0], ids[2], ids[1]]);
    }

    #[test]
    fn search_folds_case_for_non_ascii_text() {
        // This is the test that pins the whole "match in Rust, not in SQL" decision
        // (spec §3): SQLite's LIKE and lower() fold ASCII only, so a `LIKE`-based
        // implementation passes the ASCII cases above and fails this one.
        // `case_folds_outside_ascii` in `search.rs` pins the same property at the unit
        // level; what this copy adds is proof that both names actually reach the matcher,
        // read from the right columns of the grid query.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/München"));
        lib.insert_items(&[new_item(folder, "/München/Straße.jpg", 1)])
            .unwrap();

        // Of these four, only "MÜNCHEN" actually discriminates the Rust-vs-SQL-LIKE
        // design: SQLite's LIKE folds ASCII case, and `ü` already matches `ü` exactly,
        // so `'München' LIKE '%münchen%'` is true under LIKE too. `Ü` is the character
        // LIKE does not fold. Do not trim this loop down without keeping "MÜNCHEN".
        for query in ["münchen", "MÜNCHEN", "München"] {
            assert_eq!(
                lib.entries_for(GridView::Search, query).unwrap().len(),
                1,
                "{query} must find the folder München regardless of case"
            );
        }
        assert_eq!(
            lib.entries_for(GridView::Search, "straße").unwrap().len(),
            1,
            "the file Straße.jpg is found by its own name"
        );
    }

    #[test]
    fn search_does_not_treat_ss_and_eszett_as_the_same_letter() {
        // A documented limit, not an aspiration. Rust's `to_lowercase` maps "Straße" to
        // "straße" and "STRASSE" to "strasse", so the two spellings never meet. Someone
        // who types `strasse` looking for `Straße.jpg` finds nothing.
        //
        // Left as-is deliberately: fixing it means full Unicode case-folding (ß → ss),
        // which needs a dependency or a hand-rolled table, and this is a simple search.
        // The test exists so the behaviour is a decision on record rather than a surprise.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/Straße.jpg", 1)])
            .unwrap();

        assert!(
            lib.entries_for(GridView::Search, "strasse")
                .unwrap()
                .is_empty(),
            "ß does not case-fold to ss"
        );
    }

    #[test]
    fn search_treats_sql_wildcards_as_literal_characters() {
        // A LIKE-based implementation would return both rows for "%", since an
        // unescaped % matches everything (spec §3).
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/50% grey.jpg", 1),
                new_item(folder, "/p/plain.jpg", 2),
            ])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "%")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(hits, vec![ids[0]], "% matches the character, not every row");

        // Same idea for `_`, LIKE's single-character wildcard (spec §7): a LIKE-based
        // implementation would return both rows, since an unescaped `_` matches any
        // one character rather than a literal underscore.
        let (_watched2, folder2) = seed_folder(&lib, Path::new("/q"));
        let underscore_ids = lib
            .insert_items(&[
                new_item(folder2, "/q/snap_01.jpg", 1),
                new_item(folder2, "/q/noseparator.jpg", 2),
            ])
            .unwrap();
        let underscore_hits: Vec<i64> = lib
            .entries_for(GridView::Search, "_")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(
            underscore_hits,
            vec![underscore_ids[0]],
            "_ matches the character, not any character"
        );
    }

    #[test]
    fn search_keeps_grid_order_and_excludes_missing_items() {
        // Three items so surviving results can actually show an order: a one-element
        // result cannot discriminate `{GRID_ORDER}` from no ordering at all, which is
        // exactly the gap this test used to leave (search_entries has its own SQL
        // string, separate from entries_filtered's, and nothing else exercised it).
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/trip"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/trip/b.jpg", 2),
                new_item(folder, "/trip/a.jpg", 1),
                new_item(folder, "/trip/c.jpg", 3),
            ])
            .unwrap();
        let (b, a, c) = (ids[0], ids[1], ids[2]);
        // `mark_missing` takes a timestamp as its second argument; the existing tests in
        // this file call it as `mark_missing(&ids, 99)`.
        lib.mark_missing(&[c], 99).unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "trip")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(
            hits,
            vec![a, b],
            "a missing item is excluded, and the survivors keep grid order (by taken_at here)"
        );
    }

    #[test]
    fn an_empty_search_query_matches_nothing_rather_than_everything() {
        // The engine turns an empty query back into the All view (Task 2); this is the
        // safety net under that, so a bug there shows as an empty grid rather than as a
        // "search" indistinguishable from the full library.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)])
            .unwrap();

        assert!(lib.entries_for(GridView::Search, "").unwrap().is_empty());
        assert!(lib.entries_for(GridView::Search, "   ").unwrap().is_empty());
    }

    #[test]
    fn the_all_and_starred_views_are_unchanged_by_the_new_entry_point() {
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/a.jpg", 1),
                new_item(folder, "/p/b.jpg", 2),
            ])
            .unwrap();
        lib.set_ratings(&[(ids[1], 3)]).unwrap();

        let all: Vec<i64> = lib
            .entries_for(GridView::All, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        let starred: Vec<i64> = lib
            .entries_for(GridView::Starred, "")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(all, ids);
        assert_eq!(starred, vec![ids[1]]);
        assert_eq!(
            lib.grid_entries().unwrap().len(),
            2,
            "the convenience wrapper still means All"
        );
    }

    /// A video row: `new_item` with the kind set, as the scanner writes one.
    fn video_at(folder: i64, path: &str, taken_at: i64) -> NewItem {
        NewItem {
            kind: MediaKind::Video,
            ..new_item(folder, path, taken_at)
        }
    }

    /// The Videos view holds the visible videos only, and places each folder by its oldest
    /// *video*, as Starred places one by its oldest starred photo - the rule that keeps the
    /// sidebar's year groups agreeing with the grid. The fixture is built so that rule and the
    /// All view's order disagree: folder `a` has the oldest item overall (a photo, at 1) but
    /// the newest video (at 100), so by oldest photo it sorts after `b`, and by oldest video
    /// before it.
    #[test]
    fn the_videos_view_holds_the_videos_placed_by_their_oldest_video() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(a, "/p/a/photo.jpg", 1),
                video_at(a, "/p/a/clip.mp4", 100),
                video_at(b, "/p/b/clip.mov", 50),
            ])
            .unwrap();
        let view = |v: GridView| -> Vec<i64> {
            lib.entries_for(v, "")
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };

        assert_eq!(view(GridView::Videos), vec![ids[1], ids[2]]);
        // The All view orders the same folders the other way round, so the order above is the
        // Videos view's own placement and not an accident of insertion.
        let all = view(GridView::All);
        let pos = |id| all.iter().position(|&x| x == id).unwrap();
        assert!(pos(ids[2]) < pos(ids[1]));
        assert_eq!(lib.video_count().unwrap(), 2);
    }
}
