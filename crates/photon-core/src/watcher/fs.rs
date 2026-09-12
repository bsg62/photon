//! The `notify`-backed half of the watcher: register roots, debounce, and report the
//! directories that changed. Everything policy-shaped lives in `policy.rs`, which is pure.

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
    time::Duration,
};

/// A root that could not be watched, for example because the OS ran out of watch
/// descriptors. The caller falls back to periodic rescans for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchError {
    pub path: PathBuf,
    pub message: String,
}

pub struct Watcher {
    debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

impl Watcher {
    /// Starts debouncing.
    ///
    /// Each message on the first returned channel is a batch of directories that changed; a
    /// file event is reported as its parent directory. Each message on the second is a batch
    /// of watch failures reported by the OS after registration (for example an event-queue
    /// overflow, which means events were silently lost): the caller maps them back to roots
    /// and degrades those, since live updates for them can no longer be trusted.
    #[allow(clippy::type_complexity)]
    pub fn start(
        debounce: Duration,
    ) -> crate::Result<(Self, Receiver<Vec<PathBuf>>, Receiver<Vec<WatchError>>)> {
        let (tx, rx) = channel::<Vec<PathBuf>>();
        let (error_tx, error_rx) = channel::<Vec<WatchError>>();
        let debouncer = new_debouncer(debounce, None, move |result: DebounceEventResult| {
            match result {
                Ok(events) => {
                    // Both sets are what keeps a bulk import cheap here: this callback runs
                    // on the notify thread for every debounced batch, and a batch can carry
                    // thousands of paths. `seen` skips the `is_dir()` syscall for a path
                    // reported more than once, and `unique` replaces a quadratic
                    // `Vec::contains` scan per directory.
                    let mut seen: HashSet<&Path> = HashSet::new();
                    let mut unique: HashSet<PathBuf> = HashSet::new();
                    let mut dirs: Vec<PathBuf> = Vec::new();
                    for event in &events {
                        for path in &event.paths {
                            if !seen.insert(path.as_path()) {
                                continue;
                            }
                            // `is_dir()` reports `false` for a path that no longer exists,
                            // which routes a deletion to its parent directory. That's the
                            // behaviour we want: rescanning the parent is how a deletion
                            // gets noticed, since there's nothing left at `path` itself.
                            let dir = if path.is_dir() {
                                path.clone()
                            } else {
                                match path.parent() {
                                    Some(parent) => parent.to_path_buf(),
                                    None => continue,
                                }
                            };
                            if unique.insert(dir.clone()) {
                                dirs.push(dir);
                            }
                        }
                    }
                    if !dirs.is_empty() {
                        let _ = tx.send(dirs);
                    }
                }
                Err(errors) => {
                    // Reported as well as logged: an error here means events may have been
                    // lost, so the roots it touches can no longer rely on live updates and
                    // must fall back to periodic rescans until a watch is re-established.
                    // An error with no paths gets one entry with an empty path, which the
                    // policy reads as "every root is affected".
                    let mut failures = Vec::new();
                    for error in errors {
                        tracing::warn!(%error, "filesystem watch error");
                        let message = error.to_string();
                        if error.paths.is_empty() {
                            failures.push(WatchError {
                                path: PathBuf::new(),
                                message,
                            });
                        } else {
                            for path in error.paths {
                                failures.push(WatchError {
                                    path,
                                    message: message.clone(),
                                });
                            }
                        }
                    }
                    if !failures.is_empty() {
                        let _ = error_tx.send(failures);
                    }
                }
            }
        })
        .map_err(|err| std::io::Error::other(err.to_string()))?;
        Ok((Self { debouncer }, rx, error_rx))
    }

    /// Watches `path` and everything beneath it. A failure is returned rather than
    /// propagated, so one unwatchable root never stops the others.
    pub fn watch_root(&mut self, path: &Path) -> std::result::Result<(), WatchError> {
        self.debouncer
            .watch(path, RecursiveMode::Recursive)
            .map_err(|err| WatchError {
                path: path.to_path_buf(),
                message: err.to_string(),
            })
    }

    pub fn unwatch_root(&mut self, path: &Path) {
        if let Err(err) = self.debouncer.unwatch(path) {
            tracing::debug!(%err, ?path, "could not unwatch");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real filesystem events are timing-dependent, so this is excluded from CI.
    /// Run it locally with: cargo test -p photon-core -- --ignored watcher_reports
    #[test]
    #[ignore]
    fn watcher_reports_the_directory_a_new_file_landed_in() {
        let dir = tempfile::tempdir().unwrap();
        let (mut watcher, rx, _errors) = Watcher::start(Duration::from_millis(200)).unwrap();
        watcher.watch_root(dir.path()).unwrap();

        std::fs::write(dir.path().join("new.jpg"), b"x").unwrap();

        let batch = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("no event arrived");
        let canonical = dunce::canonicalize(dir.path()).unwrap();
        assert!(
            batch
                .iter()
                .any(|d| dunce::canonicalize(d).unwrap() == canonical)
        );
    }
}
