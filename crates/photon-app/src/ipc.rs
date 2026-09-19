//! Tauri command wrappers.
//!
//! Every command carries the `#[tauri::command(async)]` attribute, which never blocks
//! the UI thread: that thread only dispatches the request and returns. But for a plain
//! (non-`async`) function, that attribute runs the command body synchronously inside a
//! future spawned onto Tauri's small Tokio *worker* pool, not a dedicated blocking pool
//! (`tauri::async_runtime::spawn_blocking` is never called on this path) - so a command
//! that genuinely blocks would occupy one of those few worker threads for its whole
//! duration, and enough of them in flight would stall every other command's dispatch.
//!
//! Most commands here are short database or lock operations, so that's fine as plain
//! functions. `add_folder` (whose `dunce::canonicalize` can hang on a dead network
//! mount) and `remove_folder` (which joins a scan thread) can genuinely block for a
//! while, so they're `async fn`s that hand their blocking body to
//! `tauri::async_runtime::spawn_blocking`, which *does* run on Tauri's dedicated
//! blocking pool.

use crate::{commands, engine::Engine, error::AppError};
use photon_core::library::{Album, AlbumSummary, Person, TagCount, TagRule, WatchedFolder};
use std::sync::Arc;
use tauri::State;

type Eng<'a> = State<'a, Arc<Engine>>;

#[tauri::command(async)]
pub fn list_folders(engine: Eng<'_>) -> Result<commands::FolderList, AppError> {
    commands::list_folders(&engine)
}

#[tauri::command(async)]
pub async fn add_folder(engine: Eng<'_>, path: String) -> Result<WatchedFolder, AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || commands::add_folder(&engine, &path))
        .await
        .map_err(AppError::internal)?
}

#[tauri::command(async)]
pub async fn remove_folder(engine: Eng<'_>, watched_id: i64) -> Result<(), AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || commands::remove_folder(&engine, watched_id))
        .await
        .map_err(AppError::internal)?
}

#[tauri::command(async)]
pub fn rescan_folder(engine: Eng<'_>, watched_id: i64) -> Result<(), AppError> {
    commands::rescan_folder(engine.inner(), watched_id)
}

#[tauri::command(async)]
pub fn grid_info(engine: Eng<'_>) -> commands::GridInfo {
    commands::grid_info(&engine)
}

#[tauri::command(async)]
pub fn grid_rows(engine: Eng<'_>, offset: usize, count: usize) -> commands::GridRows {
    commands::grid_rows(&engine, offset, count)
}

#[tauri::command(async)]
pub fn grid_offset_of_folder(engine: Eng<'_>, folder_id: i64) -> Option<usize> {
    commands::grid_offset_of_folder(&engine, folder_id)
}

#[tauri::command(async)]
pub fn grid_offset_of_item(engine: Eng<'_>, item_id: i64) -> Option<usize> {
    commands::grid_offset_of_item(&engine, item_id)
}

#[tauri::command(async)]
pub fn last_folder(engine: Eng<'_>) -> Result<Option<i64>, AppError> {
    commands::last_folder(&engine)
}

#[tauri::command(async)]
pub fn set_last_folder(engine: Eng<'_>, folder_id: i64) -> Result<(), AppError> {
    commands::set_last_folder(&engine, folder_id)
}

#[tauri::command(async)]
pub fn slideshow_interval(engine: Eng<'_>) -> Result<i64, AppError> {
    commands::slideshow_interval(&engine)
}

#[tauri::command(async)]
pub fn set_slideshow_interval(engine: Eng<'_>, seconds: i64) -> Result<i64, AppError> {
    commands::set_slideshow_interval(&engine, seconds)
}

#[tauri::command(async)]
pub fn theme(engine: Eng<'_>) -> Result<photon_core::library::ThemeChoice, AppError> {
    commands::theme(&engine)
}

#[tauri::command(async)]
pub fn set_theme(
    engine: Eng<'_>,
    choice: photon_core::library::ThemeChoice,
) -> Result<(), AppError> {
    commands::set_theme(&engine, choice)
}

#[tauri::command(async)]
pub fn set_grid_view(engine: Eng<'_>, view: photon_core::grid::GridView) -> Result<(), AppError> {
    commands::set_grid_view(&engine, view)
}

#[tauri::command(async)]
pub fn set_search_query(engine: Eng<'_>, query: String) -> Result<(), AppError> {
    commands::set_search_query(&engine, &query)
}

#[tauri::command(async)]
pub fn set_person_view(engine: Eng<'_>, contact: String) -> Result<(), AppError> {
    commands::set_person_view(&engine, &contact)
}

#[tauri::command(async)]
pub fn set_album_view(engine: Eng<'_>, album_id: i64) -> Result<(), AppError> {
    commands::set_album_view(&engine, album_id)
}

#[tauri::command(async)]
pub fn set_tag_view(engine: Eng<'_>, tag: String) -> Result<(), AppError> {
    commands::set_tag_view(&engine, &tag)
}

