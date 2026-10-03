//! Command implementations as plain functions over `Engine`. `ipc.rs` exposes them to
//! the UI as Tauri commands.

pub use crate::memory::MemoryUsage;
use crate::{engine::Engine, error::AppError, memory};
use photon_core::{
    Error,
    edit::{Crop, Edit},
    face_detect::{Rect, merge},
    grid::{FolderTally, GridEntry, GridView, Section, hex_key},
    library::{
        Album, AlbumSummary, CopiesArg, FaceFilter, Folder, GridTile, ItemFace, NamedItems,
        PageFace, PeoplePage, Person, RemovedItems, SavedSearch, TagCount, TagRule, ThemeChoice,
        WatchedFolder, is_starred,
    },
    media::{MediaKind, ThumbState},
    now_ms,
    sort::Sort,
    thumbs::Priority,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const MAX_ROWS: usize = 1000;
pub const MAX_RADIUS: usize = 10;

type CmdResult<T> = Result<T, AppError>;

/// What one export came to: how many copies were written, how many photos could not be, and
/// the first reason why not. `failed` counts a photo that has gone from the library since
/// the grid was built as well as one that could not be read.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub written: usize,
    pub failed: usize,
    pub reason: Option<String>,
}

/// What one keyword write to a selection came to: the name stored - which is not always the
/// name typed, since a keyword the user has renamed stores as its new name - and how many
/// photos took it. The count is what the toast says, and it can be short of the selection:
/// a photo purged or gone missing since the grid was built is skipped, not refused.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagWrite {
    pub tag: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderList {
    pub watched: Vec<WatchedFolder>,
    pub folders: Vec<Folder>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridInfo {
    pub version: u64,
    pub len: usize,
    /// What the grid lays out and the sidebar lists, and its generation - `None` when the
    /// caller already holds this generation (`known_layout`), which is most versions: a
    /// star, a keyword, an edit, a poster frame or a hashing pass moves no photo between
    /// folders, and at 5,000 folders the two lists are about 1 MB of JSON.
    pub layout: Option<GridLayout>,
    pub starred_count: usize,
    /// Photos with a byte-identical twin; the sidebar shows the Duplicates row only above 0.
    pub duplicate_count: usize,
    /// Hidden photos; the sidebar shows the Hidden row only above 0.
    pub hidden_count: usize,
    /// Visible videos; the sidebar shows the Videos row only above 0.
    pub video_count: usize,
    pub view: GridView,
    /// What every view is sorted by. By date the grid keeps its folder sections; by any
    /// other key it is one flat run, and the sidebar drops its year groups to match.
    pub sort: Sort,
    /// The query while `view` is `Search`, otherwise empty.
    pub search_query: String,
    /// The contact hash while `view` is `Person`.
    pub person: Option<String>,
    /// The album id while `view` is `Album`.
    pub album: Option<i64>,
    /// The keyword while `view` is `Tag`.
    pub tag: Option<String>,
    /// The photo while `view` is `Copies`.
    pub copies_of: Option<CopiesOf>,
    /// Why the grid is empty when it is only because photon could not read the library at
    /// startup (`Engine::build_first_grid`); the UI says so instead of "No photos yet".
    /// `None` for every grid actually built, so the next successful rebuild clears it.
    pub build_error: Option<String>,
}

/// The grid's layout at one generation (`Engine::published`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridLayout {
    pub generation: u64,
    /// The runs the grid lays out: one per folder, or one headerless run in a flat view.
    pub sections: Vec<Section>,
    /// The folders the view's photos come from, whatever the layout. The sidebar's list.
    pub folders: Vec<FolderTally>,
}

/// The photo a Copies view is of. The name travels with the id because the sidebar labels
/// the view by it, and asking again per render would be a round trip for a constant.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopiesOf {
    pub id: i64,
    /// Empty when the photo has left the library since the view opened; the UI keeps the
    /// name it already had.
    pub file_name: String,
    /// True once the anchor photo itself is gone - purged, or missing - from the library.
    /// The membership filter keys off the anchor's own row (`COPIES_FILTER`), so once that
    /// row is gone every branch matches nothing and the grid empties even though the other
    /// copies are still live; this field is what lets the UI say *that*, rather than "no
    /// other copies", which would be a lie about photos still sitting in the library.
    pub gone: bool,
    /// True once the user has hidden the anchor. Its copies stay in the view - the filter
    /// still reads the anchor's row - but the anchor itself does not, and the UI says why.
    pub hidden: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridRows {
    pub version: u64,
    pub rows: Vec<GridEntry>,
}

/// One folder's photos in the grid, and the index version they were read from.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderIds {
    pub version: u64,
    pub ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerItem {
    pub id: i64,
    pub path: String,
    pub file_name: String,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
    /// File size in bytes, for the caption.
    pub size: i64,
    pub thumb_key: String,
    pub thumb_state: &'static str,
    pub thumb_error: Option<String>,
    pub starred: bool,
    /// Whether the user has hidden the photo: the viewer's menu offers the opposite, and
    /// Locate looks for it in the Hidden view rather than in All.
    pub hidden: bool,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub focal_mm: Option<f64>,
    pub aperture: Option<f64>,
    pub exposure_s: Option<f64>,
    pub iso: Option<i64>,
    /// Keywords from the file's XMP and IPTC, in file order.
    pub tags: Vec<String>,
    /// The caption the photo carries, if any. Shown under the photo and in the info panel.
    pub caption: Option<String>,
    /// Named faces: Picasa's, in INI order, then the confirmed faces of the people photon
    /// knows by name that are none of those. A contact linked to a person is that person.
    pub faces: Vec<ItemFace>,
    /// Faces with no name: Picasa's whose contact no INI names, then the ones photon
    /// detected that are none of Picasa's. A detection confirmed as a named person is in
    /// `faces` instead, and an unnamed Picasa face it lies over is left out with it, so the
    /// face is drawn once. In the picture as shown, like `faces`.
    ///
    /// Each carries the detection to act on, where there is one (see `UnnamedFace`).
    pub unnamed_faces: Vec<UnnamedFace>,
    /// A video plays; the viewer shows no zoom, crop or turn for it.
    pub kind: MediaKind,
    /// The video's running time, or `None` for a photo.
    pub duration_ms: Option<i64>,
    /// A video the window died opening, and must not open again: the viewer shows its
    /// `thumb_error` instead of a `<video>`. Other failed videos are still offered, without
    /// autoplay - their failure was the poster frame's, and playback may still work.
    pub video_crashed: bool,
    /// Ids of the albums the photo is in.
    pub albums: Vec<i64>,
    /// Other files with the same bytes as this one.
    pub copies: Vec<ItemCopy>,
    /// The size of the picture turned but not cropped - what `/image/<id>/uncropped`
    /// serves and the crop tool draws its rectangle on. Sent rather than read off the
    /// loaded image, because `naturalWidth` of an EXIF-rotated file is one more thing the
    /// three webviews need not agree on.
    pub uncropped_width: u32,
    pub uncropped_height: u32,
    /// What the user has done to the photo in photon, or `None` for an untouched one.
    ///
    /// For an edited photo `width`, `height` and `orientation` describe the picture *as
    /// shown* - the edited size, upright - because that is the picture every URL serves:
    /// the edit is rendered into the thumbnails and the full image, EXIF orientation
    /// included. `faces` and `unnamed_faces` are likewise mapped into the edited frame, and a face whose centre
    /// was cropped away is left out.
    pub edit: Option<ItemEdit>,
    /// Every date the photo has, for the info panel.
    pub dates: ItemDates,
    /// Where the photo was taken, when its EXIF says.
    pub gps: Option<ItemGps>,
    /// How many pixels sit at each of `photon_core::histogram::BINS` brightness steps,
    /// darkest first, counted from the grid thumbnail and so of the photo as shown. `None`
    /// for a video, and until the thumbnail exists.
    pub histogram: Option<Vec<u32>>,
}

/// A position in decimal degrees, north and east positive.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemGps {
    pub lat: f64,
    pub lon: f64,
}

/// The dates the info panel lists. Two clocks, so two units: the camera's dates are its
/// wall clock in naive seconds, like `taken_at`, and the file's are real instants in
/// milliseconds, like `mtime_ms`. The UI renders the first in UTC and the second in the
/// machine's zone; a single unit would invite formatting both the same way.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemDates {
    /// EXIF `DateTimeOriginal`; for a video, the container's creation date.
    pub taken: Option<i64>,
    /// EXIF `DateTimeDigitized`.
    pub digitized: Option<i64>,
    /// EXIF `DateTime`: when the camera or some software last wrote the file.
    pub edited: Option<i64>,
    /// The filesystem's birth time, which not every filesystem (or share) keeps.
    pub file_created_ms: Option<i64>,
    /// The mtime the library holds, not a fresh one: the one the grid was dated by.
    pub file_modified_ms: i64,
}

/// Read from the file on each call, not stored (see `photon_core::metadata::read_exif_dates`
/// for why). A file that has gone offline answers only the stored mtime.
fn item_dates(kind: MediaKind, path: &Path, mtime_ms: i64) -> ItemDates {
    let file_created_ms = std::fs::metadata(path)
        .and_then(|m| m.created())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok());
    let base = ItemDates {
        file_created_ms,
        file_modified_ms: mtime_ms,
        ..ItemDates::default()
    };
    match kind {
        MediaKind::Image => {
            let exif = photon_core::metadata::read_exif_dates(path);
            ItemDates {
                taken: exif.original,
                digitized: exif.digitized,
                edited: exif.modified,
                ..base
            }
        }
        MediaKind::Video => ItemDates {
            taken: photon_core::video::read_meta(path).taken_at,
            ..base
        },
    }
}

/// An edit on the wire. The crop is `[left, top, right, bottom]` in
/// `photon_core::edit::CROP_UNIT`s of the turned picture.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemEdit {
    pub turns: u8,
    pub crop: Option<[u16; 4]>,
}

/// What kind of relationship a listed copy has to the photo on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CopyKind {
    /// The same bytes.
    Identical,
    /// The same picture, different bytes - a resize or a re-save.
    Similar,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemCopy {
    pub id: i64,
    pub path: String,
    pub kind: CopyKind,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolderStats {
    pub watched_id: i64,
    pub photo_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    pub library_path: PathBuf,
    pub licence: &'static str,
}

pub fn clamp_count(count: usize) -> usize {
    count.min(MAX_ROWS)
}

pub fn list_folders(engine: &Engine) -> CmdResult<FolderList> {
    Ok(FolderList {
        watched: engine.lib.watched_folders()?,
        folders: engine.lib.folders()?,
    })
}

pub fn add_folder(engine: &Arc<Engine>, path: &str) -> CmdResult<WatchedFolder> {
    Ok(engine.add_folder(Path::new(path))?)
}

pub fn remove_folder(engine: &Arc<Engine>, watched_id: i64) -> CmdResult<()> {
    Ok(engine.remove_folder(watched_id)?)
}

/// Kept apart from `list_folders`, which runs on every scan completion and folder-status
/// event; only the Settings dialog needs an aggregate over every item.
pub fn watched_folder_stats(engine: &Engine) -> CmdResult<Vec<WatchedFolderStats>> {
    Ok(engine
        .lib
        .watched_photo_counts()?
        .into_iter()
        .map(|(watched_id, photo_count)| WatchedFolderStats {
            watched_id,
            photo_count,
        })
        .collect())
}

/// What the library holds, counted, for Settings' Statistics section. Asked for when that
/// section opens, not kept up to date: it reads every visible photo.
pub fn library_stats(engine: &Engine) -> CmdResult<photon_core::library::LibraryStats> {
    Ok(engine.lib.stats()?)
}

pub fn app_info(engine: &Engine) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        library_path: engine.lib.path().to_path_buf(),
        licence: env!("CARGO_PKG_LICENSE"),
    }
}

/// Asked for by the About section while it is open, not with `app_info`: it changes.
pub fn memory_usage() -> CmdResult<MemoryUsage> {
    memory::usage().map_err(AppError::internal)
}

