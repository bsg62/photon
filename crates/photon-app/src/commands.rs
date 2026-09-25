//! Command implementations as plain functions over `Engine`. `ipc.rs` exposes them to
//! the UI as Tauri commands.

use crate::{engine::Engine, error::AppError};
use photon_core::{
    Error,
    edit::{Crop, Edit},
    grid::{GridEntry, GridView, Section, hex_key},
    library::{
        Album, AlbumSummary, CopiesArg, Folder, GridTile, ItemFace, Person, SavedSearch, TagCount,
        TagRule, ThemeChoice, WatchedFolder, is_starred,
    },
    media::ThumbState,
    now_ms,
    thumbs::Priority,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
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
    pub sections: Vec<Section>,
    pub starred_count: usize,
    /// Photos with a byte-identical twin; the sidebar shows the Duplicates row only above 0.
    pub duplicate_count: usize,
    /// Hidden photos; the sidebar shows the Hidden row only above 0.
    pub hidden_count: usize,
    pub view: GridView,
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
    /// Named Picasa faces, in INI order.
    pub faces: Vec<ItemFace>,
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
    /// included. `faces` are likewise mapped into the edited frame, and a face whose centre
    /// was cropped away is left out.
    pub edit: Option<ItemEdit>,
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

pub fn app_info(engine: &Engine) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        library_path: engine.lib.path().to_path_buf(),
        licence: env!("CARGO_PKG_LICENSE"),
    }
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

pub fn grid_info(engine: &Engine) -> GridInfo {
    let (version, grid) = engine.grid();
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
    GridInfo {
        version,
        len: grid.len(),
        sections: grid.sections().to_vec(),
        starred_count: engine.lib.starred_count().unwrap_or_else(|err| {
            tracing::warn!(%err, "starred count query failed");
            0
        }),
        duplicate_count: engine.lib.duplicate_count().unwrap_or_else(|err| {
            tracing::warn!(%err, "duplicate count query failed");
            0
        }),
        hidden_count: engine.lib.hidden_count().unwrap_or_else(|err| {
            tracing::warn!(%err, "hidden count query failed");
            0
        }),
        view,
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
    }
}

pub fn set_person_view(engine: &Engine, contact: &str) -> CmdResult<()> {
    engine.set_person_view(contact)?;
    Ok(())
}

pub fn set_album_view(engine: &Engine, album_id: i64) -> CmdResult<()> {
    engine.set_album_view(album_id)?;
    Ok(())
}

pub fn set_tag_view(engine: &Engine, tag: &str) -> CmdResult<()> {
    engine.set_tag_view(tag)?;
    Ok(())
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

pub fn set_copies_view(engine: &Engine, id: i64) -> CmdResult<()> {
    engine.set_copies_view(id)?;
    Ok(())
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
    engine.albums_changed()?;
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
    engine.albums_changed()?;
    Ok(())
}

pub fn remove_from_album(engine: &Engine, album_id: i64, item_ids: &[i64]) -> CmdResult<()> {
    engine.lib.remove_from_album(album_id, item_ids)?;
    engine.albums_changed()?;
    Ok(())
}

pub fn set_grid_view(engine: &Engine, view: GridView) -> CmdResult<()> {
    engine.set_view(view)?;
    Ok(())
}

pub fn set_search_query(engine: &Engine, query: &str) -> CmdResult<()> {
    engine.set_search_query(query)?;
    Ok(())
}

pub fn grid_rows(engine: &Engine, offset: usize, count: usize) -> GridRows {
    let (version, grid) = engine.grid();
    GridRows {
        version,
        rows: grid.rows(offset, clamp_count(count)).to_vec(),
    }
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
    let faces = engine
        .lib
        .item_faces(item.id)?
        .into_iter()
        .filter_map(|face| {
            let (left, top, right, bottom) =
                edit.map_rect((face.left, face.top, face.right, face.bottom))?;
            Some(ItemFace {
                left,
                top,
                right,
                bottom,
                ..face
            })
        })
        .collect();
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
    let camera = item.camera;
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
        albums,
        copies,
        uncropped_width,
        uncropped_height,
        edit: (!edit.is_identity()).then(|| ItemEdit {
            turns: edit.turns,
            crop: edit.crop.map(|c| [c.left, c.top, c.right, c.bottom]),
        }),
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
) -> CmdResult<ExportReport> {
    let done = engine.export_items(ids, Path::new(dest), apply_edits)?;
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
/// result nothing ever reads. Their thumbnails are still queued.
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
                .is_some_and(|item| item.edit.is_identity())
        })
        .collect()
}

pub fn item_path(engine: &Engine, id: i64) -> CmdResult<PathBuf> {
    let item = engine.lib.item(id)?.ok_or(Error::NotFound(id))?;
    Ok(PathBuf::from(item.path))
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
    use crate::testutil::{fixture, jpeg};

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
        let info = grid_info(&f.engine);
        assert_eq!((info.len, info.sections.len()), (2, 2));
        let sub = list.folders.iter().find(|x| x.name == "sub").unwrap();
        assert_eq!(grid_offset_of_folder(&f.engine, sub.id), Some(1));
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
        f.engine.wait_for_scans();

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
            },
            tags: vec!["beach".into()],
            caption: None,
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
            },
            tags: vec![],
            caption: Some("Grandma".into()),
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
        f.engine.wait_for_scans();

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
        f.engine.wait_for_scans();
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
        };
        f.engine.lib.update_item_meta(&[(id, described)]).unwrap();
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
        assert_eq!(grid_info(&f.engine).len, 1);

        assert_eq!(
            rename_tag(&f.engine, "holiday", " ").unwrap_err().kind,
            "emptyTagName"
        );
        rename_tag(&f.engine, "holiday", " vacation").unwrap();
        let info = grid_info(&f.engine);
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
            grid_info(&f.engine).len,
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
        assert_eq!(grid_info(&f.engine).len, 1);

        add_to_album(&f.engine, album.id, &ids[1..]).unwrap();
        assert_eq!(
            grid_info(&f.engine).len,
            2,
            "the open album follows the add"
        );
        remove_from_album(&f.engine, album.id, &ids).unwrap();
        assert_eq!(grid_info(&f.engine).len, 0);

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
        assert_eq!(rows.version, grid_info(&f.engine).version);
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
        assert_eq!(grid_info(&f.engine).len, 0);
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
        let before = grid_info(&f.engine).len;
        assert_eq!(before, 1, "the query matches a.jpg only");
        delete_saved_search(&f.engine, saved.id).unwrap();
        assert!(list_saved_searches(&f.engine).unwrap().is_empty());
        assert_eq!(
            grid_info(&f.engine).len,
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
        let info = grid_info(&f.engine);
        assert_eq!(
            (info.view, info.album, info.len),
            (GridView::Album, Some(album.id), 1)
        );
        assert!(
            add_to_album(&f.engine, album.id, &f.ids()).is_err(),
            "the guard holds over IPC"
        );
    }
}
