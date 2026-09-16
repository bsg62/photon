//! photon-app: the Tauri shell around photon-core.

mod app;
pub mod commands;
pub mod engine;
pub mod error;
pub mod events;
mod ipc;
pub mod protocol;
pub mod watch;
#[cfg(target_os = "linux")]
mod webkit;

#[cfg(test)]
mod testutil;

pub use app::run;