pub fn rescan_folder(engine: &Arc<Engine>, watched_id: i64) -> CmdResult<()> {
    let watched = engine
        .lib
        .watched_folders()?
        .into_iter()
        .find(|w| w.id == watched_id)
        .ok_or(Error::NotFound(watched_id))?;
    engine.start_scan(watched);
    Ok(())
}

/// The grid as the UI draws it. `known_layout` is the layout generation the caller holds;
/// the layout is left out when it is still the published one.
pub fn grid_info(engine: &Engine, known_layout: Option<u64>) -> GridInfo {
    let (version, grid, build_error, layout_gen) = engine.published();
    // One read of the pair, so the argument reported is the one the view was built with.
    let (view, arg) = engine.view_and_arg();
    let copies_of = (view == GridView::Copies)
        .then(|| CopiesArg::parse(&arg))
        .flatten()
        .map(|CopiesArg { anchor: id, .. }| {
            let item = engine.lib.item(id).ok().flatten();
            let gone = item
                .as_ref()
                .is_none_or(|item| item.missing_since.is_some());
            let hidden = item.as_ref().is_some_and(|item| item.hidden);
            let file_name = item
                .and_then(|item| {
                    Path::new(&item.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            CopiesOf {
                id,
                file_name,
                gone,
                hidden,
            }
        });
    let counts = engine.counts();
    GridInfo {
        version,
        len: grid.len(),
        layout: (known_layout != Some(layout_gen)).then(|| GridLayout {
            generation: layout_gen,
            sections: grid.sections().to_vec(),
            folders: grid.folders().to_vec(),
        }),
        starred_count: counts.starred,
        duplicate_count: counts.duplicate,
        hidden_count: counts.hidden,
        video_count: counts.video,
        view,
        sort: engine.sort(),
        search_query: if view == GridView::Search {
            arg.clone()
        } else {
            String::new()
        },
        person: (view == GridView::Person).then(|| arg.clone()),
        album: (view == GridView::Album)
            .then(|| arg.parse().ok())
            .flatten(),
        tag: (view == GridView::Tag).then_some(arg),
        copies_of,
        build_error,
    }
}

pub fn set_person_view(engine: &Engine, person: &str) -> CmdResult<Option<u64>> {
    Ok(engine.set_person_view(person)?)
}

pub fn set_album_view(engine: &Engine, album_id: i64) -> CmdResult<Option<u64>> {
    Ok(engine.set_album_view(album_id)?)
}

pub fn set_tag_view(engine: &Engine, tag: &str) -> CmdResult<Option<u64>> {
    Ok(engine.set_tag_view(tag)?)
}

/// A photo's copies as its info panel lists them: byte-identical first, then look-alikes
/// that are not already listed. The menu's count is this list's length, so the two cannot
/// disagree about what a copy is.
fn item_copies(engine: &Engine, id: i64) -> CmdResult<Vec<ItemCopy>> {
    let identical = engine.lib.copies_of(id)?;
    let identical_ids: std::collections::HashSet<i64> = identical.iter().map(|c| c.id).collect();
    Ok(identical
        .into_iter()
        .map(|c| ItemCopy {
            id: c.id,
            path: c.path,
            kind: CopyKind::Identical,
            width: c.width,
            height: c.height,
        })
        // A look-alike that is also a byte-identical twin is already listed above; a
        // photo appearing twice in the info panel is a bug the user sees, not a detail.
        .chain(
            engine
                .lib
                .similar_of(id)?
                .into_iter()
                .filter(|c| !identical_ids.contains(&c.id))
                .map(|c| ItemCopy {
                    id: c.id,
                    path: c.path,
                    kind: CopyKind::Similar,
                    width: c.width,
                    height: c.height,
                }),
        )
        .collect())
}

/// How many copies the tile menu may offer to show; 0 hides the item.
pub fn copy_count(engine: &Engine, id: i64) -> CmdResult<usize> {
    Ok(item_copies(engine, id)?.len())
}

pub fn set_copies_view(engine: &Engine, id: i64) -> CmdResult<Option<u64>> {
    Ok(engine.set_copies_view(id)?)
}

/// Every named Picasa contact with a photo in the library, for the sidebar.
pub fn list_people(engine: &Engine) -> CmdResult<Vec<Person>> {
    Ok(engine.lib.people_with_counts()?)
}

/// Every keyword on a live photo, for the sidebar.
pub fn list_tags(engine: &Engine) -> CmdResult<Vec<TagCount>> {
    Ok(engine.lib.tags_with_counts()?)
}

/// The user's tag renames and removals, for Settings.
pub fn list_tag_rules(engine: &Engine) -> CmdResult<Vec<TagRule>> {
    Ok(engine.lib.tag_rules()?)
}

pub fn rename_tag(engine: &Engine, from: &str, to: &str) -> CmdResult<()> {
    engine.rename_tag(from, to)?;
    Ok(())
}

pub fn hide_tag(engine: &Engine, tag: &str) -> CmdResult<()> {
    engine.lib.hide_tag(tag)?;
    engine.tags_changed();
    Ok(())
}

pub fn restore_tag_rule(engine: &Engine, tag: &str) -> CmdResult<()> {
    engine.lib.restore_tag_rule(tag)?;
    engine.tags_changed();
    Ok(())
}

pub fn list_albums(engine: &Engine) -> CmdResult<Vec<AlbumSummary>> {
    Ok(engine.lib.albums_with_counts()?)
}

pub fn create_album(engine: &Engine, name: &str) -> CmdResult<Album> {
    Ok(engine.lib.create_album(name, now_ms())?)
}

pub fn rename_album(engine: &Engine, album_id: i64, name: &str) -> CmdResult<()> {
    engine.lib.rename_album(album_id, name)?;
    Ok(())
}

/// Deleting the album on screen empties the grid rather than leaving it showing rows of
/// an album that no longer exists; `albums_changed` is what does that.
pub fn delete_album(engine: &Engine, album_id: i64) -> CmdResult<()> {
    engine.lib.delete_album(album_id)?;
    engine.albums_changed();
    Ok(())
}

pub fn list_saved_searches(engine: &Engine) -> CmdResult<Vec<SavedSearch>> {
    Ok(engine.lib.saved_searches()?)
}

pub fn save_search(engine: &Engine, name: &str, query: &str) -> CmdResult<SavedSearch> {
    Ok(engine.lib.create_saved_search(name, query, now_ms())?)
}

pub fn rename_saved_search(engine: &Engine, search_id: i64, name: &str) -> CmdResult<()> {
    engine.lib.rename_saved_search(search_id, name)?;
    Ok(())
}

/// Unlike `delete_album`, this does not touch the grid. A saved search is a name over a
/// query, and the Search view is driven by the query string itself, not by this row: the
/// photos on screen are still the answer to what was typed, so deleting the bookmark
/// leaves them there rather than emptying the grid under the user.
pub fn delete_saved_search(engine: &Engine, search_id: i64) -> CmdResult<()> {
    engine.lib.delete_saved_search(search_id)?;
    Ok(())
}

pub fn add_to_album(engine: &Engine, album_id: i64, item_ids: &[i64]) -> CmdResult<()> {
    engine.lib.add_to_album(album_id, item_ids, now_ms())?;
    engine.albums_changed();
    Ok(())
}

pub fn remove_from_album(engine: &Engine, album_id: i64, item_ids: &[i64]) -> CmdResult<()> {
    engine.lib.remove_from_album(album_id, item_ids)?;
    engine.albums_changed();
    Ok(())
}

/// The view setters below answer with the grid version that shows the view they moved to,
/// or `None` when that cannot be vouched for (`Engine::rebuild_or_restore`). The rebuild's
/// `library_changed` can reach the webview before this reply does, and the UI's listener
/// has then already fetched the grid; the version is what lets the setter's own refresh
/// see that and not fetch it a second time.
pub fn set_grid_view(engine: &Engine, view: GridView) -> CmdResult<Option<u64>> {
    Ok(engine.set_view(view)?)
}

pub fn set_sort(engine: &Engine, sort: Sort) -> CmdResult<Option<u64>> {
    Ok(engine.set_sort(sort)?)
}

pub fn set_search_query(engine: &Engine, query: &str) -> CmdResult<Option<u64>> {
    Ok(engine.set_search_query(query)?)
}

pub fn grid_rows(engine: &Engine, offset: usize, count: usize) -> GridRows {
    let (version, grid) = engine.grid();
    GridRows {
        version,
        rows: grid.rows(offset, clamp_count(count)).to_vec(),
    }
}

/// The photos of the folder the photo at `offset` is in, read with the version of the index
/// the offset is resolved against: an offset is only meaningful against one version, so the
/// two come from one read and the UI discards an answer for a version it has moved past.
/// `None` when the offset is past the end.
pub fn grid_folder_ids_at(engine: &Engine, offset: usize) -> Option<FolderIds> {
    let (version, grid) = engine.grid();
    let ids = grid.folder_ids_at(offset)?;
    Some(FolderIds { version, ids })
}

pub fn grid_offset_of_folder(engine: &Engine, folder_id: i64) -> Option<usize> {
    engine.grid().1.offset_of_folder(folder_id)
}

/// Where `item_id` sits in the current grid, or `None` if it is not in this view at all.
///
/// The viewer holds an offset, and an offset is only meaningful against one version of the
/// index: a scan that indexes a photo into an earlier folder shifts every later offset by
/// one, and the viewer would then be showing a different photo than the one it was opened
/// on. This is how it re-finds the photo it is actually displaying after a rebuild.
pub fn grid_offset_of_item(engine: &Engine, item_id: i64) -> Option<usize> {
    engine.grid().1.position_of(item_id)
}

/// The folder the grid should scroll back to on launch, or `None` when there is nothing to
/// restore — a first run, or a folder that has been removed since it was recorded.
pub fn last_folder(engine: &Engine) -> CmdResult<Option<i64>> {
    Ok(engine.lib.last_folder()?)
}

/// Records the folder the grid is showing, for the next launch. Written when the folder at
/// the top of the grid changes, so this is a handful of writes per session, not per scroll.
pub fn set_last_folder(engine: &Engine, folder_id: i64) -> CmdResult<()> {
    engine.lib.set_last_folder(folder_id)?;
    Ok(())
}

/// Seconds a slideshow holds each photo.
pub fn slideshow_interval(engine: &Engine) -> CmdResult<i64> {
    Ok(engine.lib.slideshow_interval_s()?)
}

/// Stores the slideshow interval and returns the clamped value now in force.
pub fn set_slideshow_interval(engine: &Engine, seconds: i64) -> CmdResult<i64> {
    Ok(engine.lib.set_slideshow_interval_s(seconds)?)
}

/// Whether a slideshow plays the view in a mixed order.
pub fn slideshow_shuffle(engine: &Engine) -> CmdResult<bool> {
    Ok(engine.lib.slideshow_shuffle()?)
}

pub fn set_slideshow_shuffle(engine: &Engine, shuffle: bool) -> CmdResult<()> {
    engine.lib.set_slideshow_shuffle(shuffle)?;
    Ok(())
}

/// How far apart two perceptual hashes may be and still count as the same picture.
pub fn similar_distance(engine: &Engine) -> CmdResult<i64> {
    Ok(engine.lib.similar_distance()?)
}

/// Stores the look-alike distance, returns the clamped value now in force, and requests a
/// regroup at it: the pass that reaches the grid runs at the end of a scan, and nothing
/// else runs one, so without the request a changed setting would sit unseen until the next
/// unrelated scan.
pub fn set_similar_distance(engine: &Arc<Engine>, distance: i64) -> CmdResult<i64> {
    let clamped = engine.lib.set_similar_distance(distance)?;
    engine.request_similar_pass();
    Ok(clamped)
}

/// Whether photon looks for faces itself.
pub fn face_detection(engine: &Engine) -> CmdResult<bool> {
    Ok(engine.face_detection())
}

/// Switches face detection. On starts a pass in the background; off stops it and deletes
/// what it found. Returns without waiting for either.
pub fn set_face_detection(engine: &Arc<Engine>, enabled: bool) -> CmdResult<()> {
    engine.set_face_detection(enabled)?;
    Ok(())
}

/// What switching face detection off would delete that the user made: the people they
/// named. The faces themselves come back with the next pass; the names do not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceDataSummary {
    pub named_people: i64,
}

pub fn face_data_summary(engine: &Engine) -> CmdResult<FaceDataSummary> {
    Ok(FaceDataSummary {
        named_people: engine.lib.named_people_count()?,
    })
}

/// How many unnamed groups wait for a name, for the sidebar's People row.
pub fn people_to_name(engine: &Engine) -> CmdResult<i64> {
    Ok(engine.lib.people_to_name()?)
}

/// The People page, each group with the first `strip` of its faces.
pub fn people_page(engine: &Engine, strip: usize) -> CmdResult<PeoplePage> {
    Ok(engine.lib.people_page(strip)?)
}

/// A page of one person's or group's faces: the rest of a strip.
pub fn person_faces(
    engine: &Engine,
    person: i64,
    which: FaceFilter,
    offset: usize,
    limit: usize,
) -> CmdResult<Vec<PageFace>> {
    Ok(engine.lib.person_faces(person, which, offset, limit)?)
}

/// Names a group; returns the person it ended in, which is another when the name is taken.
pub fn name_person(engine: &Arc<Engine>, person: i64, name: &str) -> CmdResult<i64> {
    Ok(engine.write_people("naming a person", |lib| lib.name_group(person, name))?)
}

/// Renames a person; returns the person they ended in, as naming does.
pub fn rename_person(engine: &Arc<Engine>, person: i64, name: &str) -> CmdResult<i64> {
    Ok(engine.write_people("renaming a person", |lib| lib.rename_person(person, name))?)
}

pub fn confirm_faces(engine: &Arc<Engine>, faces: &[i64]) -> CmdResult<()> {
    Ok(engine.write_people("confirming faces", |lib| lib.confirm_faces(faces))?)
}

/// "Not this person": the faces leave their group, and the pass the write requests places
/// them elsewhere.
pub fn reject_faces(engine: &Arc<Engine>, faces: &[i64]) -> CmdResult<()> {
    Ok(engine.write_people("rejecting faces", |lib| lib.reject_faces(faces))?)
}

/// Names faces by id; the person they ended in, or `None` when none of them exists any more.
pub fn name_faces(engine: &Arc<Engine>, faces: &[i64], name: &str) -> CmdResult<Option<i64>> {
    Ok(engine.write_people("naming faces", |lib| lib.name_faces(faces, name))?)
}

/// Names each photo's one unnamed face; the answer says which photos it skipped and why.
pub fn name_items(engine: &Arc<Engine>, items: &[i64], name: &str) -> CmdResult<NamedItems> {
    Ok(engine.write_people("naming photos", |lib| lib.name_items(items, name))?)
}

/// Takes photos out of a person; a photo Picasa names them on stays theirs and is counted.
pub fn remove_from_person(
    engine: &Arc<Engine>,
    person: i64,
    items: &[i64],
) -> CmdResult<RemovedItems> {
    Ok(engine.write_people("removing photos from a person", |lib| {
        lib.remove_from_person(person, items)
    })?)
}

pub fn merge_people(engine: &Arc<Engine>, from: i64, into: i64) -> CmdResult<()> {
    Ok(engine.write_people("merging people", |lib| lib.merge_people(from, into))?)
}

pub fn ignore_person(engine: &Arc<Engine>, person: i64, ignored: bool) -> CmdResult<()> {
    Ok(engine.write_people("ignoring a person", |lib| {
        lib.set_person_ignored(person, ignored)
    })?)
}

pub fn ignore_faces(engine: &Arc<Engine>, faces: &[i64], ignored: bool) -> CmdResult<()> {
    Ok(engine.write_people("ignoring faces", |lib| {
        lib.set_faces_ignored(faces, ignored)
    })?)
}

/// Takes a person's name away: their faces stay together, as a group with no name.
pub fn delete_person(engine: &Arc<Engine>, person: i64) -> CmdResult<()> {
    Ok(engine.write_people("deleting a person", |lib| lib.delete_person(person))?)
}

/// The colour scheme the user chose.
pub fn theme(engine: &Engine) -> CmdResult<ThemeChoice> {
    Ok(engine.lib.theme()?)
}

pub fn set_theme(engine: &Engine, choice: ThemeChoice) -> CmdResult<()> {
    Ok(engine.lib.set_theme(choice)?)
}

/// How large the grid draws its tiles.
pub fn grid_tile(engine: &Engine) -> CmdResult<GridTile> {
    Ok(engine.lib.grid_tile()?)
}

pub fn set_grid_tile(engine: &Engine, tile: GridTile) -> CmdResult<()> {
    Ok(engine.lib.set_grid_tile(tile)?)
}

pub fn set_visible(engine: &Engine, ids: &[i64]) {
    engine.thumbs.set_visible(ids);
}

/// A face with no name in the viewer: the rectangle drawn, and which of photon's detections
/// naming it would name.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnnamedFace {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    /// `detected_faces.id` of the face to act on: the detection itself, or, for an outline
    /// that is Picasa's, the detection beneath it that no named person is confirmed on.
    /// `None` when Picasa's outline has no detection beneath it (detection off, or missed).
    pub face_id: Option<i64>,
}

impl UnnamedFace {
    fn new(rect: Rect, face_id: Option<i64>) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            face_id,
        }
    }
}

