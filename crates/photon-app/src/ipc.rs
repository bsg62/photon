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
//! functions. `add_folder` (whose `photon_core::paths::canonicalize` can hang on a dead network
//! mount) and `remove_folder` (which joins a scan thread) can genuinely block for a
//! while, so they're `async fn`s that hand their blocking body to
//! `tauri::async_runtime::spawn_blocking`, which *does* run on Tauri's dedicated
//! blocking pool. So does every other command that can: a full-size render, an export, a
//! path the user picked, a long poll. `an_export_holds_no_ipc_worker` shows what the
//! difference is, through Tauri's own dispatch.

use crate::{commands, engine::Engine, error::AppError};
use photon_core::library::{
    Album, AlbumSummary, FaceFilter, PageFace, PeoplePage, Person, SavedSearch, TagCount, TagRule,
    WatchedFolder,
};
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

/// Copies the photo, as shown, to the clipboard as a picture. An `async fn` that hands all of
/// it to the blocking pool, like `add_folder`: the full-size decode takes a second or two and
/// waits its turn behind any other full-size render, and the clipboard write is not cheap
/// either - on Linux the clipboard library encodes a PNG inside it.
#[tauri::command(async)]
pub async fn copy_photo(
    app: tauri::AppHandle,
    engine: Eng<'_>,
    item_id: i64,
) -> Result<(), AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        // `copy_picture` drops the render lock when it returns: the write below must not
        // hold up the viewer's next render.
        let picture = commands::copy_picture(&engine, item_id)?;
        let (width, height) = picture.dimensions();
        let image = tauri::image::Image::new_owned(picture.into_raw(), width, height);
        app.clipboard().write_image(&image).map_err(|err| AppError {
            kind: "clipboard",
            message: format!("Couldn't put the photo on the clipboard: {err}"),
        })
    })
    .await
    .map_err(AppError::internal)?
}

#[tauri::command(async)]
pub fn rescan_folder(engine: Eng<'_>, watched_id: i64) -> Result<(), AppError> {
    commands::rescan_folder(engine.inner(), watched_id)
}

#[tauri::command(async)]
pub fn grid_info(engine: Eng<'_>, known_layout: Option<u64>) -> commands::GridInfo {
    commands::grid_info(&engine, known_layout)
}

#[tauri::command(async)]
pub fn grid_rows(engine: Eng<'_>, offset: usize, count: usize) -> commands::GridRows {
    commands::grid_rows(&engine, offset, count)
}

