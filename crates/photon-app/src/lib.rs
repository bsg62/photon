//! photon-app: the Tauri shell around photon-core.

pub mod engine;
pub mod events;

#[cfg(test)]
mod testutil;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running photon");
}
