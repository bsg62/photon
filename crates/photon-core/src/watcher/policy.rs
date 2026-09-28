use crate::paths::is_within;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// How many directories may wait as pending follow-ups for one watched folder before the
/// whole set collapses into a single rescan of that folder's root.
///
/// Small on purpose: the set exists so no change is dropped while a folder's scan slot is
/// busy, not as a queue. Beyond a handful of unrelated branches, one walk of the root is
/// cheaper than many overlapping subtree walks anyway.
pub const MAX_PENDING_DIRS: usize = 8;

/// A watched folder, as the watcher sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchedRoot {
    pub watched_id: i64,
    pub path: PathBuf,
}

/// Turns changed directories into subtree scan requests.
///
/// Drops anything inside an excluded directory or outside every watched root, collapses a
/// directory into an ancestor that is also changing (one walk covers both), removes
/// duplicates, and returns a deterministic order.
pub fn plan_scans(
    dirs: &[PathBuf],
    roots: &[WatchedRoot],
    excluded: &[PathBuf],
) -> Vec<(i64, PathBuf)> {
    let mut requests: Vec<(i64, PathBuf)> = Vec::new();
    for dir in dirs {
        if excluded.iter().any(|x| is_within(dir, x)) {
            continue;
        }
        let Some(root) = roots.iter().find(|r| is_within(dir, &r.path)) else {
            continue;
        };
        requests.push((root.watched_id, dir.clone()));
    }
    requests.sort();
    requests.dedup();
    // Keep only the shallowest request per branch: scanning an ancestor covers its
    // descendants, and sorting put every ancestor before the directories beneath it, so a
    // single forward pass is enough to drop each descendant once its ancestor is kept.
    let mut kept: Vec<(i64, PathBuf)> = Vec::new();
    for (id, dir) in requests {
        if kept
            .iter()
            .any(|(kept_id, kept_dir)| *kept_id == id && is_within(&dir, kept_dir))
        {
            continue;
        }
        kept.push((id, dir));
    }
    kept
}

/// Adds `dir` to `set`, the directories still waiting for a subtree scan of one watched
/// folder (`root`), so that between them they still cover every queued change.
///
/// A directory an already-queued ancestor covers is dropped (that one walk covers both), and
/// queued descendants of `dir` collapse into it for the same reason. Unrelated branches are
/// kept side by side rather than merged into their common ancestor: two sibling directories
/// directly under `root` have `root` itself as their common ancestor, and a subtree scan of
/// the root delegates to a full rescan — so merging would turn a bulk import touching a
/// couple of top-level directories into repeated full rescans of the folder.
///
/// The set is bounded at [`MAX_PENDING_DIRS`]: past that, tracking the individual branches
/// stops paying for itself, so the whole set collapses to `root` — one full rescan, which
/// covers everything queued and cannot drop a change.
pub fn insert_pending(set: &mut Vec<PathBuf>, dir: &Path, root: &Path) {
    if set.iter().any(|queued| is_within(dir, queued)) {
        return;
    }
    set.retain(|queued| !is_within(queued, dir));
    set.push(dir.to_path_buf());
    if set.len() > MAX_PENDING_DIRS {
        set.clear();
        set.push(root.to_path_buf());
    }
}

/// Turns folders whose only change is their Picasa INI into INI-pass requests: drops anything
/// excluded or outside every watched root, removes duplicates, and returns a deterministic
/// order. Unlike `plan_scans` it keeps a folder beneath another: the pass rereads one INI per
/// folder and does not descend, so a parent's pass says nothing about its child's INI.
pub fn plan_ini(
    dirs: &[PathBuf],
    roots: &[WatchedRoot],
    excluded: &[PathBuf],
) -> Vec<(i64, PathBuf)> {
    let mut requests: Vec<(i64, PathBuf)> = dirs
        .iter()
        .filter(|dir| !excluded.iter().any(|x| is_within(dir, x)))
        .filter_map(|dir| {
            roots
                .iter()
                .find(|r| is_within(dir, &r.path))
                .map(|r| (r.watched_id, dir.clone()))
        })
        .collect();
    requests.sort();
    requests.dedup();
    requests
}

/// What is still waiting for one watched folder while its scan slot is busy: directories to
/// walk (`insert_pending`'s rules and cap) and folders whose INI to reread. The INI set has
/// no cap - a pass over it costs one INI read per folder, and it is bounded by the folder
/// count - so an INI-only burst (photon starring across many folders, Picasa renaming an
/// album) never collapses into a rescan of the root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    pub dirs: Vec<PathBuf>,
    pub ini: BTreeSet<PathBuf>,
}

impl Pending {
    /// Queues a walk of `dir`. A walk rereads the INI of every folder it covers, so those
    /// leave the INI set; a collapse to `root` covers them all.
    pub fn queue_walk(&mut self, dir: &Path, root: &Path) {
        insert_pending(&mut self.dirs, dir, root);
        let dirs = &self.dirs;
        self.ini
            .retain(|ini| !dirs.iter().any(|walk| is_within(ini, walk)));
    }