#[tauri::command(async)]
pub fn grid_folder_ids_at(engine: Eng<'_>, offset: usize) -> Option<commands::FolderIds> {
    commands::grid_folder_ids_at(&engine, offset)
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
pub fn slideshow_shuffle(engine: Eng<'_>) -> Result<bool, AppError> {
    commands::slideshow_shuffle(&engine)
}

#[tauri::command(async)]
pub fn set_slideshow_shuffle(engine: Eng<'_>, shuffle: bool) -> Result<(), AppError> {
    commands::set_slideshow_shuffle(&engine, shuffle)
}

#[tauri::command(async)]
pub fn similar_distance(engine: Eng<'_>) -> Result<i64, AppError> {
    commands::similar_distance(&engine)
}

#[tauri::command(async)]
pub fn set_similar_distance(engine: Eng<'_>, distance: i64) -> Result<i64, AppError> {
    commands::set_similar_distance(engine.inner(), distance)
}

#[tauri::command(async)]
pub fn face_detection(engine: Eng<'_>) -> Result<bool, AppError> {
    commands::face_detection(&engine)
}

#[tauri::command(async)]
pub fn set_face_detection(engine: Eng<'_>, enabled: bool) -> Result<(), AppError> {
    commands::set_face_detection(engine.inner(), enabled)
}

#[tauri::command(async)]
pub fn people_to_name(engine: Eng<'_>) -> Result<i64, AppError> {
    commands::people_to_name(&engine)
}

#[tauri::command(async)]
pub fn face_data_summary(engine: Eng<'_>) -> Result<commands::FaceDataSummary, AppError> {
    commands::face_data_summary(&engine)
}

#[tauri::command(async)]
pub fn people_page(engine: Eng<'_>, strip: usize) -> Result<PeoplePage, AppError> {
    commands::people_page(&engine, strip)
}

#[tauri::command(async)]
pub fn person_faces(
    engine: Eng<'_>,
    person: i64,
    which: FaceFilter,
    offset: usize,
    limit: usize,
) -> Result<Vec<PageFace>, AppError> {
    commands::person_faces(&engine, person, which, offset, limit)
}

#[tauri::command(async)]
pub fn name_person(engine: Eng<'_>, person: i64, name: String) -> Result<i64, AppError> {
    commands::name_person(engine.inner(), person, &name)
}

#[tauri::command(async)]
pub fn rename_person(engine: Eng<'_>, person: i64, name: String) -> Result<i64, AppError> {
    commands::rename_person(engine.inner(), person, &name)
}

#[tauri::command(async)]
pub fn confirm_faces(engine: Eng<'_>, faces: Vec<i64>) -> Result<(), AppError> {
    commands::confirm_faces(engine.inner(), &faces)
}

#[tauri::command(async)]
pub fn reject_faces(engine: Eng<'_>, faces: Vec<i64>) -> Result<(), AppError> {
    commands::reject_faces(engine.inner(), &faces)
}

#[tauri::command(async)]
pub fn merge_people(engine: Eng<'_>, from: i64, into: i64) -> Result<(), AppError> {
    commands::merge_people(engine.inner(), from, into)
}

#[tauri::command(async)]
pub fn ignore_person(engine: Eng<'_>, person: i64, ignored: bool) -> Result<(), AppError> {
    commands::ignore_person(engine.inner(), person, ignored)
}

#[tauri::command(async)]
pub fn ignore_faces(engine: Eng<'_>, faces: Vec<i64>, ignored: bool) -> Result<(), AppError> {
    commands::ignore_faces(engine.inner(), &faces, ignored)
}

#[tauri::command(async)]
pub fn delete_person(engine: Eng<'_>, person: i64) -> Result<(), AppError> {
    commands::delete_person(engine.inner(), person)
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
pub fn grid_tile(engine: Eng<'_>) -> Result<photon_core::library::GridTile, AppError> {
    commands::grid_tile(&engine)
}

#[tauri::command(async)]
pub fn set_grid_tile(
    engine: Eng<'_>,
    tile: photon_core::library::GridTile,
) -> Result<(), AppError> {
    commands::set_grid_tile(&engine, tile)
}

#[tauri::command(async)]
pub fn set_grid_view(
    engine: Eng<'_>,
    view: photon_core::grid::GridView,
) -> Result<Option<u64>, AppError> {
    commands::set_grid_view(&engine, view)
}

#[tauri::command(async)]
pub fn set_sort(engine: Eng<'_>, sort: photon_core::sort::Sort) -> Result<Option<u64>, AppError> {
    commands::set_sort(&engine, sort)
}

#[tauri::command(async)]
pub fn set_search_query(engine: Eng<'_>, query: String) -> Result<Option<u64>, AppError> {
    commands::set_search_query(&engine, &query)
}

#[tauri::command(async)]
pub fn set_person_view(engine: Eng<'_>, person: String) -> Result<Option<u64>, AppError> {
    commands::set_person_view(&engine, &person)
}

#[tauri::command(async)]
pub fn set_album_view(engine: Eng<'_>, album_id: i64) -> Result<Option<u64>, AppError> {
    commands::set_album_view(&engine, album_id)
}

#[tauri::command(async)]
pub fn set_tag_view(engine: Eng<'_>, tag: String) -> Result<Option<u64>, AppError> {
    commands::set_tag_view(&engine, &tag)
}

#[tauri::command(async)]
pub fn copy_count(engine: Eng<'_>, id: i64) -> Result<usize, AppError> {
    commands::copy_count(&engine, id)
}

#[tauri::command(async)]
pub fn set_copies_view(engine: Eng<'_>, id: i64) -> Result<Option<u64>, AppError> {
    commands::set_copies_view(&engine, id)
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
pub fn list_saved_searches(engine: Eng<'_>) -> Result<Vec<SavedSearch>, AppError> {
    commands::list_saved_searches(&engine)
}

#[tauri::command(async)]
pub fn save_search(engine: Eng<'_>, name: String, query: String) -> Result<SavedSearch, AppError> {
    commands::save_search(&engine, &name, &query)
}

#[tauri::command(async)]
pub fn rename_saved_search(engine: Eng<'_>, search_id: i64, name: String) -> Result<(), AppError> {
    commands::rename_saved_search(&engine, search_id, &name)
}

#[tauri::command(async)]
pub fn delete_saved_search(engine: Eng<'_>, search_id: i64) -> Result<(), AppError> {
    commands::delete_saved_search(&engine, search_id)
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
    commands::rotate_item(engine.inner(), id, clockwise)
}

#[tauri::command(async)]
pub fn set_item_edit(
    engine: Eng<'_>,
    id: i64,
    turns: u8,
    crop: Option<[u16; 4]>,
) -> Result<(), AppError> {
    commands::set_item_edit(engine.inner(), id, turns, crop)
}

/// On the blocking pool like `add_folder`: a star is written into `.picasa.ini` in the
/// photo's own folder, often on a network share, which hangs when the share has stopped
/// answering - and every other star then queues behind it on `Engine::ini_write`.
#[tauri::command(async)]
pub async fn set_star(engine: Eng<'_>, id: i64, starred: bool) -> Result<(), AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || commands::set_star(&engine, id, starred))
        .await
        .map_err(AppError::internal)?
}

/// As `set_star`, once per folder in the selection.
#[tauri::command(async)]
pub async fn set_stars(engine: Eng<'_>, ids: Vec<i64>, starred: bool) -> Result<usize, AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || commands::set_stars(&engine, &ids, starred))
        .await
        .map_err(AppError::internal)?
}

#[tauri::command(async)]
pub fn set_folder_hidden(engine: Eng<'_>, folder_id: i64, hidden: bool) -> Result<usize, AppError> {
    commands::set_folder_hidden(&engine, folder_id, hidden)
}

#[tauri::command(async)]
pub fn set_folder_alias(
    engine: Eng<'_>,
    folder_id: i64,
    alias: Option<String>,
) -> Result<bool, AppError> {
    commands::set_folder_alias(&engine, folder_id, alias)
}

#[tauri::command(async)]
pub fn set_items_hidden(engine: Eng<'_>, ids: Vec<i64>, hidden: bool) -> Result<usize, AppError> {
    commands::set_items_hidden(&engine, &ids, hidden)
}

#[tauri::command(async)]
pub fn add_item_tag(engine: Eng<'_>, id: i64, tag: String) -> Result<String, AppError> {
    commands::add_item_tag(&engine, id, &tag)
}

#[tauri::command(async)]
pub fn remove_item_tag(engine: Eng<'_>, id: i64, tag: String) -> Result<(), AppError> {
    commands::remove_item_tag(&engine, id, &tag)
}

/// On the blocking pool like `add_folder`: the call lasts the whole export - every copy, and
/// every edited photo's render waiting its turn behind the viewer's - and the destination is
/// canonicalized first, which can hang on a dead mount. Progress still arrives as events
/// while it runs.
#[tauri::command(async)]
pub async fn export_items(
    engine: Eng<'_>,
    ids: Vec<i64>,
    dest: String,
    apply_edits: bool,
    max_edge: Option<u32>,
) -> Result<commands::ExportReport, AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        commands::export_items(&engine, &ids, &dest, apply_edits, max_edge)
    })
    .await
    .map_err(AppError::internal)?
}

