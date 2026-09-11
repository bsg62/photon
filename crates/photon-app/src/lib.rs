//! photon-app: the Tauri shell around photon-core.

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running photon");
}
