//! photon-app: the Tauri shell around photon-engine.

mod app;
mod ipc;
pub mod media_server;
pub mod protocol;
#[cfg(target_os = "linux")]
mod webkit;

// The engine's modules under the names they had while they lived here, so `crate::engine`,
// `crate::commands` and the rest read the same in every file of this crate.
#[cfg(test)]
pub(crate) use photon_engine::testutil;
pub use photon_engine::{commands, engine, error, events, watch};

pub use app::run;