/// On the blocking pool for the reason `add_folder` is: it canonicalizes a path the user
/// picked, which can hang on a dead network mount.
#[tauri::command(async)]
pub async fn check_export_dest(engine: Eng<'_>, dest: String) -> Result<(), AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || commands::check_export_dest(&engine, &dest))
        .await
        .map_err(AppError::internal)?
}

#[tauri::command(async)]
pub fn export_apply_edits(engine: Eng<'_>) -> Result<bool, AppError> {
    commands::export_apply_edits(&engine)
}

#[tauri::command(async)]
pub fn set_export_apply_edits(engine: Eng<'_>, apply: bool) -> Result<(), AppError> {
    commands::set_export_apply_edits(&engine, apply)
}

#[tauri::command(async)]
pub fn add_items_tag(
    engine: Eng<'_>,
    ids: Vec<i64>,
    tag: String,
) -> Result<commands::TagWrite, AppError> {
    commands::add_items_tag(&engine, &ids, &tag)
}

#[tauri::command(async)]
pub fn remove_items_tag(
    engine: Eng<'_>,
    ids: Vec<i64>,
    tag: String,
) -> Result<commands::TagWrite, AppError> {
    commands::remove_items_tag(&engine, &ids, &tag)
}

#[tauri::command(async)]
pub fn neighbours(engine: Eng<'_>, id: i64, radius: usize) -> Vec<i64> {
    commands::neighbours(&engine, id, radius)
}

/// Every call into the opener runs on the blocking pool, like `add_folder`. Before it does
/// anything the opener canonicalizes (reveal) or stats (open) the path - a photo's, often on
/// a network share, which hangs when the share has stopped answering - and then makes a
/// blocking call of its own: D-Bus to the file manager on Linux, the shell on Windows.
async fn with_opener(
    engine: Eng<'_>,
    open: impl FnOnce(&Engine) -> Result<(), AppError> + Send + 'static,
) -> Result<(), AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || open(&engine))
        .await
        .map_err(AppError::internal)?
}

