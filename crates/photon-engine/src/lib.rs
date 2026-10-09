//! photon-engine: `Engine`, the commands over it and the folder watcher. No UI is named
//! here, and no UI runtime is a dependency (`the_engine_depends_on_no_ui_runtime`), so more
//! than one shell can stand on it.

pub mod commands;
pub mod engine;
pub mod error;
pub mod events;
mod memory;
pub mod watch;

#[cfg(any(test, feature = "test-support"))]
pub mod testutil;
