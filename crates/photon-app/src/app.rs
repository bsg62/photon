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
        self.emit("library-changed", e);
    }
    fn scan_progress(&self, e: ScanProgressEvent) {
        self.emit("scan-progress", e);
    }
    fn folder_status(&self, e: FolderStatus) {
        self.emit("folder-status", e);
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
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
                }
                Err(err) => {
                    tracing::error!(%err, "could not open the photon library");
                    app.dialog()
                        .message(format!(
                            "photon could not open its library:\n\n{err}\n\nNothing was changed."
                        ))
                        .kind(MessageDialogKind::Error)
                        .title("photon")
                        .show(|_| std::process::exit(1));
                }
            }
            Ok(())
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
        .expect("error while building photon")
        .run(|app, event| {
            if let RunEvent::Exit = event
                && let Some(engine) = app.try_state::<Arc<Engine>>()
            {
                engine.shutdown();
            }
        });
}
