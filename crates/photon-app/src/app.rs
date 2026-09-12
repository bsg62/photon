//! Builds and runs the Tauri app: plugins, the photon:// protocol, commands, and the
//! engine's lifecycle.

use crate::{
    engine::{Engine, EngineConfig},
    events::{Events, FolderStatus, LibraryChanged, ScanProgressEvent},
    ipc, protocol,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// Event names emitted to the webview. The UI (Task 12) listens for these exact
/// strings, so a typo here would break it silently.
pub const LIBRARY_CHANGED: &str = "library-changed";
pub const SCAN_PROGRESS: &str = "scan-progress";
pub const FOLDER_STATUS: &str = "folder-status";

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
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    match tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
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
            ipc::set_visible,
            ipc::viewer_item,
            ipc::neighbours,
            ipc::reveal_in_file_manager,
            ipc::reveal_folder,
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

    #[test]
    fn event_names_match_what_the_ui_listens_for() {
        assert_eq!(LIBRARY_CHANGED, "library-changed");
        assert_eq!(SCAN_PROGRESS, "scan-progress");
        assert_eq!(FOLDER_STATUS, "folder-status");
    }
}
