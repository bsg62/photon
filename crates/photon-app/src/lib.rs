//! photon-app: the Tauri shell around photon-core.

pub mod commands;
pub mod engine;
pub mod error;
pub mod events;
pub mod protocol;

#[cfg(test)]
mod testutil;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running photon");
}
