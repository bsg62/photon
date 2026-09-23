//! Builds and runs the Tauri app: plugins, the photon:// protocol, commands, and the
//! engine's lifecycle.

use crate::{
    engine::{Engine, EngineConfig},
    events::{Events, ExportProgress, FolderStatus, LibraryChanged, ScanProgressEvent},
    ipc, protocol,
};
use photon_core::library::ThemeChoice;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_window_state::StateFlags;

/// Event names emitted to the webview. The UI (Task 12) listens for these exact
/// strings, so a typo here would break it silently.
pub const LIBRARY_CHANGED: &str = "library-changed";
pub const SCAN_PROGRESS: &str = "scan-progress";
pub const FOLDER_STATUS: &str = "folder-status";
pub const EXPORT_PROGRESS: &str = "export-progress";

/// The native window's theme for the user's choice. `None` is "the desktop's", and is what
/// lets the title bar keep following the desktop while photon runs.
fn window_theme(choice: ThemeChoice) -> Option<tauri::Theme> {
    match choice {
        ThemeChoice::System => None,
        ThemeChoice::Light => Some(tauri::Theme::Light),
        ThemeChoice::Dark => Some(tauri::Theme::Dark),
    }
}

/// What the window-state plugin persists, so photon reopens the size and place the user
/// left it rather than `tauri.conf.json`'s default every time.
///
/// Spelled out rather than taking the plugin's `StateFlags::all()`/`default()`, which also
/// persist VISIBLE — the one flag that is a trap here. `setup` hides the main window when
/// `Engine::open` fails, so the error dialog doesn't sit in front of a dead window;
/// persisting that would carry the hidden window into the next launch and photon would
/// start with no window at all and no way to ask for one. DECORATIONS is left out for a
/// duller reason: photon never changes them, so there is nothing to restore.
///
/// The state file lives in the app's *config* dir, not the data dir beside `library.db`;
/// that is the plugin's own choice, not ours.
///
/// Position is restored only when a monitor that still exists overlaps the saved rectangle
/// (the plugin checks `available_monitors`), so unplugging the screen a window was last on
/// leaves the placement to the OS rather than stranding the window off-screen.
const WINDOW_STATE_FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED)
    .union(StateFlags::FULLSCREEN);

struct TauriEvents(AppHandle);

impl TauriEvents {
    fn emit<S: Serialize + Clone>(&self, name: &str, payload: S) {
        if let Err(err) = self.0.emit(name, payload) {
            tracing::warn!(%err, name, "failed to emit event");
        }
    }
}

impl Events for TauriEvents {
    fn library_changed(&self, e: LibraryChanged) {
        self.emit(LIBRARY_CHANGED, e);
    }
    fn scan_progress(&self, e: ScanProgressEvent) {
        self.emit(SCAN_PROGRESS, e);
    }
    fn folder_status(&self, e: FolderStatus) {
        self.emit(FOLDER_STATUS, e);
    }
    fn export_progress(&self, e: ExportProgress) {
        self.emit(EXPORT_PROGRESS, e);
    }
}

