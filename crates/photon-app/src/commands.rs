//! Command implementations as plain functions over `Engine`. `ipc.rs` exposes them to
//! the UI as Tauri commands.

use crate::{engine::Engine, error::AppError};
use photon_core::{
    Error,
    grid::{GridEntry, GridView, Section, hex_key},
    library::{Folder, WatchedFolder, is_starred},
    media::ThumbState,
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
    pub search_query: String,
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
    GridInfo {
        version,
        len: grid.len(),
        sections: grid.sections().to_vec(),
        starred_count: engine.lib.starred_count().unwrap_or_else(|err| {
            tracing::warn!(%err, "starred count query failed");
            0
        }),
        view: engine.view(),
        search_query: engine.search_query(),
    }
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
    Ok(ViewerItem {
        id: item.id,
        thumb_key: hex_key(item.fingerprint()),
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
    })
}

pub fn set_star(engine: &Engine, id: i64, starred: bool) -> CmdResult<()> {
    engine.set_star(id, starred)?;
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
