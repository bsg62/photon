//! Watching watched folders for changes.
//!
//! The policy — which directories become scan requests — is pure and lives in [`policy`].
//! The `notify`-backed part lives in `fs` and is deliberately thin (arrives in a later task).

mod policy;

pub use policy::{WatchedRoot, plan_scans};
