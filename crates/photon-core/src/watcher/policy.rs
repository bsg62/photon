use crate::paths::is_within;
use std::path::PathBuf;

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
}
