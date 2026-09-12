//! The `notify`-backed half of the watcher: register roots, debounce, and report the
//! directories that changed. Everything policy-shaped lives in `policy.rs`, which is pure.

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use std::{
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
    /// Starts debouncing. Each message on the returned channel is a batch of directories
    /// that changed; a file event is reported as its parent directory.
    pub fn start(debounce: Duration) -> crate::Result<(Self, Receiver<Vec<PathBuf>>)> {
        let (tx, rx) = channel::<Vec<PathBuf>>();
        let debouncer = new_debouncer(debounce, None, move |result: DebounceEventResult| {
            match result {
                Ok(events) => {
                    let mut dirs: Vec<PathBuf> = Vec::new();
                    for event in events {
                        for path in &event.paths {
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
                            if !dirs.contains(&dir) {
                                dirs.push(dir);
                            }
                        }
                    }
                    if !dirs.is_empty() {
                        let _ = tx.send(dirs);
                    }
                }
                Err(errors) => {
                    for error in errors {
                        tracing::warn!(%error, "filesystem watch error");
                    }
                }
            }
        })
        .map_err(|err| std::io::Error::other(err.to_string()))?;
        Ok((Self { debouncer }, rx))
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
        let (mut watcher, rx) = Watcher::start(Duration::from_millis(200)).unwrap();
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