pub fn run() {
    // First, while the process is still single-threaded; see `webkit`.
    #[cfg(target_os = "linux")]
    let dmabuf_disabled = crate::webkit::apply_dmabuf_workaround();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    #[cfg(target_os = "linux")]
    if dmabuf_disabled {
        tracing::info!("NVIDIA driver detected; WebKit's DMABUF renderer is disabled");
    }

    match tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(WINDOW_STATE_FLAGS)
                .build(),
        )
        .register_asynchronous_uri_scheme_protocol("photon", |ctx, request, responder| {
            let engine = ctx
                .app_handle()
                .try_state::<Arc<Engine>>()
                .map(|s| s.inner().clone());
            let path = request.uri().path().to_string();
            tauri::async_runtime::spawn_blocking(move || {
                let response = match engine {
                    Some(engine) => protocol::handle(&engine, &path),
                    None => tauri::http::Response::builder()
                        .status(503)
                        .body(b"starting".to_vec())
                        .expect("static response"),
                };
                responder.respond(response);
            });
        })
        .setup(|app| {
            let paths = app.path();
            let config = EngineConfig {
                db_path: paths.app_data_dir()?.join("library.db"),
                cache_dir: paths.app_cache_dir()?.join("thumbs"),
                workers: photon_core::thumbs::default_workers(),
            };
            let events = Arc::new(TauriEvents(app.handle().clone()));
            match Engine::open(config, events) {
                Ok(engine) => {
                    // The title bar, before the webview has run a line of script. The UI
                    // sets the window's theme too (`app-theme.svelte.ts`), but only once it
                    // has loaded and asked for the setting over IPC; until then a pinned
                    // theme that differs from the desktop's showed the desktop's title bar
                    // over photon's own colours. This is the earliest the choice is known:
                    // the library has just opened, and the configured window already
                    // exists. A failure costs that moment's mismatch and nothing else, so
                    // it is logged and startup goes on.
                    match engine.lib.theme() {
                        Ok(choice) => {
                            if let Some(window) = app.get_webview_window("main")
                                && let Err(err) = window.set_theme(window_theme(choice))
                            {
                                tracing::warn!(%err, "could not theme the title bar at launch");
                            }
                        }
                        Err(err) => tracing::warn!(%err, "could not read the theme at launch"),
                    }
                    let pictures = paths.picture_dir().ok();
                    app.manage(engine.clone());
                    engine.startup(pictures);
                    Ok(())
                }
                Err(err) => {
                    tracing::error!(%err, "could not open the photon library");
                    // `setup` runs on the main thread (inside Tauri's event-loop `Ready`
                    // callback: tauri-2.11.5/src/app.rs:1424 calls `setup(&mut self)`
                    // from `make_run_event_loop_callback`, which the event loop invokes on
                    // its own thread). `blocking_show` can't be used here: it blocks the
                    // calling thread on a channel that only resolves once the dialog has
                    // actually been shown, but showing it requires `run_on_main_thread`
                    // (tauri-plugin-dialog-2.7.3/src/desktop.rs:213-228) to run a queued
                    // closure on that same main thread's event loop - so blocking this
                    // thread on it would deadlock the whole app before any window ever
                    // appears. The non-blocking `show` (with its callback) is the only
                    // option here; it hands off to the main thread's event loop instead of
                    // blocking it.
                    //
                    // The window Tauri configured (`tauri.conf.json`'s `app.windows`) already
                    // exists by the time this hook runs, so hide it first - otherwise an
                    // empty, non-functional window would sit behind the dialog.
                    if let Some(window) = app.get_webview_window("main")
                        && let Err(err) = window.hide()
                    {
                        tracing::warn!(%err, "could not hide the main window");
                    }
                    app.dialog()
                        .message(format!(
                            "photon could not open its library:\n\n{err}\n\nNothing was changed."
                        ))
                        .kind(MessageDialogKind::Error)
                        .title("photon")
                        .show(|_| std::process::exit(1));
                    Ok(())
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ipc::list_folders,
            ipc::add_folder,
            ipc::remove_folder,
            ipc::rescan_folder,
            ipc::grid_info,
            ipc::grid_rows,
            ipc::grid_offset_of_folder,
            ipc::grid_offset_of_item,
            ipc::last_folder,
            ipc::set_last_folder,
            ipc::slideshow_interval,
            ipc::set_slideshow_interval,
            ipc::similar_distance,
            ipc::set_similar_distance,
            ipc::theme,
            ipc::set_theme,
            ipc::grid_tile,
            ipc::set_grid_tile,
            ipc::set_grid_view,
            ipc::set_search_query,
            ipc::set_person_view,
            ipc::set_album_view,
            ipc::set_tag_view,
            ipc::copy_count,
            ipc::set_copies_view,
            ipc::list_people,
            ipc::list_tags,
            ipc::list_tag_rules,
            ipc::rename_tag,
            ipc::hide_tag,
            ipc::restore_tag_rule,
            ipc::list_albums,
            ipc::create_album,
            ipc::rename_album,
            ipc::delete_album,
            ipc::list_saved_searches,
            ipc::save_search,
            ipc::rename_saved_search,
            ipc::delete_saved_search,
            ipc::add_to_album,
            ipc::remove_from_album,
            ipc::set_visible,
            ipc::viewer_item,
            ipc::set_star,
            ipc::set_stars,
            ipc::set_items_hidden,
            ipc::set_folder_hidden,
            ipc::rotate_item,
            ipc::set_item_edit,
            ipc::add_item_tag,
            ipc::remove_item_tag,
            ipc::export_items,
            ipc::check_export_dest,
            ipc::export_apply_edits,
            ipc::set_export_apply_edits,
            ipc::add_items_tag,
            ipc::remove_items_tag,
            ipc::neighbours,
            ipc::reveal_in_file_manager,
            ipc::reveal_folder,
            ipc::watched_folder_stats,
            ipc::app_info,
            ipc::reveal_watched,
            ipc::reveal_library,
        ])
        .build(tauri::generate_context!())
    {
        // `build` never runs `setup` - it only assembles the app and its windows. Errors
        // here are things like a broken bundle/context, not a failed `Engine::open`
        // (that failure is handled inside `setup` above, once the event loop actually
        // starts and calls it). This match only ever sees a genuine `build` failure.
        Ok(app) => app.run(|app, event| {
            if let RunEvent::Exit = event
                && let Some(engine) = app.try_state::<Arc<Engine>>()
            {
                engine.shutdown();
            }
        }),
        Err(err) => {
            tracing::error!(%err, "could not build photon");
            eprintln!("photon could not start: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window-state flags are the one part of reopening at the last size that can be
    /// checked without a window. VISIBLE is the flag that matters: `setup` hides the main
    /// window when the library cannot be opened, and persisting that would reopen photon
    /// hidden — running, with nothing on screen and no way to ask for a window. Widening
    /// this to `StateFlags::all()` or the plugin's default is what this guards against.
    #[test]
    fn the_window_state_flags_never_persist_visibility() {
        assert!(!WINDOW_STATE_FLAGS.contains(StateFlags::VISIBLE));
        assert!(!WINDOW_STATE_FLAGS.contains(StateFlags::DECORATIONS));
        assert!(WINDOW_STATE_FLAGS.contains(StateFlags::SIZE));
        assert!(WINDOW_STATE_FLAGS.contains(StateFlags::POSITION));
        assert!(WINDOW_STATE_FLAGS.contains(StateFlags::MAXIMIZED));
        assert!(WINDOW_STATE_FLAGS.contains(StateFlags::FULLSCREEN));
    }

    /// The call that uses this needs a window, so the mapping is the part that can be held
    /// here. `System` must be `None`, not the desktop's scheme resolved in Rust: `None` hands
    /// the title bar back to the desktop, which then keeps following it while photon runs.
    #[test]
    fn a_pinned_theme_is_the_windows_theme_and_system_is_none() {
        assert_eq!(window_theme(ThemeChoice::System), None);
        assert_eq!(window_theme(ThemeChoice::Light), Some(tauri::Theme::Light));
        assert_eq!(window_theme(ThemeChoice::Dark), Some(tauri::Theme::Dark));
    }

    #[test]
    fn event_names_match_what_the_ui_listens_for() {
        assert_eq!(LIBRARY_CHANGED, "library-changed");
        assert_eq!(SCAN_PROGRESS, "scan-progress");
        assert_eq!(FOLDER_STATUS, "folder-status");
    }
}