#[tauri::command(async)]
pub async fn reveal_in_file_manager(engine: Eng<'_>, id: i64) -> Result<(), AppError> {
    with_opener(engine, move |engine| {
        let path = commands::item_path(engine, id)?;
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
    })
    .await
}

/// Hands the photo's file to whatever the system opens that kind of file with. photon
/// registers no file types, so that is never photon itself.
#[tauri::command(async)]
pub async fn open_in_default_app(engine: Eng<'_>, id: i64) -> Result<(), AppError> {
    with_opener(engine, move |engine| {
        let path = commands::item_path(engine, id)?;
        tauri_plugin_opener::open_path(path, None::<&str>).map_err(AppError::internal)
    })
    .await
}

/// Opens the place a photo was taken in the system's browser. Nothing happens for a photo
/// with no position: the UI offers this only where there is one.
#[tauri::command(async)]
pub async fn open_in_map(engine: Eng<'_>, id: i64) -> Result<(), AppError> {
    with_opener(engine, move |engine| {
        match commands::item_map_url(engine, id)? {
            Some(url) => {
                tauri_plugin_opener::open_url(url, None::<&str>).map_err(AppError::internal)
            }
            None => Ok(()),
        }
    })
    .await
}

#[tauri::command(async)]
pub async fn reveal_folder(engine: Eng<'_>, folder_id: i64) -> Result<(), AppError> {
    with_opener(engine, move |engine| {
        let path = commands::folder_path(engine, folder_id)?;
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
    })
    .await
}

#[tauri::command(async)]
pub fn watched_folder_stats(
    engine: Eng<'_>,
) -> Result<Vec<commands::WatchedFolderStats>, AppError> {
    commands::watched_folder_stats(&engine)
}

#[tauri::command(async)]
pub fn library_stats(engine: Eng<'_>) -> Result<photon_core::library::LibraryStats, AppError> {
    commands::library_stats(&engine)
}

#[tauri::command(async)]
pub fn app_info(engine: Eng<'_>) -> commands::AppInfo {
    commands::app_info(&engine)
}

#[tauri::command(async)]
pub fn memory_usage() -> Result<commands::MemoryUsage, AppError> {
    commands::memory_usage()
}

/// The likeliest of all to meet a dead mount: Settings offers it for a root that is offline.
#[tauri::command(async)]
pub async fn reveal_watched(engine: Eng<'_>, watched_id: i64) -> Result<(), AppError> {
    with_opener(engine, move |engine| {
        let path = commands::watched_path(engine, watched_id)?;
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
    })
    .await
}

#[tauri::command(async)]
pub async fn reveal_library(engine: Eng<'_>) -> Result<(), AppError> {
    with_opener(engine, |engine| {
        let path = commands::app_info(engine).library_path;
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
    })
    .await
}

#[tauri::command(async)]
pub fn media_base(server: State<'_, crate::media_server::MediaServer>) -> Result<String, AppError> {
    Ok(commands::media_base(&server))
}

#[tauri::command(async)]
pub fn video_session_start(engine: Eng<'_>, supported: bool) -> Result<(), AppError> {
    commands::video_session_start(&engine, supported)
}

/// Holds the call open up to `VIDEO_JOB_WAIT`, so it runs on the blocking pool like
/// `add_folder`: parked on a worker thread it would stall every other command's dispatch.
#[tauri::command(async)]
pub async fn next_video_job(engine: Eng<'_>) -> Result<Option<commands::VideoJobDto>, AppError> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        commands::next_video_job(&engine, commands::VIDEO_JOB_WAIT)
    })
    .await
    .map_err(AppError::internal)?
}

/// The frame arrives as the raw request body - a JPEG of a few hundred KB, which as a JSON
/// number array would be several MB - with its id and key in headers.
#[tauri::command(async)]
pub async fn put_video_frame(
    engine: Eng<'_>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), AppError> {
    let tauri::ipc::InvokeBody::Raw(jpeg) = request.body().clone() else {
        return Err(AppError::internal("expected the frame as raw bytes"));
    };
    let get = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let id: i64 = get("x-photon-id")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| AppError::internal("missing x-photon-id"))?;
    let key = get("x-photon-key").ok_or_else(|| AppError::internal("missing x-photon-key"))?;
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        commands::put_video_frame(&engine, id, &key, &jpeg)
    })
    .await
    .map_err(AppError::internal)?
}

