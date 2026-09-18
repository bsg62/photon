//! Command implementations as plain functions over `Engine`. `ipc.rs` exposes them to
//! the UI as Tauri commands.

use crate::{engine::Engine, error::AppError};
use photon_core::{
    Error,
    grid::{GridEntry, GridView, Section, hex_key},
    library::{
        Album, AlbumSummary, Folder, ItemFace, Person, TagCount, TagRule, WatchedFolder, is_starred,
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
    pub view: GridView,
    /// The query while `view` is `Search`, otherwise empty.
    pub search_query: String,
    /// The contact hash while `view` is `Person`.
    pub person: Option<String>,
    /// The album id while `view` is `Album`.
    pub album: Option<i64>,
    /// The keyword while `view` is `Tag`.
    pub tag: Option<String>,
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
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub focal_mm: Option<f64>,
    pub aperture: Option<f64>,
    pub exposure_s: Option<f64>,
    pub iso: Option<i64>,
    /// Keywords from the file's XMP and IPTC, in file order.
    pub tags: Vec<String>,
    /// Named Picasa faces, in INI order.
    pub faces: Vec<ItemFace>,
    /// Ids of the albums the photo is in.
    pub albums: Vec<i64>,
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

pub fn remove_folder(engine: &Engine, watched_id: i64) -> CmdResult<()> {
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
    GridInfo {
        version,
        len: grid.len(),
        sections: grid.sections().to_vec(),
        starred_count: engine.lib.starred_count().unwrap_or_else(|err| {
            tracing::warn!(%err, "starred count query failed");
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
    let faces = engine.lib.item_faces(item.id)?;
    let albums = engine.lib.item_albums(item.id)?;
    let thumb_key = hex_key(item.fingerprint());
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
        width: item.width,
        height: item.height,
        orientation: item.orientation,
        taken_at: item.taken_at,
        size: item.size,
        thumb_error: item.thumb_error,
        starred: is_starred(item.rating),
        path: item.path,
        make: camera.make,
        model: camera.model,
        lens: camera.lens,
        focal_mm: camera.focal_mm,
        aperture: camera.aperture,
        exposure_s: camera.exposure_s,
        iso: camera.iso,
        tags,
        faces,
        albums,
    })
}

pub fn set_star(engine: &Engine, id: i64, starred: bool) -> CmdResult<()> {
    engine.set_star(id, starred)?;
    Ok(())
}

pub fn add_item_tag(engine: &Engine, id: i64, tag: &str) -> CmdResult<String> {
    Ok(engine.add_item_tag(id, tag)?)
}

pub fn remove_item_tag(engine: &Engine, id: i64, tag: &str) -> CmdResult<()> {
    engine.remove_item_tag(id, tag)?;
    Ok(())
}

/// Items around `id`, nearest first, queued at neighbour priority so the viewer's
/// next and previous previews are ready early.
pub fn neighbours(engine: &Engine, id: i64, radius: usize) -> Vec<i64> {
    let ids = engine.grid().1.neighbours(id, radius.min(MAX_RADIUS));
    engine.thumbs.prioritize(&ids, Priority::Neighbour);
    ids
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

    /// Gives a scanned photo keywords the way the scanner's metadata backfill writes them.
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
}
