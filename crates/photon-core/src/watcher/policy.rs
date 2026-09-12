use crate::paths::{common_ancestor, is_within};
use std::path::{Path, PathBuf};

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

/// Merges two pending subtree-scan requests for the same watched folder into one directory
/// that covers both.
///
/// When one request is an ancestor of the other (or they're equal), keeps the ancestor:
/// scanning it already covers the descendant. Otherwise, since a single request can't cover
/// two unrelated branches, falls back to their nearest common ancestor — clamped to `root`
/// (the watched folder's own path) in case it somehow comes out shallower, so the merged
/// request never rises above the folder actually being watched. `root` itself is always a
/// valid answer: it simply becomes a full rescan of the folder.
pub fn merge_pending(existing: &Path, new: &Path, root: &Path) -> PathBuf {
    if is_within(new, existing) {
        return existing.to_path_buf();
    }
    if is_within(existing, new) {
        return new.to_path_buf();
    }
    let merged = common_ancestor(existing, new);
    if is_within(&merged, root) {
        merged
    } else {
        root.to_path_buf()
    }
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

    #[test]
    fn merge_pending_keeps_the_ancestor_of_a_nested_pair() {
        let root = PathBuf::from("/photos");
        assert_eq!(
            merge_pending(Path::new("/photos/a"), Path::new("/photos/a/deep"), &root),
            PathBuf::from("/photos/a")
        );
        assert_eq!(
            merge_pending(Path::new("/photos/a/deep"), Path::new("/photos/a"), &root),
            PathBuf::from("/photos/a")
        );
    }

    #[test]
    fn merge_pending_of_siblings_is_their_common_ancestor() {
        let root = PathBuf::from("/photos");
        assert_eq!(
            merge_pending(Path::new("/photos/a"), Path::new("/photos/b"), &root),
            PathBuf::from("/photos")
        );
    }

    #[test]
    fn merge_pending_never_rises_above_the_watched_root() {
        // Two directories that share no prefix beneath the root: the merge is clamped to
        // the root itself rather than the (nonsensical, or empty) raw common ancestor.
        let root = PathBuf::from("/photos");
        assert_eq!(
            merge_pending(Path::new("/photos/a"), Path::new("/elsewhere/b"), &root),
            PathBuf::from("/photos")
        );
    }
}