#[tauri::command(async)]
pub fn video_frame_failed(
    engine: Eng<'_>,
    id: i64,
    key: String,
    reason: photon_core::thumbs::VideoFailure,
) -> Result<(), AppError> {
    commands::video_frame_failed(&engine, id, &key, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Fixture, fixture, jpeg};
    use serde_json::json;
    use std::{sync::mpsc, time::Duration};
    use tauri::{
        Manager,
        ipc::{CallbackFn, InvokeBody},
        test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets},
        webview::InvokeRequest,
    };

    fn request(cmd: &str, args: serde_json::Value) -> InvokeRequest {
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        }
    }

    /// Sends as many `cmd`s as Tauri's runtime has worker threads, while `park` holds a lock
    /// each of them waits on, then `grid_info`, and fails unless `grid_info` answers while
    /// they are still parked. Run on a worker, the parked calls hold every one of them, and
    /// no other command is dispatched until they finish. Sent through Tauri's own dispatch,
    /// because the difference is in what `#[tauri::command(async)]` does with a plain `fn`.
    /// `check` reads each parked call's result once the lock is released. The runtime is
    /// Tauri's global one, shared by every test in the binary, so one command reverted to a
    /// plain `fn` starves it for the others too: when several of these fail together, the
    /// one to read is the one named after the command that changed.
    fn assert_parked_calls_hold_no_worker<G>(
        f: &Fixture,
        cmd: &str,
        args: impl Fn(usize) -> serde_json::Value,
        park: impl FnOnce() -> G,
        check: impl Fn(serde_json::Value),
    ) {
        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![
                export_items,
                set_star,
                set_stars,
                grid_info
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        app.manage(f.engine.clone());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let workers = tauri::async_runtime::handle()
            .inner()
            .metrics()
            .num_workers();

        let parked = park();
        std::thread::scope(|s| {
            let calls: Vec<_> = (0..workers)
                .map(|n| {
                    let (webview, args) = (&webview, args(n));
                    s.spawn(move || get_ipc_response(webview, request(cmd, args)))
                })
                .collect();
            // Long enough for every call to be dispatched and reach the lock.
            std::thread::sleep(Duration::from_millis(300));
            let (tx, rx) = mpsc::channel();
            let webview = &webview;
            s.spawn(move || {
                let _ = tx.send(get_ipc_response(webview, request("grid_info", json!({}))));
            });
            let answered = rx.recv_timeout(Duration::from_secs(5));
            // Released before asserting, so a failure still lets the parked calls finish.
            drop(parked);
            assert!(
                answered.is_ok_and(|r| r.is_ok()),
                "grid_info waited behind {workers} {cmd} calls"
            );
            for call in calls {
                check(call.join().unwrap().unwrap().deserialize().unwrap());
            }
        });
    }

    /// Each export parked on the one full-size render at a time - as a real export of
    /// edited photos waits behind the viewer's render.
    #[test]
    fn an_export_holds_no_ipc_worker() {
        let f = fixture(&[("a.jpg", &jpeg(16, 8))]);
        f.add_photos();
        let id = f.ids()[0];
        // An edit, so the export renders and takes `RENDERING`.
        commands::set_item_edit(&f.engine, id, 1, None).unwrap();
        assert_parked_calls_hold_no_worker(
            &f,
            "export_items",
            |n| {
                let dest = f.dir.path().join(format!("out{n}"));
                std::fs::create_dir_all(&dest).unwrap();
                json!({ "ids": [id], "dest": dest, "applyEdits": true })
            },
            || crate::protocol::RENDERING.lock(),
            |report| assert_eq!(report["written"], 1, "{report}"),
        );
    }

    /// Each star parked on the INI-write lock, as stars queue behind one whose folder is on
    /// a share that has stopped answering.
    #[test]
    fn a_star_holds_no_ipc_worker() {
        let f = fixture(&[("a.jpg", &jpeg(16, 8))]);
        f.add_photos();
        let id = f.ids()[0];
        assert_parked_calls_hold_no_worker(
            &f,
            "set_star",
            |n| json!({ "id": id, "starred": n % 2 == 0 }),
            || f.engine.hold_ini_write(),
            |unit| assert_eq!(unit, serde_json::Value::Null),
        );
    }

    /// The same for a selection's stars.
    #[test]
    fn stars_hold_no_ipc_worker() {
        let f = fixture(&[("a.jpg", &jpeg(16, 8))]);
        f.add_photos();
        let id = f.ids()[0];
        assert_parked_calls_hold_no_worker(
            &f,
            "set_stars",
            |n| json!({ "ids": [id], "starred": n % 2 == 0 }),
            || f.engine.hold_ini_write(),
            |landed| assert_eq!(landed, 1),
        );
    }
}
