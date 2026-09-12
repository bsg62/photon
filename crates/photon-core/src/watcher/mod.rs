//! Watching watched folders for changes.
//!
//! The policy — which directories become scan requests — is pure and lives in [`policy`].
//! The `notify`-backed part lives in [`fs`] and is deliberately thin.

mod fs;
mod policy;

pub use fs::{WatchError, Watcher};
pub use policy::{WatchedRoot, merge_pending, plan_scans};