#[tauri::command(async)]
pub fn list_people(engine: Eng<'_>) -> Result<Vec<Person>, AppError> {
    commands::list_people(&engine)
}

#[tauri::command(async)]
pub fn list_tags(engine: Eng<'_>) -> Result<Vec<TagCount>, AppError> {
    commands::list_tags(&engine)
}

#[tauri::command(async)]
pub fn list_tag_rules(engine: Eng<'_>) -> Result<Vec<TagRule>, AppError> {
    commands::list_tag_rules(&engine)
}

#[tauri::command(async)]
pub fn rename_tag(engine: Eng<'_>, from: String, to: String) -> Result<(), AppError> {
    commands::rename_tag(&engine, &from, &to)
}

#[tauri::command(async)]
pub fn hide_tag(engine: Eng<'_>, tag: String) -> Result<(), AppError> {
    commands::hide_tag(&engine, &tag)
}

#[tauri::command(async)]
pub fn restore_tag_rule(engine: Eng<'_>, tag: String) -> Result<(), AppError> {
    commands::restore_tag_rule(&engine, &tag)
}

#[tauri::command(async)]
pub fn list_albums(engine: Eng<'_>) -> Result<Vec<AlbumSummary>, AppError> {
    commands::list_albums(&engine)
}

#[tauri::command(async)]
pub fn create_album(engine: Eng<'_>, name: String) -> Result<Album, AppError> {
    commands::create_album(&engine, &name)
}

#[tauri::command(async)]
pub fn rename_album(engine: Eng<'_>, album_id: i64, name: String) -> Result<(), AppError> {
    commands::rename_album(&engine, album_id, &name)
}

#[tauri::command(async)]
pub fn delete_album(engine: Eng<'_>, album_id: i64) -> Result<(), AppError> {
    commands::delete_album(&engine, album_id)
}

#[tauri::command(async)]
pub fn add_to_album(engine: Eng<'_>, album_id: i64, item_ids: Vec<i64>) -> Result<(), AppError> {
    commands::add_to_album(&engine, album_id, &item_ids)
}

#[tauri::command(async)]
pub fn remove_from_album(
    engine: Eng<'_>,
    album_id: i64,
    item_ids: Vec<i64>,
) -> Result<(), AppError> {
    commands::remove_from_album(&engine, album_id, &item_ids)
}

#[tauri::command(async)]
pub fn set_visible(engine: Eng<'_>, ids: Vec<i64>) {
    commands::set_visible(&engine, &ids)
}

#[tauri::command(async)]
pub fn viewer_item(engine: Eng<'_>, id: i64) -> Result<commands::ViewerItem, AppError> {
    commands::viewer_item(&engine, id)
}

#[tauri::command(async)]
pub fn rotate_item(engine: Eng<'_>, id: i64, clockwise: bool) -> Result<(), AppError> {
    commands::rotate_item(&engine, id, clockwise)
}

#[tauri::command(async)]
pub fn set_item_edit(
    engine: Eng<'_>,
    id: i64,
    turns: u8,
    crop: Option<[u16; 4]>,
) -> Result<(), AppError> {
    commands::set_item_edit(&engine, id, turns, crop)
}

#[tauri::command(async)]
pub fn set_star(engine: Eng<'_>, id: i64, starred: bool) -> Result<(), AppError> {
    commands::set_star(&engine, id, starred)
}

#[tauri::command(async)]
pub fn add_item_tag(engine: Eng<'_>, id: i64, tag: String) -> Result<String, AppError> {
    commands::add_item_tag(&engine, id, &tag)
}

#[tauri::command(async)]
pub fn remove_item_tag(engine: Eng<'_>, id: i64, tag: String) -> Result<(), AppError> {
    commands::remove_item_tag(&engine, id, &tag)
}

#[tauri::command(async)]
pub fn neighbours(engine: Eng<'_>, id: i64, radius: usize) -> Vec<i64> {
    commands::neighbours(&engine, id, radius)
}

#[tauri::command(async)]
pub fn reveal_in_file_manager(engine: Eng<'_>, id: i64) -> Result<(), AppError> {
    let path = commands::item_path(&engine, id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
}

#[tauri::command(async)]
pub fn reveal_folder(engine: Eng<'_>, folder_id: i64) -> Result<(), AppError> {
    let path = commands::folder_path(&engine, folder_id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
}

#[tauri::command(async)]
pub fn watched_folder_stats(
    engine: Eng<'_>,
) -> Result<Vec<commands::WatchedFolderStats>, AppError> {
    commands::watched_folder_stats(&engine)
}

#[tauri::command(async)]
pub fn app_info(engine: Eng<'_>) -> commands::AppInfo {
    commands::app_info(&engine)
}

#[tauri::command(async)]
pub fn reveal_watched(engine: Eng<'_>, watched_id: i64) -> Result<(), AppError> {
    let path = commands::watched_path(&engine, watched_id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
}

#[tauri::command(async)]
pub fn reveal_library(engine: Eng<'_>) -> Result<(), AppError> {
    let path = commands::app_info(&engine).library_path;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
}
