//! The `notify`-backed half of the watcher: register roots, debounce, and report the
//! directories that changed. Everything policy-shaped lives in `policy.rs`, which is pure.

use notify::{
    RecursiveMode,
    event::{AccessKind, AccessMode, EventKind},
};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, NoCache, new_debouncer_opt,
};
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
    debouncer: Debouncer<notify::RecommendedWatcher, NoCache>,
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
        let handler = move |result: DebounceEventResult| {
            match result {
                Ok(events) => {
                    let (dirs, lost) = changed_dirs(&events);
                    if !dirs.is_empty() {
                        let _ = tx.send(dirs);
                    }
                    if !lost.is_empty() {
                        let _ = error_tx.send(lost);
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
        };
        // `NoCache`, not the debouncer's `RecommendedCache`. On Windows and macOS that is a
        // file-id map, which `watch` fills by walking the whole root and reading the id of
        // every file in it - opening each one on Windows, a stat on macOS - while holding
        // the debouncer's lock; it walks again on every rescan flag and beneath every new
        // path an event names, and prunes the whole map on every removal. On a network
        // share the launch walk alone is minutes of round trips. All of it serves one
        // thing: stitching the two halves of a rename into a single event. photon has no
        // use for that - every path the handler sees, from either half, is reduced to a
        // directory to rescan, so a rename rescans the same directories stitched or not.
        // Linux's recommended cache is already `NoCache`; this makes the other two
        // platforms match it.
        //
        // Symlinks are not followed, because the scanner does not follow them either
        // (`walk_tree`'s `follow_links(false)`): a photo reached only through a link is never
        // indexed, so a change beneath one has nothing to update. Following them, inotify's
        // registration walk descended every linked tree - possibly a whole other share -
        // spending a watch descriptor on each of its directories (the budget whose exhaustion
        // degrades a root) and a loop check on each link. The watched root itself is still
        // resolved when it is a link: walkdir follows a root regardless, and photon stores
        // roots canonicalized anyway. Only inotify and kqueue read this; FSEvents and
        // Windows ignore it.
        let debouncer = new_debouncer_opt::<_, notify::RecommendedWatcher, NoCache>(
            debounce,
            None,
            handler,
            NoCache,
            notify::Config::default().with_follow_symlinks(false),
        )
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

/// The directories a debounced batch says changed, and the events in it that say changes
/// were *lost*.
///
/// A lost-events notice - inotify's queue overflow, FSEvents' "must scan subdirectories" -
/// reaches the handler as an ordinary `Ok` event flagged `need_rescan`, not as an error,
/// and usually with no path at all. Read as a change it named no directory and was
/// dropped, so a root that had silently missed changes kept trusting its watch until the
/// next launch. It is reported as a watch failure instead - with its path when it has one,
/// and an empty one, which the policy reads as every root, when it has none - so the roots
/// it touches fall back to periodic rescans, which is what finds what was missed.
fn changed_dirs(events: &[DebouncedEvent]) -> (Vec<PathBuf>, Vec<WatchError>) {
    // Both sets are what keeps a bulk import cheap here: this runs on the notify thread for
    // every debounced batch, and a batch can carry thousands of paths. `seen` skips the
    // `is_dir()` syscall for a path reported more than once, and `unique` replaces a
    // quadratic `Vec::contains` scan per directory.
    let mut seen: HashSet<&Path> = HashSet::new();
    let mut unique: HashSet<PathBuf> = HashSet::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut lost: Vec<WatchError> = Vec::new();
    for event in events {
        if event.need_rescan() {
            tracing::warn!(?event, "filesystem events were lost");
            let message = "filesystem events were lost".to_string();
            if event.paths.is_empty() {
                lost.push(WatchError {
                    path: PathBuf::new(),
                    message: message.clone(),
                });
            }
            for path in &event.paths {
                lost.push(WatchError {
                    path: path.clone(),
                    message: message.clone(),
                });
            }
            continue;
        }
        if !may_have_changed(&event.kind) {
            continue;
        }
        for path in &event.paths {
            if !seen.insert(path.as_path()) {
                continue;
            }
            // `is_dir()` reports `false` for a path that no longer exists, which routes a
            // deletion to its parent directory. That's the behaviour we want: rescanning the
            // parent is how a deletion gets noticed, since there's nothing left at `path`.
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
    (dirs, lost)
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
    use crate::paths;

    /// A lost-events notice arrives as an `Ok` event flagged for a rescan, usually with no
    /// path. It must come out as a watch failure - an empty path when it names none, which
    /// the policy reads as every root - and never be dropped as a change naming nothing.
    #[test]
    fn a_lost_events_notice_is_reported_as_a_watch_failure() {
        use notify::event::{Event, Flag};
        let at = std::time::Instant::now();
        let overflow = DebouncedEvent::new(Event::new(EventKind::Other).set_flag(Flag::Rescan), at);
        let (dirs, lost) = changed_dirs(&[overflow]);
        assert!(dirs.is_empty());
        assert_eq!(
            lost.iter().map(|e| e.path.clone()).collect::<Vec<_>>(),
            [PathBuf::new()]
        );

        let subtree = PathBuf::from("/photos/2024");
        let must_scan = DebouncedEvent::new(
            Event::new(EventKind::Other)
                .set_flag(Flag::Rescan)
                .add_path(subtree.clone()),
            at,
        );
        let (dirs, lost) = changed_dirs(&[must_scan]);
        assert!(dirs.is_empty(), "a rescan notice is not a change: {dirs:?}");
        assert_eq!(
            lost.iter().map(|e| e.path.clone()).collect::<Vec<_>>(),
            [subtree]
        );
    }

    /// The inodes of every directory an inotify instance in this process is watching, read
    /// from the kernel's own table (`/proc/self/fdinfo`, one `inotify wd:.. ino:<hex>` line
    /// per watch). That is what registration actually did, with no event and no timing
    /// involved, which is why this can run in CI where the event tests cannot.
    #[cfg(target_os = "linux")]
    fn inotify_watched_inodes() -> HashSet<u64> {
        let mut inodes = HashSet::new();
        for fd in std::fs::read_dir("/proc/self/fd").unwrap().flatten() {
            let is_inotify = std::fs::read_link(fd.path())
                .is_ok_and(|target| target.as_os_str() == "anon_inode:inotify");
            if !is_inotify {
                continue;
            }
            let info_path = Path::new("/proc/self/fdinfo").join(fd.file_name());
            let Ok(info) = std::fs::read_to_string(info_path) else {
                continue;
            };
            // Only the watch lines: the file's own header has an `ino:` line too, the
            // inotify instance's inode, in decimal and tab-separated.
            let watches = info.lines().filter(|line| line.starts_with("inotify "));
            for field in watches.flat_map(str::split_whitespace) {
                if let Some(hex) = field.strip_prefix("ino:") {
                    inodes.insert(u64::from_str_radix(hex, 16).unwrap());
                }
            }
        }
        inodes
    }

    /// The scanner does not follow symlinks, so nothing beneath a link is ever indexed and
    /// the watcher must not descend one either: following it registered a watch on every
    /// directory of the linked tree - possibly a whole other network share - which is the
    /// registration walk and the watch-descriptor budget spent on photos photon never shows.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_symlinked_directory_inside_a_root_is_not_watched() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let real_sub = root.join("sub");
        std::fs::create_dir_all(&real_sub).unwrap();
        let outside = dir.path().join("outside");
        let outside_sub = outside.join("deeper");
        std::fs::create_dir_all(&outside_sub).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();

        let (mut watcher, _rx, _errors) = Watcher::start(Duration::from_millis(200)).unwrap();
        watcher.watch_root(&root).unwrap();
        let watched = inotify_watched_inodes();

        // The positive half keeps this from passing on a table it failed to read.
        assert!(watched.contains(&ino(&root)), "the root is watched");
        assert!(
            watched.contains(&ino(&real_sub)),
            "a real subdirectory is watched"
        );
        assert!(
            !watched.contains(&ino(&outside)),
            "a symlinked directory's target is not watched"
        );
        assert!(
            !watched.contains(&ino(&outside_sub)),
            "nor anything beneath it"
        );
    }

    /// Real filesystem events are timing-dependent, so this is excluded from CI.
    /// Run it locally with: cargo test -p photon-core -- --ignored a_write_beneath
    #[test]
    #[ignore]
    #[cfg(unix)]
    fn a_write_beneath_a_symlinked_directory_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let (mut watcher, rx, _errors) = Watcher::start(Duration::from_millis(200)).unwrap();
        watcher.watch_root(&root).unwrap();

        std::fs::write(outside.join("new.jpg"), b"x").unwrap();
        assert!(
            rx.recv_timeout(Duration::from_secs(1)).is_err(),
            "a write beneath a link the scanner never follows is not a change"
        );

        // And the watch is live, so the silence above is not a dead watcher.
        std::fs::write(root.join("sub").join("new.jpg"), b"x").unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("a write in a real subdirectory is a change");
    }

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
        let canonical = paths::canonicalize(dir.path()).unwrap();
        assert!(
            batch
                .iter()
                .any(|d| paths::canonicalize(d).unwrap() == canonical)
        );
    }
}

#[cfg(test)]
mod change_tests {
    use super::*;
    use crate::paths;
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
        let canonical = paths::canonicalize(dir.path().join("sub")).unwrap();
        assert!(
            batch
                .iter()
                .any(|d| paths::canonicalize(d).unwrap() == canonical)
        );
    }
}