    /// Queues an INI pass over `dir`, unless a queued walk already covers it.
    pub fn queue_ini(&mut self, dir: &Path) {
        if self.dirs.iter().any(|walk| is_within(dir, walk)) {
            return;
        }
        self.ini.insert(dir.to_path_buf());
    }

    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty() && self.ini.is_empty()
    }

    pub fn len(&self) -> usize {
        self.dirs.len() + self.ini.len()
    }
}

/// The watched folders whose live updates can no longer be trusted after the OS reported
/// `errors` on an established watch.
///
/// An error carrying a path degrades the roots that contain it. An error with no path (an
/// event-queue overflow, say) says only that events were lost somewhere, so every root is
/// degraded: claiming otherwise would leave a folder silently missing changes.
pub fn roots_affected_by(errors: &[crate::watcher::WatchError], roots: &[WatchedRoot]) -> Vec<i64> {
    let all_affected = errors.iter().any(|err| {
        err.path.as_os_str().is_empty() || !roots.iter().any(|r| is_within(&err.path, &r.path))
    });
    roots
        .iter()
        .filter(|root| all_affected || errors.iter().any(|err| is_within(&err.path, &root.path)))
        .map(|root| root.watched_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<WatchedRoot> {
        vec![
            WatchedRoot {
                watched_id: 1,
                path: PathBuf::from("/photos"),
            },
            WatchedRoot {
                watched_id: 2,
                path: PathBuf::from("/other"),
            },
        ]
    }

    #[test]
    fn maps_directories_to_their_watched_folder() {
        let dirs = [PathBuf::from("/photos/a"), PathBuf::from("/other/b")];
        assert_eq!(
            plan_scans(&dirs, &roots(), &[]),
            vec![
                (1, PathBuf::from("/photos/a")),
                (2, PathBuf::from("/other/b"))
            ]
        );
    }

    #[test]
    fn collapses_a_directory_into_an_ancestor_that_is_also_changing() {
        let dirs = [
            PathBuf::from("/photos/a/deep"),
            PathBuf::from("/photos/a"),
            PathBuf::from("/photos/b"),
        ];
        assert_eq!(
            plan_scans(&dirs, &roots(), &[]),
            vec![
                (1, PathBuf::from("/photos/a")),
                (1, PathBuf::from("/photos/b"))
            ]
        );
    }

    #[test]
    fn drops_excluded_and_unknown_directories() {
        let dirs = [
            PathBuf::from("/photos/cache"),
            PathBuf::from("/photos/cache/deep"),
            PathBuf::from("/elsewhere/x"),
            PathBuf::from("/photos/keep"),
        ];
        let excluded = [PathBuf::from("/photos/cache")];
        assert_eq!(
            plan_scans(&dirs, &roots(), &excluded),
            vec![(1, PathBuf::from("/photos/keep"))]
        );
    }

    #[test]
    fn removes_duplicates_and_is_deterministic() {
        let dirs = [
            PathBuf::from("/other/b"),
            PathBuf::from("/photos/a"),
            PathBuf::from("/photos/a"),
        ];
        assert_eq!(
            plan_scans(&dirs, &roots(), &[]),
            vec![
                (1, PathBuf::from("/photos/a")),
                (2, PathBuf::from("/other/b"))
            ]
        );
        assert!(plan_scans(&[], &roots(), &[]).is_empty());
    }

    #[test]
    fn a_changed_root_itself_is_a_valid_request() {
        let dirs = [PathBuf::from("/photos")];
        assert_eq!(
            plan_scans(&dirs, &roots(), &[]),
            vec![(1, PathBuf::from("/photos"))]
        );
    }

    fn pending(dirs: &[&str]) -> Vec<PathBuf> {
        let root = PathBuf::from("/photos");
        let mut set = Vec::new();
        for dir in dirs {
            insert_pending(&mut set, Path::new(dir), &root);
        }
        set
    }

    #[test]
    fn insert_pending_keeps_only_the_ancestor_of_a_nested_pair() {
        assert_eq!(
            pending(&["/photos/a", "/photos/a/deep"]),
            vec![PathBuf::from("/photos/a")]
        );
        assert_eq!(
            pending(&["/photos/a/deep", "/photos/a"]),
            vec![PathBuf::from("/photos/a")],
            "a newly queued ancestor absorbs the descendants already queued"
        );
    }

    #[test]
    fn insert_pending_keeps_siblings_side_by_side() {
        // The whole point of the bounded set: merging these two into their common ancestor
        // would be the watched root, whose subtree scan is a full rescan of the folder.
        assert_eq!(
            pending(&["/photos/a", "/photos/b"]),
            vec![PathBuf::from("/photos/a"), PathBuf::from("/photos/b")]
        );
    }

    #[test]
    fn insert_pending_collapses_to_the_root_only_on_overflow() {
        let eight: Vec<String> = (0..MAX_PENDING_DIRS)
            .map(|i| format!("/photos/{i}"))
            .collect();
        let refs: Vec<&str> = eight.iter().map(String::as_str).collect();
        assert_eq!(
            pending(&refs).len(),
            MAX_PENDING_DIRS,
            "exactly at the bound, nothing collapses"
        );

        let mut set = pending(&refs);
        insert_pending(&mut set, Path::new("/photos/extra"), Path::new("/photos"));
        assert_eq!(
            set,
            vec![PathBuf::from("/photos")],
            "one past the bound, the set becomes a single full rescan of the folder"
        );

        // The collapsed set stays collapsed rather than growing again: the root covers
        // everything that could arrive afterwards.
        insert_pending(&mut set, Path::new("/photos/a/deep"), Path::new("/photos"));
        assert_eq!(set, vec![PathBuf::from("/photos")]);
    }

    #[test]
    fn insert_pending_ignores_a_directory_already_covered() {
        assert_eq!(
            pending(&["/photos/a", "/photos/a", "/photos/a/deep/deeper"]),
            vec![PathBuf::from("/photos/a")]
        );
    }

    /// Review Focus 1.
    #[test]
    fn plan_ini_keeps_a_nested_ini_folder() {
        let roots = [WatchedRoot {
            watched_id: 1,
            path: PathBuf::from("/p"),
        }];
        let planned = plan_ini(
            &[
                PathBuf::from("/p/a/b"),
                PathBuf::from("/p/a"),
                PathBuf::from("/p/a"),
            ],
            &roots,
            &[],
        );
        assert_eq!(
            planned,
            [(1, PathBuf::from("/p/a")), (1, PathBuf::from("/p/a/b"))]
        );
    }

    #[test]
    fn plan_ini_drops_excluded_and_unwatched_folders() {
        let roots = [WatchedRoot {
            watched_id: 1,
            path: PathBuf::from("/p"),
        }];
        let planned = plan_ini(
            &[
                PathBuf::from("/p/x/a"),
                PathBuf::from("/elsewhere"),
                PathBuf::from("/p/b"),
            ],
            &roots,
            &[PathBuf::from("/p/x")],
        );
        assert_eq!(planned, [(1, PathBuf::from("/p/b"))]);
    }

    #[test]
    fn the_ini_queue_has_no_cap() {
        let mut pending = Pending::default();
        for i in 0..50 {
            pending.queue_ini(&PathBuf::from(format!("/p/{i}")));
        }
        assert_eq!(pending.ini.len(), 50);
        assert!(pending.dirs.is_empty());
    }

    #[test]
    fn an_ini_dir_under_a_queued_walk_is_not_added() {
        let mut pending = Pending::default();
        pending.queue_walk(Path::new("/p/a"), Path::new("/p"));
        pending.queue_ini(Path::new("/p/a/b"));
        assert!(pending.ini.is_empty());
    }

    /// Review Focus 4.
    #[test]
    fn queueing_a_walk_drops_the_ini_dirs_it_covers() {
        let mut pending = Pending::default();
        pending.queue_ini(Path::new("/p/a/b"));
        pending.queue_ini(Path::new("/p/c"));
        pending.queue_walk(Path::new("/p/a"), Path::new("/p"));
        assert_eq!(pending.ini.iter().collect::<Vec<_>>(), [Path::new("/p/c")]);
    }

    /// Review Focus 4.
    #[test]
    fn a_collapse_to_the_root_clears_the_ini_set() {
        let mut pending = Pending::default();
        pending.queue_ini(Path::new("/p/z"));
        for i in 0..=MAX_PENDING_DIRS {
            pending.queue_walk(&PathBuf::from(format!("/p/{i}")), Path::new("/p"));
        }
        assert_eq!(pending.dirs, [PathBuf::from("/p")]);
        assert!(pending.ini.is_empty());
    }

    fn failure(path: &str) -> crate::watcher::WatchError {
        crate::watcher::WatchError {
            path: PathBuf::from(path),
            message: "boom".into(),
        }
    }

    #[test]
    fn an_error_inside_one_root_degrades_only_that_root() {
        assert_eq!(
            roots_affected_by(&[failure("/photos/a")], &roots()),
            vec![1]
        );
    }

    #[test]
    fn a_pathless_or_unknown_error_degrades_every_root() {
        // No path at all (e.g. an event-queue overflow): events were lost, but not where.
        assert_eq!(roots_affected_by(&[failure("")], &roots()), vec![1, 2]);
        // A path under no watched root tells us just as little.
        assert_eq!(
            roots_affected_by(&[failure("/elsewhere/x")], &roots()),
            vec![1, 2]
        );
        assert!(roots_affected_by(&[], &roots()).is_empty());
    }
}
