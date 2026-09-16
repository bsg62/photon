//! The `notify`-backed half of the watcher: register roots, debounce, and report the
//! directories that changed. Everything policy-shaped lives in `policy.rs`, which is pure.

use notify::{
    RecursiveMode,
    event::{AccessKind, AccessMode, EventKind},
};
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
                        if !may_have_changed(&event.kind) {
                            continue;
                        }
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

/// Whether an event can mean something on disk is different from what the library holds.
///
/// Linux's inotify reports every *open* and *close* as well as every write, and notify
/// registers for them, so reading a file's EXIF, listing a directory, or walkdir entering
/// one all arrive here as `Access` events on that directory. A scan does all three to
/// every directory it walks. Treating those as changes made each scan schedule the next
/// one two seconds later, for as long as photon ran: a loop that was invisible until the
/// status bar started showing scan progress. Only a close after writing is kept from the
/// access family; a change of content or name always arrives as `Modify`, `Create`,
/// `Remove` or `Any` anyway, so nothing real is lost by dropping the rest.
fn may_have_changed(kind: &EventKind) -> bool {
    !matches!(
        kind,
        EventKind::Access(
            AccessKind::Any
                | AccessKind::Read
                | AccessKind::Open(_)
                | AccessKind::Other
                | AccessKind::Close(
                    AccessMode::Any | AccessMode::Execute | AccessMode::Read | AccessMode::Other
                )
        )
    )
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

#[cfg(test)]
mod change_tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};

    #[test]
    fn only_writes_creates_removes_and_renames_count_as_changes() {
        // The access family is what a scan itself generates on every directory it walks.
        for kind in [
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            EventKind::Access(AccessKind::Open(AccessMode::Read)),
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
            EventKind::Access(AccessKind::Read),
            EventKind::Access(AccessKind::Any),
            EventKind::Access(AccessKind::Other),
        ] {
            assert!(!may_have_changed(&kind), "{kind:?} is not a change");
        }
        for kind in [
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            EventKind::Create(CreateKind::File),
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
            EventKind::Remove(RemoveKind::File),
            EventKind::Any,
            EventKind::Other,
        ] {
            assert!(may_have_changed(&kind), "{kind:?} is a change");
        }
    }

    /// Real filesystem events are timing-dependent, so this is excluded from CI.
    /// Run it locally with: cargo test -p photon-core -- --ignored a_scans_own_reads
    ///
    /// The loop this guards: a scan reads every file's header and lists every directory,
    /// and on Linux each of those is an inotify event. Reported as changes, they scheduled
    /// the next subtree scan of the same directories two seconds after every scan, forever.
    #[test]
    #[ignore]
    fn a_scans_own_reads_report_nothing_but_a_write_still_does() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("b.jpg"), b"x").unwrap();
        let (mut watcher, rx, _errors) = Watcher::start(Duration::from_millis(200)).unwrap();
        watcher.watch_root(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert!(
            rx.try_recv().is_err(),
            "registering the watch (which walks the tree) must not report a change"
        );

        // Everything a scan does to a directory: read a file, stat one, list it, walk it.
        let _ = std::fs::read(dir.path().join("sub").join("b.jpg")).unwrap();
        let _ = std::fs::metadata(dir.path().join("sub").join("b.jpg")).unwrap();
        let _ = std::fs::read_dir(dir.path().join("sub")).unwrap().count();
        let _ = walkdir::WalkDir::new(dir.path()).into_iter().count();
        assert!(
            rx.recv_timeout(Duration::from_secs(1)).is_err(),
            "reads and listings are not changes"
        );

        std::fs::write(dir.path().join("sub").join("c.jpg"), b"y").unwrap();
        let batch = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a written file is a change");
        let canonical = dunce::canonicalize(dir.path().join("sub")).unwrap();
        assert!(
            batch
                .iter()
                .any(|d| dunce::canonicalize(d).unwrap() == canonical)
        );
    }
}
