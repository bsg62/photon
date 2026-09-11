//! Tauri command wrappers. Each runs on the async runtime (`async` attribute), so
//! blocking work such as waiting for a cancelled scan never stalls the UI thread.

use crate::{commands, engine::Engine, error::AppError};
use photon_core::library::WatchedFolder;
use std::sync::Arc;
use tauri::State;

type Eng<'a> = State<'a, Arc<Engine>>;

#[tauri::command(async)]
pub fn list_folders(engine: Eng<'_>) -> Result<commands::FolderList, AppError> {
    commands::list_folders(&engine)
}

#[tauri::command(async)]
pub fn add_folder(engine: Eng<'_>, path: String) -> Result<WatchedFolder, AppError> {
    commands::add_folder(engine.inner(), &path)
}

#[tauri::command(async)]
pub fn remove_folder(engine: Eng<'_>, watched_id: i64) -> Result<(), AppError> {
    commands::remove_folder(&engine, watched_id)
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
pub fn set_visible(engine: Eng<'_>, ids: Vec<i64>) {
    commands::set_visible(&engine, &ids)
}

#[tauri::command(async)]
pub fn viewer_item(engine: Eng<'_>, id: i64) -> Result<commands::ViewerItem, AppError> {
    commands::viewer_item(&engine, id)
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