/// A photo the scanner has marked missing is `NotFound` here, not returned: the viewer
/// asks this after `grid_offset_of_item` comes back empty, to tell "left the current view"
/// (keep showing it) from "gone" (say so), and a missing row is the second case.
pub fn viewer_item(engine: &Engine, id: i64) -> CmdResult<ViewerItem> {
    let item = engine.lib.item(id)?.ok_or(Error::NotFound(id))?;
    if item.missing_since.is_some() {
        return Err(Error::NotFound(id).into());
    }
    let file_name = Path::new(&item.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tags = engine.lib.item_tags(item.id)?;
    let caption = engine.lib.item_caption(item.id)?;
    let edit = item.edit;
    let mut faces: Vec<ItemFace> = engine
        .lib
        .item_faces(item.id)?
        .into_iter()
        .filter_map(|face| {
            let rect = merge::shown(
                edit,
                Rect {
                    left: face.left,
                    top: face.top,
                    right: face.right,
                    bottom: face.bottom,
                },
            )?;
            Some(ItemFace {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
                ..face
            })
        })
        .collect();
    // A face photon's own person is confirmed on is named in the viewer, once: where Picasa
    // recorded the same face under any named contact, linked to a person or not, Picasa's
    // is the one shown.
    let named_here: Vec<Rect> = faces
        .iter()
        .map(|f| Rect {
            left: f.left,
            top: f.top,
            right: f.right,
            bottom: f.bottom,
        })
        .collect();
    let named_detected = engine.lib.item_named_detected_faces(item.id)?;
    let detected = engine.lib.item_detected_faces_with_ids(item.id)?;
    // A Picasa plate of a person photon knows is acted on through the detection beneath it
    // that is confirmed as that same person. A `c:` plate (a contact no person is linked
    // to) gets none: the face beneath may well be someone else's.
    for face in &mut faces {
        let Some(person) = face
            .key
            .strip_prefix("p:")
            .and_then(|k| k.parse::<i64>().ok())
        else {
            continue;
        };
        let plate = Rect {
            left: face.left,
            top: face.top,
            right: face.right,
            bottom: face.bottom,
        };
        face.face_id = detected
            .iter()
            .find(|(_, rect, p, confirmed)| {
                *confirmed && *p == Some(person) && merge::same_face(&plate, rect)
            })
            .map(|(id, ..)| *id);
    }
    let mut named_over_unnamed: Vec<Rect> = Vec::new();
    for (face_id, rect, person, name) in &named_detected {
        if named_here.iter().any(|p| merge::same_face(p, rect)) {
            continue;
        }
        named_over_unnamed.push(*rect);
        faces.push(ItemFace {
            key: format!("p:{person}"),
            name: name.clone(),
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            face_id: Some(*face_id),
        });
    }
    // One rule for both readers (`face_detect::merge`): search counts exactly these.
    let picasa: Vec<(Rect, bool)> = engine
        .lib
        .item_picasa_faces(item.id)?
        .into_iter()
        .filter_map(|(rect, named)| Some((merge::shown(edit, rect)?, named)))
        .collect();
    let all: Vec<Rect> = picasa.iter().map(|(rect, _)| *rect).collect();
    let is_named = |id: i64| named_detected.iter().any(|(n, ..)| *n == id);
    // A Picasa face no INI names, under a name plate of ours, is the one face: the plate
    // is drawn, not an outline on top of it. An outline that is Picasa's is drawn in place
    // of the detection beneath it, which is the face naming it would name.
    let mut unnamed_faces: Vec<UnnamedFace> = picasa
        .iter()
        .filter(|(rect, named)| {
            !named && !named_over_unnamed.iter().any(|n| merge::same_face(rect, n))
        })
        .map(|(rect, _)| {
            let beneath = detected
                .iter()
                .find(|(id, d, ..)| !is_named(*id) && merge::same_face(rect, d))
                .map(|(id, ..)| *id);
            UnnamedFace::new(*rect, beneath)
        })
        .collect();
    // A detection a person is confirmed on has a name plate; it is no unnamed outline.
    let unnamed_detected: Vec<(i64, Rect)> = detected
        .iter()
        .filter(|(id, ..)| !is_named(*id))
        .map(|(id, rect, ..)| (*id, *rect))
        .collect();
    // Those that are none of Picasa's.
    unnamed_faces.extend(
        merge::unmatched_by(&all, &unnamed_detected)
            .into_iter()
            .map(|(id, rect)| UnnamedFace::new(rect, Some(id))),
    );
    let (upright_w, upright_h) =
        photon_core::metadata::oriented_dims(item.width, item.height, item.orientation);
    let (uncropped_width, uncropped_height) = edit.without_crop().dims(upright_w, upright_h);
    let (width, height, orientation) = if edit.is_identity() {
        (item.width, item.height, item.orientation)
    } else {
        let (w, h) = edit.dims(upright_w, upright_h);
        (w, h, 1)
    };
    let albums = engine.lib.item_albums(item.id)?;
    let copies = item_copies(engine, item.id)?;
    let thumb_key = hex_key(item.thumb_key());
    let video_crashed = photon_core::thumbs::video_crashed(&item);
    // A video's poster frame is one instant of it; its histogram would say nothing of the
    // rest. ~0.1 ms for a photo: one small file, decoded by the library that wrote it.
    let histogram = (item.kind == MediaKind::Image)
        .then(|| engine.thumbs.histogram(&item))
        .flatten()
        .map(|bins| bins.to_vec());
    let camera = item.camera;
    let kind = item.kind;
    let duration_ms = item.duration_ms;
    let dates = item_dates(kind, Path::new(&item.path), item.mtime_ms);
    Ok(ViewerItem {
        id: item.id,
        thumb_key,
        thumb_state: match item.thumb_state {
            ThumbState::Pending => "pending",
            ThumbState::Ready => "ready",
            ThumbState::Failed => "failed",
        },
        file_name,
        width,
        height,
        orientation,
        taken_at: item.taken_at,
        size: item.size,
        thumb_error: item.thumb_error,
        starred: is_starred(item.rating),
        hidden: item.hidden,
        path: item.path,
        make: camera.make,
        model: camera.model,
        lens: camera.lens,
        focal_mm: camera.focal_mm,
        aperture: camera.aperture,
        exposure_s: camera.exposure_s,
        iso: camera.iso,
        tags,
        caption,
        faces,
        unnamed_faces,
        kind,
        duration_ms,
        video_crashed,
        albums,
        copies,
        uncropped_width,
        uncropped_height,
        edit: (!edit.is_identity()).then(|| ItemEdit {
            turns: edit.turns,
            crop: edit.crop.map(|c| [c.left, c.top, c.right, c.bottom]),
        }),
        dates,
        gps: camera.gps.map(|g| ItemGps {
            lat: g.lat,
            lon: g.lon,
        }),
        histogram,
    })
}

/// Turns one photo a quarter, keeping its crop on the same part of the picture.
pub fn rotate_item(engine: &Arc<Engine>, id: i64, clockwise: bool) -> CmdResult<()> {
    engine.rotate_item(id, clockwise)?;
    Ok(())
}

/// Replaces one photo's edit. `crop` is `[left, top, right, bottom]` in `CROP_UNIT`s of the
/// turned picture; no turns and no crop is the original again.
pub fn set_item_edit(
    engine: &Arc<Engine>,
    id: i64,
    turns: u8,
    crop: Option<[u16; 4]>,
) -> CmdResult<()> {
    let crop = crop.map(|[left, top, right, bottom]| Crop {
        left,
        top,
        right,
        bottom,
    });
    let edit = Edit::new(turns, crop).ok_or(Error::InvalidCrop)?;
    engine.set_item_edit(id, edit)?;
    Ok(())
}

/// The photo as the clipboard gets it: as shown, edits applied, capped at
/// `edit::CLIPBOARD_MAX_EDGE`. The clipboard write itself is `ipc::copy_photo`'s, which holds
/// the app handle this layer does not; here is everything a test can reach.
pub fn copy_picture(engine: &Engine, id: i64) -> CmdResult<photon_core::edit::ClipboardPicture> {
    let item = engine.lib.item(id)?.ok_or(Error::NotFound(id))?;
    if item.kind != MediaKind::Image {
        return Err(Error::NotAPhoto(id).into());
    }
    // One full-size decode at a time, the lock the viewer's render and export share. Held
    // across the render only: the caller's clipboard write must not keep the next render,
    // or the viewer's, waiting on the desktop's clipboard.
    let _one_at_a_time = crate::protocol::RENDERING.lock();
    photon_core::edit::clipboard_picture(Path::new(&item.path), item.orientation, item.edit)
        .map_err(|err| match err {
            Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound => AppError {
                kind: "notFound",
                // Not "gone": a photo on an unmounted share reads as missing too, and is only
                // offline. photon cannot tell the two apart from here.
                message: "This photo can't be read: its file is gone, or its folder is offline."
                    .into(),
            },
            err => err.into(),
        })
}

pub fn set_star(engine: &Engine, id: i64, starred: bool) -> CmdResult<()> {
    engine.set_star(id, starred)?;
    Ok(())
}

/// Stars or unstars several photos, returning how many landed. See `Engine::set_stars` for
/// why a folder that cannot be written is skipped rather than fatal.
pub fn set_stars(engine: &Engine, ids: &[i64], starred: bool) -> CmdResult<usize> {
    Ok(engine.set_stars(ids, starred)?)
}

pub fn add_item_tag(engine: &Engine, id: i64, tag: &str) -> CmdResult<String> {
    Ok(engine.add_item_tag(id, tag)?)
}

pub fn remove_item_tag(engine: &Engine, id: i64, tag: &str) -> CmdResult<()> {
    engine.remove_item_tag(id, tag)?;
    Ok(())
}

/// Adds one keyword to several photos, reporting the name stored and how many took it.
/// The name can differ from what was typed: a keyword the user has renamed stores as the
/// name they renamed it to, which is the name they will see on the photos.
pub fn add_items_tag(engine: &Engine, ids: &[i64], tag: &str) -> CmdResult<TagWrite> {
    let (tag, count) = engine.add_items_tag(ids, tag)?;
    Ok(TagWrite { tag, count })
}

/// Hides or unhides a folder and the photos in it, reporting how many photos changed.
pub fn set_folder_hidden(engine: &Engine, folder_id: i64, hidden: bool) -> CmdResult<usize> {
    Ok(engine.set_folder_hidden(folder_id, hidden)?)
}

/// Names a folder in photon, or clears the name with `None`, reporting whether it changed.
/// See `Library::set_folder_alias` for how the name is normalised.
pub fn set_folder_alias(engine: &Engine, folder_id: i64, alias: Option<String>) -> CmdResult<bool> {
    Ok(engine.set_folder_alias(folder_id, alias.as_deref())?)
}

/// Hides or unhides several photos, reporting how many changed.
pub fn set_items_hidden(engine: &Engine, ids: &[i64], hidden: bool) -> CmdResult<usize> {
    Ok(engine.set_items_hidden(ids, hidden)?)
}

/// Removes one keyword from several photos, reporting how many changed.
pub fn remove_items_tag(engine: &Engine, ids: &[i64], tag: &str) -> CmdResult<TagWrite> {
    let count = engine.remove_items_tag(ids, tag)?;
    Ok(TagWrite {
        tag: tag.to_string(),
        count,
    })
}

/// Copies photos into `dest`. See `Engine::export_items`: a destination inside a watched
/// folder is refused, and a photo that cannot be written is counted rather than fatal.
pub fn export_items(
    engine: &Engine,
    ids: &[i64],
    dest: &str,
    apply_edits: bool,
    max_edge: Option<u32>,
) -> CmdResult<ExportReport> {
    let options = photon_core::export::Options {
        apply_edits,
        max_edge,
    };
    let done = engine.export_items(ids, Path::new(dest), options)?;
    Ok(ExportReport {
        written: done.written,
        failed: done.failed,
        reason: done.reason,
    })
}

/// Whether copies may be written into `dest`. The dialog asks as soon as a folder is
/// picked, so the one refusal this feature expects is shown while it is still open.
pub fn check_export_dest(engine: &Engine, dest: &str) -> CmdResult<()> {
    engine.check_export_dest(Path::new(dest))?;
    Ok(())
}

/// Whether an export renders edits into the copies; remembered between exports.
pub fn export_apply_edits(engine: &Engine) -> CmdResult<bool> {
    Ok(engine.lib.export_apply_edits()?)
}

pub fn set_export_apply_edits(engine: &Engine, apply: bool) -> CmdResult<()> {
    engine.lib.set_export_apply_edits(apply)?;
    Ok(())
}

/// Items around `id`, nearest first, queued at neighbour priority so the viewer's
/// next and previous previews are ready early.
///
/// The ids returned are the ones whose *full image* is worth fetching ahead, which leaves
/// out edited photos: their full image is rendered per request and the viewer asks for it
/// under a keyed URL, so a preload of the bare URL costs a full-size decode and encode whose
/// result nothing ever reads; and videos, which the viewer plays rather than preloads. Their
/// thumbnails are still queued.
pub fn neighbours(engine: &Engine, id: i64, radius: usize) -> Vec<i64> {
    let ids = engine.grid().1.neighbours(id, radius.min(MAX_RADIUS));
    engine.thumbs.prioritize(&ids, Priority::Neighbour);
    ids.into_iter()
        .filter(|&id| {
            engine
                .lib
                .item(id)
                .ok()
                .flatten()
                .is_some_and(|item| item.kind == MediaKind::Image && item.edit.is_identity())
        })
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoJobDto {
    pub id: i64,
    /// Hex, as `thumbKey` is everywhere else on the wire.
    pub key: String,
}

/// How long `next_video_job` holds the call open waiting for a job. Under the webview's
/// own IPC timeouts, and long enough that an idle page asks about twice a minute.
pub const VIDEO_JOB_WAIT: Duration = Duration::from_secs(25);

pub fn media_base(server: &crate::media_server::MediaServer) -> String {
    server.base_url()
}

pub fn video_session_start(engine: &Engine, supported: bool) -> CmdResult<()> {
    Ok(engine.thumbs.video_session_start(supported)?)
}

pub fn next_video_job(engine: &Engine, wait: Duration) -> CmdResult<Option<VideoJobDto>> {
    Ok(engine.thumbs.next_video_job(wait)?.map(|job| VideoJobDto {
        id: job.id,
        key: hex_key(job.key),
    }))
}

/// The grid rebuild is coalesced (`Engine::frame_stored`), and never an error of this call:
/// the frame is stored by then, and a rejection would tell the page its `put` failed.
pub fn put_video_frame(engine: &Arc<Engine>, id: i64, key: &str, jpeg: &[u8]) -> CmdResult<()> {
    let key = crate::protocol::parse_key(key).ok_or(Error::NotFound(id))?;
    if engine.thumbs.put_video_frame(id, key, jpeg)? {
        engine.frame_stored();
    }
    Ok(())
}

pub fn video_frame_failed(
    engine: &Engine,
    id: i64,
    key: &str,
    reason: photon_core::thumbs::VideoFailure,
) -> CmdResult<()> {
    let key = crate::protocol::parse_key(key).ok_or(Error::NotFound(id))?;
    Ok(engine.thumbs.video_frame_failed(id, key, reason)?)
}

pub fn item_path(engine: &Engine, id: i64) -> CmdResult<PathBuf> {
    let item = engine.lib.item(id)?.ok_or(Error::NotFound(id))?;
    Ok(PathBuf::from(item.path))
}

/// The web page that shows where a photo was taken, or `None` for a photo with no position.
///
/// Built here from the stored position rather than taken from the webview, so the one URL
/// photon ever hands the system's browser is one it wrote itself. OpenStreetMap, because it
/// needs no key and no account; the marker and the map centre are the same point. Six
/// decimals is about a decimetre, finer than any camera's fix.
pub fn item_map_url(engine: &Engine, id: i64) -> CmdResult<Option<String>> {
    let item = engine.lib.item(id)?.ok_or(Error::NotFound(id))?;
    Ok(item.camera.gps.map(|g| {
        format!(
            "https://www.openstreetmap.org/?mlat={lat:.6}&mlon={lon:.6}#map=16/{lat:.6}/{lon:.6}",
            lat = g.lat,
            lon = g.lon
        )
    }))
}

pub fn folder_path(engine: &Engine, folder_id: i64) -> CmdResult<PathBuf> {
    let folder = engine
        .lib
        .folders()?
        .into_iter()
        .find(|f| f.id == folder_id)
        .ok_or(Error::NotFound(folder_id))?;
    Ok(PathBuf::from(folder.path))
}

/// A watched root's own path. Not `folder_path`: an offline or never-scanned root may have
/// no folder row to look up.
pub fn watched_path(engine: &Engine, watched_id: i64) -> CmdResult<PathBuf> {
    let watched = engine
        .lib
        .watched_folders()?
        .into_iter()
        .find(|w| w.id == watched_id)
        .ok_or(Error::NotFound(watched_id))?;
    Ok(PathBuf::from(watched.path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Fixture, fixture, jpeg};
    use photon_core::face_detect::{DETECTOR_VERSION, Detection};

    #[test]
    fn grid_info_leaves_out_a_layout_the_caller_holds() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        f.add_photos();
        let first = grid_info(&f.engine, None);
        let layout = first.layout.expect("a first call always gets the layout");
        assert_eq!(layout.sections.len(), 2);

        f.engine.set_star(f.ids()[0], true).unwrap();
        let again = grid_info(&f.engine, Some(layout.generation));

        assert!(again.version > first.version);
        assert!(
            again.layout.is_none(),
            "the star moved no photo between folders"
        );
        assert_eq!(again.starred_count, 1);
    }

    #[test]
    fn grid_info_sends_a_layout_that_moved() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        f.add_photos();
        let generation = grid_info(&f.engine, None).layout.unwrap().generation;

        f.engine.set_items_hidden(&[f.ids()[0]], true).unwrap();
        let after = grid_info(&f.engine, Some(generation))
            .layout
            .expect("the hide moved the layout");

        assert_eq!(after.sections.len(), 1);
        assert_eq!(after.generation, generation + 1);
    }

    #[test]
    fn folder_listing_and_grid_info() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        let watched = f.add_photos();
        let list = list_folders(&f.engine).unwrap();
        assert_eq!(
            list.watched,
            vec![photon_core::library::WatchedFolder {
                online: true,
                ..watched
            }]
        );
        assert_eq!(list.folders.len(), 2);
        let info = grid_info(&f.engine, None);
        assert_eq!(
            (info.len, info.layout.as_ref().unwrap().sections.len()),
            (2, 2)
        );
        let sub = list.folders.iter().find(|x| x.name == "sub").unwrap();
        assert_eq!(grid_offset_of_folder(&f.engine, sub.id), Some(1));
    }

    /// Recent is laid out as one headerless run whatever order its folders fall in, and the
    /// sidebar still gets each folder the photos came from.
    #[test]
    fn recent_is_one_run_under_no_folder_and_still_lists_its_folders() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        f.add_photos();
        f.engine.set_view(GridView::Recent).unwrap();
        let info = grid_info(&f.engine, None);
        assert_eq!(
            info.layout
                .as_ref()
                .unwrap()
                .sections
                .iter()
                .map(|s| (s.folder_id, s.offset, s.count))
                .collect::<Vec<_>>(),
            [(None, 0, 2)]
        );
        assert_eq!(info.layout.as_ref().unwrap().folders.len(), 2);
        assert_eq!(
            info.layout
                .as_ref()
                .unwrap()
                .folders
                .iter()
                .map(|t| t.count)
                .sum::<usize>(),
            2
        );
    }

    #[test]
    fn settings_reads_counts_paths_and_the_library_location() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        let watched = f.add_photos();
        assert_eq!(
            watched_folder_stats(&f.engine).unwrap(),
            [WatchedFolderStats {
                watched_id: watched.id,
                photo_count: 2
            }]
        );
        assert_eq!(
            watched_path(&f.engine, watched.id).unwrap(),
            PathBuf::from(&watched.path)
        );
        assert!(watched_path(&f.engine, watched.id + 1).is_err());
        let info = app_info(&f.engine);
        assert_eq!(
            info.library_path,
            f.dir.path().join("data").join("library.db")
        );
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    /// The offset the viewer holds is only meaningful against one version of the index.
    /// Indexing a photo into an earlier folder renumbers everything after it, and without a
    /// way to re-find the open photo by id the viewer silently shows its neighbour.
    #[test]
    fn an_items_offset_follows_it_when_the_grid_is_renumbered() {
        let img = jpeg(16, 16);
        let f = fixture(&[("b/second.jpg", &img)]);
        f.add_photos();
        let watched = f.engine.lib.watched_folders().unwrap()[0].clone();
        let open = f.ids()[0];
        assert_eq!(grid_offset_of_item(&f.engine, open), Some(0));

        // A folder that sorts ahead of it appears, the way a scan of a newly-copied
        // directory would add one.
        std::fs::create_dir_all(f.photos.join("a")).unwrap();
        std::fs::write(f.photos.join("a").join("first.jpg"), &img).unwrap();
        f.engine.start_scan(watched);
        f.settle();

        assert_eq!(f.ids().len(), 2);
        assert_eq!(
            grid_offset_of_item(&f.engine, open),
            Some(1),
            "the photo moved, and says where it moved to"
        );
        assert_eq!(
            grid_offset_of_item(&f.engine, 9_999),
            None,
            "a photo no longer in this view says so, rather than answering with a neighbour"
        );
    }

    #[test]
    fn viewer_item_carries_camera_keywords_faces_and_albums() {
        // The plumbing from the row to the viewer, not the readers themselves (photon-core
        // pins those): the camera columns and keywords are written the way the scanner's
        // backfill writes them, the face the way Picasa's INI delivers it.
        use photon_core::library::NewItem;
        use photon_core::media::MediaKind;
        use photon_core::metadata::CameraMeta;
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        std::fs::write(
            f.photos.join(".picasa.ini"),
            b"[Contacts2]\nabc=Ada\n[a.jpg]\nfaces=rect64(4000200080006000),abc\n",
        )
        .unwrap();
        f.add_photos();
        let id = f.ids()[0];
        let row = f.engine.lib.item(id).unwrap().unwrap();
        let described = NewItem {
            folder_id: row.folder_id,
            path: row.path.clone(),
            file_name: "a.jpg".into(),
            kind: MediaKind::Image,
            size: row.size,
            mtime_ms: row.mtime_ms,
            width: row.width,
            height: row.height,
            orientation: row.orientation,
            taken_at: row.taken_at,
            rating: None,
            camera: CameraMeta {
                make: Some("Canon".into()),
                model: Some("EOS 5D".into()),
                lens: Some("EF50mm".into()),
                focal_mm: Some(50.0),
                aperture: Some(1.8),
                exposure_s: Some(0.004),
                iso: Some(400),
                gps: None,
            },
            tags: vec!["beach".into()],
            caption: None,
            duration_ms: None,
        };
        f.engine.lib.update_item_meta(&[(id, described)]).unwrap();
        let album = f.engine.lib.create_album("Trip", 1).unwrap();
        f.engine.lib.add_to_album(album.id, &[id], 1).unwrap();

        let item = viewer_item(&f.engine, id).unwrap();
        assert_eq!(item.make.as_deref(), Some("Canon"));
        assert_eq!(item.model.as_deref(), Some("EOS 5D"));
        assert_eq!(item.lens.as_deref(), Some("EF50mm"));
        assert_eq!(item.focal_mm, Some(50.0));
        assert_eq!(item.aperture, Some(1.8));
        assert_eq!(item.exposure_s, Some(0.004));
        assert_eq!(item.iso, Some(400));
        assert_eq!(item.tags, vec!["beach"]);
        assert_eq!(item.faces.len(), 1);
        assert_eq!(item.faces[0].name, "Ada");
        assert_eq!(item.albums, vec![album.id]);
        assert_eq!(list_people(&f.engine).unwrap()[0].name, "Ada");
        assert_eq!(list_tags(&f.engine).unwrap()[0].tag, "beach");
    }

    #[test]
    fn a_copied_photo_is_the_edited_picture_and_a_missing_file_says_so() {
        let f = fixture(&[("a.jpg", &jpeg(40, 20))]);
        f.add_photos();
        let id = f.ids()[0];
        assert_eq!(copy_picture(&f.engine, id).unwrap().dimensions(), (40, 20));
        set_item_edit(&f.engine, id, 1, None).unwrap();
        assert_eq!(
            copy_picture(&f.engine, id).unwrap().dimensions(),
            (20, 40),
            "turned, as the viewer shows it"
        );
        std::fs::remove_file(f.photos.join("a.jpg")).unwrap();
        let err = copy_picture(&f.engine, id).unwrap_err();
        assert_eq!(
            (err.kind, err.message.as_str()),
            (
                "notFound",
                "This photo can't be read: its file is gone, or its folder is offline."
            )
        );
    }

    /// The plumbing from the file to the panel's dates (photon-core pins the EXIF reader):
    /// the camera's date arrives in naive seconds, the file's in milliseconds, and a date
    /// the file does not carry arrives as nothing rather than as the one it fell back to.
    #[test]
    fn viewer_item_reports_the_dates_the_file_carries() {
        // A JPEG whose EXIF holds only DateTimeOriginal (0x9003). Little-endian TIFF: IFD0
        // at 8 holds just the pointer (0x8769) to the Exif IFD at 26, whose one ASCII entry
        // is stored just past it at 44. kamadak-exif knows a tag by the IFD it sits in, so
        // the date cannot go in IFD0.
        let entry = |tag: u16, typ: u16, count: u32, value: u32| {
            let mut e = tag.to_le_bytes().to_vec();
            e.extend_from_slice(&typ.to_le_bytes());
            e.extend_from_slice(&count.to_le_bytes());
            e.extend_from_slice(&value.to_le_bytes());
            e
        };
        let mut tiff = b"II*\0".to_vec();
        tiff.extend_from_slice(&8u32.to_le_bytes());
        for (tag, typ, count, value) in [(0x8769, 4, 1, 26), (0x9003, 2, 20, 44)] {
            tiff.extend_from_slice(&1u16.to_le_bytes());
            tiff.extend_from_slice(&entry(tag, typ, count, value));
            tiff.extend_from_slice(&0u32.to_le_bytes());
        }
        tiff.extend_from_slice(b"2024:06:15 12:30:45\0");
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let plain = jpeg(16, 16);
        let mut dated = plain[..2].to_vec();
        dated.extend_from_slice(&[0xFF, 0xE1]);
        dated.extend_from_slice(&((2 + app1.len()) as u16).to_be_bytes());
        dated.extend_from_slice(&app1);
        dated.extend_from_slice(&plain[2..]);

        let f = fixture(&[("a.jpg", &dated), ("b.jpg", &plain)]);
        f.add_photos();
        let dates = |name: &str| {
            f.ids()
                .into_iter()
                .map(|id| viewer_item(&f.engine, id).unwrap())
                .find(|item| item.file_name == name)
                .unwrap()
                .dates
        };
        let a = dates("a.jpg");
        assert_eq!(a.taken, Some(1_718_454_645));
        assert_eq!((a.digitized, a.edited), (None, None));
        assert_eq!(a.file_modified_ms, 1_600_000_000_000);
        let b = dates("b.jpg");
        assert_eq!(
            b.taken, None,
            "dated by its mtime, but the file carries no capture date"
        );
        assert_eq!(b.file_modified_ms, 1_600_000_000_000);
    }

    /// The position travels from the row to the panel, and to the one URL photon opens.
    #[test]
    fn viewer_item_and_the_map_url_report_the_position() {
        use photon_core::library::NewItem;
        use photon_core::metadata::{CameraMeta, Gps};
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        assert_eq!(viewer_item(&f.engine, id).unwrap().gps, None);
        assert_eq!(item_map_url(&f.engine, id).unwrap(), None);

        let row = f.engine.lib.item(id).unwrap().unwrap();
        let placed = NewItem {
            folder_id: row.folder_id,
            path: row.path.clone(),
            file_name: "a.jpg".into(),
            kind: row.kind,
            size: row.size,
            mtime_ms: row.mtime_ms,
            width: row.width,
            height: row.height,
            orientation: row.orientation,
            taken_at: row.taken_at,
            rating: None,
            camera: CameraMeta {
                gps: Some(Gps {
                    lat: -33.868_82,
                    lon: 151.209_3,
                }),
                ..CameraMeta::default()
            },
            tags: vec![],
            caption: None,
            duration_ms: None,
        };
        f.engine.lib.update_item_meta(&[(id, placed)]).unwrap();
        assert_eq!(
            viewer_item(&f.engine, id).unwrap().gps,
            Some(ItemGps {
                lat: -33.868_82,
                lon: 151.209_3
            })
        );
        assert_eq!(
            item_map_url(&f.engine, id).unwrap().as_deref(),
            Some(
                "https://www.openstreetmap.org/?mlat=-33.868820&mlon=151.209300#map=16/-33.868820/151.209300"
            )
        );
        assert!(item_map_url(&f.engine, id + 999).is_err());
    }

    /// The histogram is of the thumbnail on disk: absent until there is one, then one count
    /// per pixel of it.
    #[test]
    fn viewer_item_reports_the_histogram_once_the_thumbnail_exists() {
        let img = jpeg(64, 32);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        f.engine.thumbs.wait_idle();
        let item = f.engine.lib.item(id).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Ready);
        let bins = viewer_item(&f.engine, id)
            .unwrap()
            .histogram
            .expect("a cached thumbnail has a histogram");
        assert_eq!(bins.len(), photon_core::histogram::BINS);
        assert_eq!(bins.iter().sum::<u32>(), 64 * 32);

        // No thumbnail under the photo's key, no histogram - and no error either.
        let cached = f
            .engine
            .thumbs
            .path_for(item.thumb_key(), photon_core::thumbs::ThumbSize::Grid);
        std::fs::remove_file(cached).unwrap();
        assert_eq!(viewer_item(&f.engine, id).unwrap().histogram, None);
    }

    #[test]
    fn viewer_item_reports_the_caption() {
        use photon_core::library::NewItem;
        use photon_core::media::MediaKind;
        use photon_core::metadata::CameraMeta;
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        assert_eq!(viewer_item(&f.engine, id).unwrap().caption, None);
        let row = f.engine.lib.item(id).unwrap().unwrap();
        let described = NewItem {
            folder_id: row.folder_id,
            path: row.path.clone(),
            file_name: "a.jpg".into(),
            kind: MediaKind::Image,
            size: row.size,
            mtime_ms: row.mtime_ms,
            width: row.width,
            height: row.height,
            orientation: row.orientation,
            taken_at: row.taken_at,
            rating: None,
            camera: CameraMeta {
                make: None,
                model: None,
                lens: None,
                focal_mm: None,
                aperture: None,
                exposure_s: None,
                iso: None,
                gps: None,
            },
            tags: vec![],
            caption: Some("Grandma".into()),
            duration_ms: None,
        };
        f.engine.lib.update_item_meta(&[(id, described)]).unwrap();
        assert_eq!(
            viewer_item(&f.engine, id).unwrap().caption.as_deref(),
            Some("Grandma")
        );
    }

    /// The info panel tells a byte-identical twin from a look-alike, and lists the twins
    /// first: `orig` and `identical` are the same bytes (`jpeg_pattern` is deterministic in
    /// its inputs, so calling it twice with the same size produces the same file), while
    /// `resized` is the same picture at a different size and different bytes - a look-alike,
    /// not a copy. The second scan and `wait_idle` are load-bearing the way
    /// `a_scan_finds_the_look_alikes_it_indexed` (`engine.rs`) explains: the look-alike pass
    /// hashes cached thumbnails, and the first scan's pass can run before the thumbnail
    /// workers have caught up.
    #[test]
    fn the_viewer_lists_identical_copies_before_look_alikes() {
        use crate::testutil::jpeg_pattern;
        let f = fixture(&[
            ("a/orig.jpg", &jpeg_pattern(180, 120)),
            ("a/identical.jpg", &jpeg_pattern(180, 120)),
            ("a/resized.jpg", &jpeg_pattern(72, 48)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.settle();

        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let orig_id = f
            .ids()
            .into_iter()
            .find(|&id| path_of(id).ends_with("orig.jpg"))
            .unwrap();

        let item = viewer_item(&f.engine, orig_id).unwrap();
        assert_eq!(item.copies.len(), 2, "expected one twin and one look-alike");
        assert!(item.copies[0].path.ends_with("identical.jpg"));
        assert!(matches!(item.copies[0].kind, CopyKind::Identical));
        assert!(item.copies[1].path.ends_with("resized.jpg"));
        assert!(matches!(item.copies[1].kind, CopyKind::Similar));
        assert_eq!((item.copies[1].width, item.copies[1].height), (72, 48));
    }

    /// The menu's count is the info panel's list, counted: `identical.jpg` is both the same
    /// bytes and the same picture as `orig.jpg`, and is one copy, not two. Same fixture and
    /// the same reason for the second scan as the test above.
    #[test]
    fn the_copy_count_is_the_info_panels_list_counted_once() {
        use crate::testutil::jpeg_pattern;
        let f = fixture(&[
            ("a/orig.jpg", &jpeg_pattern(180, 120)),
            ("a/identical.jpg", &jpeg_pattern(180, 120)),
            ("a/resized.jpg", &jpeg_pattern(72, 48)),
            ("a/unrelated.jpg", &jpeg(64, 64)),
        ]);
        let watched = f.add_photos();
        f.engine.thumbs.wait_idle();
        f.engine.start_scan(watched);
        f.settle();
        let path_of = |id: i64| f.engine.lib.item(id).unwrap().unwrap().path;
        let id_of = |name: &str| {
            f.ids()
                .into_iter()
                .find(|&id| path_of(id).ends_with(name))
                .unwrap()
        };

        let orig = id_of("orig.jpg");
        assert_eq!(copy_count(&f.engine, orig).unwrap(), 2);
        assert_eq!(
            copy_count(&f.engine, orig).unwrap(),
            viewer_item(&f.engine, orig).unwrap().copies.len()
        );
        assert_eq!(copy_count(&f.engine, id_of("unrelated.jpg")).unwrap(), 0);
    }

    /// Gives a scanned photo keywords the way the scanner's metadata backfill writes them.
    #[test]
    fn an_edited_neighbour_is_not_offered_for_a_full_size_preload() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img), ("c.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        assert_eq!(neighbours(&f.engine, ids[1], 1).len(), 2);
        rotate_item(&f.engine, ids[2], true).unwrap();
        assert_eq!(neighbours(&f.engine, ids[1], 1), vec![ids[0]]);
    }

    /// The viewer gets Picasa's named faces as before, and beside them every face without
    /// a name: Picasa's unnamed ones and the detections that are none of Picasa's.
    #[test]
    fn viewer_item_carries_unnamed_faces() {
        let f = fixture(&[
            ("a/a.jpg", &jpeg(400, 300)),
            (
                "a/.picasa.ini",
                // Ada at the left, and a face whose contact no INI names at the right.
                b"[Contacts2]\nabc=Ada\n[a.jpg]\nfaces=rect64(1000200030006000),abc;rect64(c0002000f0006000),zzz\n",
            ),
        ]);
        f.add_photos();
        f.engine.thumbs.wait_idle();
        let id = f.ids()[0];
        f.engine.lib.set_face_detection(true).unwrap();
        f.engine
            .lib
            .set_thumb_state(id, ThumbState::Ready, None)
            .unwrap();
        let listed = f
            .engine
            .lib
            .face_candidates(0, 10, DETECTOR_VERSION)
            .unwrap();
        let at = |left: f64| Detection {
            rect: Rect {
                left,
                top: 0.2,
                right: left + 0.1,
                bottom: 0.3,
            },
            landmarks: [(0.0, 0.0); 5],
            score: 0.9,
        };
        // One over Ada (0.0625..0.1875 wide), one over the unnamed face (0.75..0.9375),
        // one in the middle that is nobody Picasa knew.
        f.engine
            .lib
            .write_face_batch(
                &[(listed[0].clone(), vec![at(0.08), at(0.80), at(0.45)])],
                DETECTOR_VERSION,
            )
            .unwrap();

        let item = viewer_item(&f.engine, id).unwrap();
        assert_eq!(item.faces.len(), 1);
        assert_eq!(item.faces[0].name, "Ada");
        assert_eq!(item.unnamed_faces.len(), 2, "{:?}", item.unnamed_faces);
        // Picasa's unnamed face first, then the detection that matched nothing.
        assert!((item.unnamed_faces[0].left - 0.75).abs() < 0.001);
        assert!((item.unnamed_faces[1].left - 0.45).abs() < 0.001);
    }

    /// A person's confirmed faces are named in the viewer; a contact linked to a person is
    /// that person, under the person's name rather than Picasa's, shown once where Picasa
    /// and photon both found the face; a suggestion is still an unnamed outline.
    #[test]
    fn viewer_item_names_confirmed_faces_and_shows_a_linked_contact_once() {
        let f = fixture(&[
            ("a/a.jpg", &jpeg(400, 300)),
            (
                "a/.picasa.ini",
                b"[Contacts2]\nabc=Ada L.\n[a.jpg]\nfaces=rect64(1000200030006000),abc\n",
            ),
        ]);
        f.add_photos();
        f.engine.thumbs.wait_idle();
        let id = f.ids()[0];
        f.engine.lib.set_face_detection(true).unwrap();
        f.engine
            .lib
            .set_thumb_state(id, ThumbState::Ready, None)
            .unwrap();
        let listed = f
            .engine
            .lib
            .face_candidates(0, 10, DETECTOR_VERSION)
            .unwrap();
        let at = |left: f64| Detection {
            rect: Rect {
                left,
                top: 0.2,
                right: left + 0.1,
                bottom: 0.3,
            },
            landmarks: [(0.0, 0.0); 5],
            score: 0.9,
        };
        f.engine
            .lib
            .write_face_batch(
                &[(listed[0].clone(), vec![at(0.08), at(0.45), at(0.80)])],
                DETECTOR_VERSION,
            )
            .unwrap();
        // Ada is linked to Picasa's contact (whom Picasa calls "Ada L.") and confirmed on the
        // detection over it; Bea has a confirmed face in the middle and only a suggestion at
        // the right.
        let w = rusqlite::Connection::open(&f.config().db_path).unwrap();
        w.execute(
            "INSERT INTO people (id, name) VALUES (1, 'Ada'), (2, 'Bea')",
            [],
        )
        .unwrap();
        w.execute("INSERT INTO person_contacts VALUES ('abc', 1)", [])
            .unwrap();
        for (left, person, confirmed) in [(0.08, 1, 1), (0.45, 2, 1), (0.80, 2, 0)] {
            w.execute(
                "UPDATE detected_faces SET person_id = ?2, confirmed = ?3 WHERE left = ?1",
                rusqlite::params![left, person, confirmed],
            )
            .unwrap();
        }
        drop(w);

        let item = viewer_item(&f.engine, id).unwrap();
        let named: Vec<(&str, &str)> = item
            .faces
            .iter()
            .map(|f| (f.key.as_str(), f.name.as_str()))
            .collect();
        assert_eq!(
            named,
            vec![("p:1", "Ada"), ("p:2", "Bea")],
            "Ada once, under her key and the name the user gave her, not also as the \
             detection over Picasa's rectangle"
        );
        assert!((item.faces[0].left - 0.0625).abs() < 0.001, "Picasa's box");
        assert_eq!(item.unnamed_faces.len(), 1, "{:?}", item.unnamed_faces);
        assert!(
            (item.unnamed_faces[0].left - 0.80).abs() < 0.001,
            "only the suggestion is an outline"
        );
    }

    /// A confirmed face over a Picasa face no INI names is one face with a name plate, not
    /// a plate with an unnamed outline drawn on the same place.
    #[test]
    fn a_named_detection_over_an_unnamed_picasa_face_is_drawn_once() {
        let f = fixture(&[
            ("a/a.jpg", &jpeg(400, 300)),
            (
                "a/.picasa.ini",
                b"[a.jpg]\nfaces=rect64(1000200030006000),zzz\n",
            ),
        ]);
        f.add_photos();
        f.engine.thumbs.wait_idle();
        let id = f.ids()[0];
        f.engine.lib.set_face_detection(true).unwrap();
        f.engine
            .lib
            .set_thumb_state(id, ThumbState::Ready, None)
            .unwrap();
        let listed = f
            .engine
            .lib
            .face_candidates(0, 10, DETECTOR_VERSION)
            .unwrap();
        let det = Detection {
            rect: Rect {
                left: 0.08,
                top: 0.2,
                right: 0.18,
                bottom: 0.3,
            },
            landmarks: [(0.0, 0.0); 5],
            score: 0.9,
        };
        f.engine
            .lib
            .write_face_batch(&[(listed[0].clone(), vec![det])], DETECTOR_VERSION)
            .unwrap();
        let w = rusqlite::Connection::open(&f.config().db_path).unwrap();
        w.execute("INSERT INTO people (id, name) VALUES (1, 'Ada')", [])
            .unwrap();
        w.execute("UPDATE detected_faces SET person_id = 1, confirmed = 1", [])
            .unwrap();
        drop(w);

        let item = viewer_item(&f.engine, id).unwrap();
        assert_eq!(item.faces.len(), 1, "{:?}", item.faces);
        assert_eq!(item.faces[0].key, "p:1");
        assert!(
            item.unnamed_faces.is_empty(),
            "no outline under the plate: {:?}",
            item.unnamed_faces
        );
    }

    /// A photo with a Picasa INI and detections (0.1 wide, at the given lefts, in face order).
    fn photo_with_detections(ini: &[u8], lefts: &[f64]) -> (Fixture, i64) {
        let f = fixture(&[("a/a.jpg", &jpeg(400, 300)), ("a/.picasa.ini", ini)]);
        f.add_photos();
        f.engine.thumbs.wait_idle();
        let id = f.ids()[0];
        f.engine.lib.set_face_detection(true).unwrap();
        f.engine
            .lib
            .set_thumb_state(id, ThumbState::Ready, None)
            .unwrap();
        let listed = f
            .engine
            .lib
            .face_candidates(0, 10, DETECTOR_VERSION)
            .unwrap();
        let dets = lefts
            .iter()
            .map(|&left| Detection {
                rect: Rect {
                    left,
                    top: 0.2,
                    right: left + 0.1,
                    bottom: 0.3,
                },
                landmarks: [(0.0, 0.0); 5],
                score: 0.9,
            })
            .collect();
        f.engine
            .lib
            .write_face_batch(&[(listed[0].clone(), dets)], DETECTOR_VERSION)
            .unwrap();
        (f, id)
    }

    /// Run SQL against the library (people, links, confirmations).
    fn sql(f: &Fixture, statements: &str) {
        let w = rusqlite::Connection::open(&f.config().db_path).unwrap();
        w.execute_batch(statements).unwrap();
    }

    /// The `detected_faces.id` of the detection starting at `left`.
    fn face_at(f: &Fixture, left: f64) -> i64 {
        let w = rusqlite::Connection::open(&f.config().db_path).unwrap();
        w.query_row(
            "SELECT id FROM detected_faces WHERE left = ?1",
            [left],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn a_detections_plate_carries_its_face_id() {
        let (f, id) = photo_with_detections(b"", &[0.2, 0.6]);
        sql(
            &f,
            "INSERT INTO people (id, name) VALUES (1, 'Ada');
             UPDATE detected_faces SET person_id = 1, confirmed = 1 WHERE left = 0.6;",
        );
        let item = viewer_item(&f.engine, id).unwrap();
        assert_eq!(item.faces.len(), 1);
        assert_eq!(item.faces[0].face_id, Some(face_at(&f, 0.6)));
    }

    #[test]
    fn a_linked_picasa_plate_carries_the_detection_beneath() {
        let (f, id) = photo_with_detections(
            b"[Contacts2]\nabc=Ada L.\n[a.jpg]\nfaces=rect64(1000200030006000),abc\n",
            &[0.08],
        );
        sql(
            &f,
            "INSERT INTO people (id, name) VALUES (1, 'Ada'), (2, 'Bea');
             INSERT INTO person_contacts VALUES ('abc', 1);
             UPDATE detected_faces SET person_id = 1, confirmed = 1;",
        );
        let item = viewer_item(&f.engine, id).unwrap();
        assert_eq!(item.faces.len(), 1);
        assert_eq!(item.faces[0].key, "p:1");
        assert_eq!(item.faces[0].face_id, Some(face_at(&f, 0.08)));
        // The detection under the plate is confirmed as somebody else: not the plate's face.
        sql(&f, "UPDATE detected_faces SET person_id = 2;");
        let item = viewer_item(&f.engine, id).unwrap();
        let ada = item.faces.iter().find(|p| p.key == "p:1").unwrap();
        assert_eq!(ada.face_id, None);
    }

    #[test]
    fn an_unlinked_contacts_plate_has_no_face_id() {
        let (f, id) = photo_with_detections(
            b"[Contacts2]\nabc=Ada L.\n[a.jpg]\nfaces=rect64(1000200030006000),abc\n",
            &[0.08],
        );
        sql(
            &f,
            "INSERT INTO people (id, name) VALUES (1, 'Ada');
             UPDATE detected_faces SET person_id = 1, confirmed = 1;",
        );
        let item = viewer_item(&f.engine, id).unwrap();
        assert_eq!(item.faces[0].key, "c:abc");
        assert_eq!(item.faces[0].face_id, None);
    }

    #[test]
    fn an_unnamed_outline_carries_its_face_id() {
        // Picasa's unnamed face at 0.75 with a detection beneath, another with none
        // beneath (at 0.20), and a detection that is none of Picasa's (0.45).
        let (f, id) = photo_with_detections(
            b"[a.jpg]\nfaces=rect64(c0002000f0006000),zzz;rect64(2000200030006000),yyy\n",
            &[0.80, 0.45],
        );
        let item = viewer_item(&f.engine, id).unwrap();
        let by_left = |left: f64| {
            item.unnamed_faces
                .iter()
                .find(|u| (u.left - left).abs() < 0.001)
                .unwrap_or_else(|| panic!("{left}: {:?}", item.unnamed_faces))
        };
        assert_eq!(by_left(0.75).face_id, Some(face_at(&f, 0.80)));
        assert_eq!(by_left(0.125).face_id, None);
        assert_eq!(by_left(0.45).face_id, Some(face_at(&f, 0.45)));
        assert_eq!(item.unnamed_faces.len(), 3);
    }

    /// Two Picasa faces on one spot, one named and one not, and two detections on it, the
    /// first confirmed as a person: naming the unnamed outline must act on the detection
    /// nobody is confirmed on, not on the named one.
    #[test]
    fn an_unnamed_outline_skips_the_detection_a_person_is_confirmed_on() {
        let (f, id) = photo_with_detections(
            b"[Contacts2]\nabc=Ada L.\n[a.jpg]\nfaces=rect64(1000200030006000),abc;rect64(1000200030006000),zzz\n",
            &[0.08, 0.09],
        );
        sql(
            &f,
            "INSERT INTO people (id, name) VALUES (1, 'Ada');
             UPDATE detected_faces SET person_id = 1, confirmed = 1 WHERE left = 0.08;",
        );
        let item = viewer_item(&f.engine, id).unwrap();
        let outline = item
            .unnamed_faces
            .iter()
            .find(|u| (u.left - 0.0625).abs() < 0.001)
            .unwrap_or_else(|| panic!("{:?}", item.unnamed_faces));
        assert_eq!(outline.face_id, Some(face_at(&f, 0.09)));
    }

    /// An edit maps Picasa's unnamed face like its named ones; a detection is already in
    /// the picture as shown and is not mapped again.
    #[test]
    fn unnamed_faces_follow_the_edit() {
        let f = fixture(&[
            ("a/a.jpg", &jpeg(400, 300)),
            (
                "a/.picasa.ini",
                b"[a.jpg]\nfaces=rect64(1000200030006000),zzz\n",
            ),
        ]);
        f.add_photos();
        let id = f.ids()[0];
        let before = viewer_item(&f.engine, id).unwrap().unnamed_faces;
        rotate_item(&f.engine, id, true).unwrap();
        let after = viewer_item(&f.engine, id).unwrap().unnamed_faces;
        assert_eq!((before.len(), after.len()), (1, 1));
        // A quarter turn clockwise sends (l, t, r, b) to (1 - b, l, 1 - t, r).
        assert!((after[0].left - (1.0 - before[0].bottom)).abs() < 0.001);
        assert!((after[0].top - before[0].left).abs() < 0.001);
    }

    #[test]
    fn an_edited_photo_is_described_as_it_is_shown() {
        // A 40x20 photo with a face centred at (0.375, 0.25). What the viewer is told has
        // to match the picture the URLs serve, which has the edit rendered into it.
        let img = jpeg(40, 20);
        let f = fixture(&[("a.jpg", &img)]);
        std::fs::write(
            f.photos.join(".picasa.ini"),
            b"[Contacts2]\nabc=Ada\n[a.jpg]\nfaces=rect64(4000200080006000),abc\n",
        )
        .unwrap();
        f.add_photos();
        let id = f.ids()[0];
        let plain = viewer_item(&f.engine, id).unwrap();
        assert_eq!(
            (plain.width, plain.height, plain.edit.is_none()),
            (40, 20, true)
        );

        rotate_item(&f.engine, id, true).unwrap();
        let turned = viewer_item(&f.engine, id).unwrap();
        assert_eq!(
            (turned.width, turned.height, turned.orientation),
            (20, 40, 1)
        );
        assert_eq!(
            turned.edit,
            Some(ItemEdit {
                turns: 1,
                crop: None
            })
        );
        assert_ne!(turned.thumb_key, plain.thumb_key);
        // Clockwise, the face's left edge becomes its top and its bottom its left.
        let face = &turned.faces[0];
        assert!((face.top - 0.25).abs() < 1e-3 && (face.left - 0.625).abs() < 1e-3);

        // The right half of the unturned photo: the face is in the left half, so it goes.
        set_item_edit(&f.engine, id, 0, Some([32768, 0, 65535, 65535])).unwrap();
        let cropped = viewer_item(&f.engine, id).unwrap();
        assert_eq!((cropped.width, cropped.height), (20, 20));
        assert_eq!(
            (cropped.uncropped_width, cropped.uncropped_height),
            (40, 20)
        );
        assert_eq!((turned.uncropped_width, turned.uncropped_height), (20, 40));
        assert!(cropped.faces.is_empty());

        let err = set_item_edit(&f.engine, id, 0, Some([40000, 0, 30000, 65535])).unwrap_err();
        assert_eq!(err.kind, "invalidCrop");

        set_item_edit(&f.engine, id, 0, None).unwrap();
        let reset = viewer_item(&f.engine, id).unwrap();
        assert_eq!(reset.edit, None);
        assert_eq!(
            reset.thumb_key, plain.thumb_key,
            "the cached original is reused"
        );
        assert_eq!(reset.faces.len(), 1);
    }

    fn set_keywords(f: &crate::testutil::Fixture, id: i64, tags: &[&str]) {
        use photon_core::library::NewItem;
        let row = f.engine.lib.item(id).unwrap().unwrap();
        let file_name = Path::new(&row.path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let described = NewItem {
            folder_id: row.folder_id,
            path: row.path.clone(),
            file_name,
            kind: row.kind,
            size: row.size,
            mtime_ms: row.mtime_ms,
            width: row.width,
            height: row.height,
            orientation: row.orientation,
            taken_at: row.taken_at,
            rating: None,
            camera: Default::default(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            caption: None,
            duration_ms: None,
        };
        f.engine.lib.update_item_meta(&[(id, described)]).unwrap();
    }

    /// The people commands' refusals reach the UI under kinds of their own.
    #[test]
    fn people_errors_have_their_own_kinds() {
        let f = fixture(&[]);
        assert_eq!(
            name_person(&f.engine, 1, "  ").unwrap_err().kind,
            "emptyPersonName"
        );
        assert_eq!(
            merge_people(&f.engine, 9_999, 9_998).unwrap_err().kind,
            "notAPerson"
        );
        assert_eq!(
            remove_from_person(&f.engine, 9_999, &[1]).unwrap_err().kind,
            "notAPerson"
        );
    }

    /// Renaming the tag on screen must carry the view with it: left on the old name, the
    /// view would show a keyword that now answers to nothing, and the grid would empty.
    #[test]
    fn renaming_the_viewed_tag_keeps_the_view_on_it() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        set_keywords(&f, ids[0], &["holiday"]);
        set_tag_view(&f.engine, "holiday").unwrap();
        assert_eq!(grid_info(&f.engine, None).len, 1);

        assert_eq!(
            rename_tag(&f.engine, "holiday", " ").unwrap_err().kind,
            "emptyTagName"
        );
        rename_tag(&f.engine, "holiday", " vacation").unwrap();
        let info = grid_info(&f.engine, None);
        assert_eq!(info.tag.as_deref(), Some("vacation"));
        assert_eq!(info.len, 1);
        assert_eq!(
            list_tag_rules(&f.engine).unwrap(),
            [photon_core::library::TagRule {
                tag: "holiday".into(),
                target: Some("vacation".into()),
            }]
        );

        hide_tag(&f.engine, "vacation").unwrap();
        assert_eq!(
            grid_info(&f.engine, None).len,
            0,
            "the removed tag's view empties"
        );
        assert!(list_tags(&f.engine).unwrap().is_empty());

        restore_tag_rule(&f.engine, "holiday").unwrap();
        assert_eq!(list_tags(&f.engine).unwrap()[0].tag, "holiday");
    }

    #[test]
    fn album_commands_round_trip_and_refresh_the_open_album() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let album = create_album(&f.engine, "Trip").unwrap();
        assert_eq!(
            create_album(&f.engine, "  ").unwrap_err().kind,
            "emptyAlbumName"
        );
        add_to_album(&f.engine, album.id, &ids[..1]).unwrap();
        set_album_view(&f.engine, album.id).unwrap();
        assert_eq!(grid_info(&f.engine, None).len, 1);

        add_to_album(&f.engine, album.id, &ids[1..]).unwrap();
        assert_eq!(
            grid_info(&f.engine, None).len,
            2,
            "the open album follows the add"
        );
        remove_from_album(&f.engine, album.id, &ids).unwrap();
        assert_eq!(grid_info(&f.engine, None).len, 0);

        rename_album(&f.engine, album.id, "Zoo").unwrap();
        assert_eq!(list_albums(&f.engine).unwrap()[0].name, "Zoo");
        delete_album(&f.engine, album.id).unwrap();
        assert!(list_albums(&f.engine).unwrap().is_empty());
        assert_eq!(
            delete_album(&f.engine, album.id).unwrap_err().kind,
            "notFound"
        );
    }

    #[test]
    fn grid_rows_are_capped_and_versioned() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let rows = grid_rows(&f.engine, 0, 5_000);
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.version, grid_info(&f.engine, None).version);
        assert_eq!(clamp_count(5_000), MAX_ROWS);
        assert!(grid_rows(&f.engine, 10, 5).rows.is_empty());
    }

    #[test]
    fn viewer_item_and_neighbours() {
        let img = jpeg(32, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img), ("c.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let item = viewer_item(&f.engine, ids[1]).unwrap();
        assert_eq!(
            (item.file_name.as_str(), item.width, item.height),
            ("b.jpg", 32, 16)
        );
        // The caption shows the file's size; it comes from the indexed row, not a stat.
        assert_eq!(item.size, img.len() as i64);
        assert_eq!(item.thumb_key.len(), 16);
        assert!(matches!(item.thumb_state, "pending" | "ready"));
        assert_eq!(neighbours(&f.engine, ids[1], 50), vec![ids[2], ids[0]]);
        assert_eq!(viewer_item(&f.engine, 9_999).unwrap_err().kind, "notFound");
    }

    #[test]
    fn errors_carry_a_kind_for_the_ui() {
        let f = fixture(&[]);
        f.add_photos();
        let nested = f.photos.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let err = add_folder(&f.engine, nested.to_str().unwrap()).unwrap_err();
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "folderOverlap");
        assert!(json["message"].as_str().unwrap().contains("photos"));
        assert_eq!(
            rescan_folder(&f.engine, 9_999).unwrap_err().kind,
            "notFound"
        );
    }

    #[test]
    fn remove_and_paths() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        let id = f.ids()[0];
        assert!(item_path(&f.engine, id).unwrap().ends_with("a.jpg"));
        let root = list_folders(&f.engine).unwrap().folders[0].id;
        assert_eq!(
            folder_path(&f.engine, root).unwrap(),
            std::path::PathBuf::from(&watched.path)
        );
        remove_folder(&f.engine, watched.id).unwrap();
        assert_eq!(grid_info(&f.engine, None).len, 0);
    }

    /// The command layer's own wiring: a saved search round-trips, its errors reach the UI
    /// as kinds rather than as `internal`, and - unlike deleting an album - deleting one
    /// leaves the grid showing the photos the query still answers for.
    #[test]
    fn saved_searches_round_trip_and_deleting_one_leaves_the_grid_alone() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();

        let saved = save_search(&f.engine, "  Canon  ", "a").unwrap();
        assert_eq!((saved.name.as_str(), saved.query.as_str()), ("Canon", "a"));
        assert_eq!(
            save_search(&f.engine, "  ", "a").unwrap_err().kind,
            "emptySearchName"
        );
        assert_eq!(
            save_search(&f.engine, "Canon", "  ").unwrap_err().kind,
            "emptySearchQuery"
        );
        assert_eq!(
            list_saved_searches(&f.engine)
                .unwrap()
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>(),
            ["Canon"],
            "the two refusals wrote nothing"
        );

        rename_saved_search(&f.engine, saved.id, "Dad's").unwrap();
        assert_eq!(list_saved_searches(&f.engine).unwrap()[0].name, "Dad's");

        // The search view is driven by the query string, not by the saved row.
        set_search_query(&f.engine, "a").unwrap();
        let before = grid_info(&f.engine, None).len;
        assert_eq!(before, 1, "the query matches a.jpg only");
        delete_saved_search(&f.engine, saved.id).unwrap();
        assert!(list_saved_searches(&f.engine).unwrap().is_empty());
        assert_eq!(
            grid_info(&f.engine, None).len,
            before,
            "deleting the bookmark leaves the photos it was pointing at on screen"
        );
    }

    #[test]
    fn a_picasa_album_reaches_the_sidebar_the_viewer_and_its_view() {
        use photon_core::grid::GridView;
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("a/two.jpg", &img)]);
        std::fs::write(
            f.photos.join("a/.picasa.ini"),
            b"[.album:t]\nname=Holiday\n[one.jpg]\nalbums=t\n",
        )
        .unwrap();
        f.add_photos();

        let album = list_albums(&f.engine)
            .unwrap()
            .into_iter()
            .find(|a| a.picasa)
            .expect("the scan imported the album");
        assert_eq!((album.name.as_str(), album.count), ("Holiday", 1));

        let one = f
            .ids()
            .into_iter()
            .find(|&id| {
                f.engine
                    .lib
                    .item(id)
                    .unwrap()
                    .unwrap()
                    .path
                    .ends_with("one.jpg")
            })
            .unwrap();
        assert_eq!(viewer_item(&f.engine, one).unwrap().albums, vec![album.id]);

        set_album_view(&f.engine, album.id).unwrap();
        let info = grid_info(&f.engine, None);
        assert_eq!(
            (info.view, info.album, info.len),
            (GridView::Album, Some(album.id), 1)
        );
        assert!(
            add_to_album(&f.engine, album.id, &f.ids()).is_err(),
            "the guard holds over IPC"
        );
    }

    #[test]
    fn a_video_refuses_edits_and_the_clipboard() {
        let f = fixture(&[("clip.mp4", b"video")]);
        f.add_photos();
        let id = f.ids()[0];
        let refused = |r: CmdResult<()>| {
            matches!(
                r,
                Err(AppError {
                    kind: "notAPhoto",
                    ..
                })
            )
        };
        assert!(refused(rotate_item(&f.engine, id, true)));
        assert!(refused(set_item_edit(&f.engine, id, 1, None)));
        assert!(matches!(
            copy_picture(&f.engine, id),
            Err(AppError {
                kind: "notAPhoto",
                ..
            })
        ));
    }

    #[test]
    fn the_viewer_is_told_it_is_a_video_and_neighbours_leave_it_out() {
        let f = fixture(&[
            ("a.jpg", &jpeg(8, 8)),
            ("b.mp4", b"video"),
            ("c.jpg", &jpeg(8, 8)),
        ]);
        f.add_photos();
        let video = f
            .ids()
            .into_iter()
            .find(|&id| f.engine.lib.item(id).unwrap().unwrap().kind == MediaKind::Video)
            .unwrap();
        let item = viewer_item(&f.engine, video).unwrap();
        assert_eq!(item.kind, MediaKind::Video);
        assert!(!neighbours(&f.engine, f.ids()[0], 2).contains(&video));
    }

    /// A video has a poster in the cache like any photo's thumbnail, and still no
    /// histogram: one frame says nothing of the rest.
    #[test]
    fn a_video_has_no_histogram_though_its_poster_is_cached() {
        let f = fixture(&[("clip.mp4", b"video")]);
        f.add_photos();
        video_session_start(&f.engine, true).unwrap();
        let job = next_video_job(&f.engine, Duration::from_millis(200))
            .unwrap()
            .unwrap();
        put_video_frame(&f.engine, job.id, &job.key, &jpeg(32, 16)).unwrap();
        let item = f.engine.lib.item(job.id).unwrap().unwrap();
        assert!(
            f.engine.thumbs.histogram(&item).is_some(),
            "the poster is cached, so only the kind keeps the histogram out"
        );
        assert_eq!(viewer_item(&f.engine, job.id).unwrap().histogram, None);
    }

    #[test]
    fn next_video_job_speaks_the_ui_key() {
        let f = fixture(&[("clip.mp4", b"video")]);
        f.add_photos();
        video_session_start(&f.engine, true).unwrap();
        let job = next_video_job(&f.engine, Duration::from_millis(200))
            .unwrap()
            .unwrap();
        assert_eq!(job.key, viewer_item(&f.engine, job.id).unwrap().thumb_key);
    }

    /// The viewer must never open a video the crash guard failed, and must still offer one
    /// whose poster merely failed to decode: two Failed rows, told apart only by why.
    #[test]
    fn the_viewer_is_told_which_failed_video_crashed_the_window() {
        let f = fixture(&[("a.mp4", b"video a"), ("b.mp4", b"video b")]);
        f.add_photos();
        let ids = f.ids();
        // The page claims both and dies with them open, twice: the guard's own path.
        for _ in 0..2 {
            video_session_start(&f.engine, true).unwrap();
            let mut claimed = 0;
            while next_video_job(&f.engine, Duration::from_millis(100))
                .unwrap()
                .is_some()
            {
                claimed += 1;
            }
            assert_eq!(claimed, 2);
        }
        video_session_start(&f.engine, true).unwrap();
        assert!(
            next_video_job(&f.engine, Duration::from_millis(100))
                .unwrap()
                .is_none()
        );
        let crashed = viewer_item(&f.engine, ids[0]).unwrap();
        assert_eq!(crashed.thumb_state, "failed");
        assert!(crashed.video_crashed);

        let g = fixture(&[("c.mp4", b"video c")]);
        g.add_photos();
        video_session_start(&g.engine, true).unwrap();
        let job = next_video_job(&g.engine, Duration::from_millis(200))
            .unwrap()
            .unwrap();
        video_frame_failed(
            &g.engine,
            job.id,
            &job.key,
            photon_core::thumbs::VideoFailure::Decode,
        )
        .unwrap();
        let broken = viewer_item(&g.engine, job.id).unwrap();
        assert_eq!(broken.thumb_state, "failed");
        assert!(!broken.video_crashed);
    }

    /// A folder of videos is a burst of frames, one per video, and a grid rebuild per frame
    /// is a whole-library query each. They coalesce - but onto the trailing edge as well,
    /// since nothing else is coming to show the last one: the version must move again after
    /// the last `put` has returned.
    #[test]
    fn a_burst_of_poster_frames_rebuilds_the_grid_once_or_twice_and_the_last_counts() {
        use crate::events::Recorded;
        let names: Vec<String> = (0..6).map(|n| format!("clip{n}.mp4")).collect();
        let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"video"[..])).collect();
        let f = fixture(&files);
        f.add_photos();
        video_session_start(&f.engine, true).unwrap();
        let rebuilds = || {
            f.events
                .all()
                .iter()
                .filter(|e| matches!(e, Recorded::Library(_)))
                .count()
        };
        let before = rebuilds();
        let frame = jpeg(32, 18);
        let mut drawn = 0;
        while let Some(job) = next_video_job(&f.engine, Duration::from_millis(200)).unwrap() {
            put_video_frame(&f.engine, job.id, &job.key, &frame).unwrap();
            drawn += 1;
        }
        assert_eq!(drawn, names.len());
        let after_last_put = f.engine.grid().0;

        std::thread::sleep(Duration::from_millis(1_600));
        let rebuilt = rebuilds() - before;
        assert!(
            (1..=2).contains(&rebuilt),
            "{rebuilt} rebuilds for {drawn} frames"
        );
        assert!(
            f.engine.grid().0 > after_last_put,
            "the last frames of the burst were never shown"
        );
    }
}
