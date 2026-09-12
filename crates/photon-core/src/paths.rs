//! Path comparison for watched-folder rules: component-wise, and case-insensitive on
//! the platforms whose default filesystems are (macOS, Windows).

use std::path::{Component, Path, PathBuf};

fn keys(path: &Path) -> Vec<String> {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| {
            let s = c.as_os_str().to_string_lossy();
            if cfg!(any(target_os = "macos", windows)) {
                s.to_lowercase()
            } else {
                s.into_owned()
            }
        })
        .collect()
}

pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    keys(a) == keys(b)
}

/// True when `child` is `parent` or lies inside it.
pub(crate) fn is_within(child: &Path, parent: &Path) -> bool {
    let (child, parent) = (keys(child), keys(parent));
    child.len() >= parent.len() && child[..parent.len()] == parent[..]
}

pub(crate) fn overlaps(a: &Path, b: &Path) -> bool {
    is_within(a, b) || is_within(b, a)
}

/// The nearest common ancestor of `a` and `b`, compared component-wise using the same case
/// rules as [`is_within`]. Neither path need be an ancestor of the other; the result is
/// their longest shared path prefix (which may be the root, or even empty on Windows if
/// they don't share a prefix at all).
pub(crate) fn common_ancestor(a: &Path, b: &Path) -> PathBuf {
    let (ka, kb) = (keys(a), keys(b));
    let shared = ka.iter().zip(kb.iter()).take_while(|(x, y)| x == y).count();
    a.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .take(shared)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn within_is_component_wise() {
        assert!(is_within(Path::new("/a/b"), Path::new("/a")));
        assert!(is_within(Path::new("/a"), Path::new("/a")));
        assert!(!is_within(Path::new("/ab"), Path::new("/a")));
        assert!(!is_within(Path::new("/a"), Path::new("/a/b")));
        assert!(overlaps(Path::new("/a"), Path::new("/a/b")));
        assert!(overlaps(Path::new("/a/b"), Path::new("/a")));
        assert!(!overlaps(Path::new("/a/b"), Path::new("/a/c")));
        assert!(same_path(Path::new("/a/./b"), Path::new("/a/b")));
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn comparison_ignores_case_on_case_insensitive_platforms() {
        assert!(same_path(Path::new("/A/b"), Path::new("/a/B")));
        assert!(is_within(Path::new("/Photos/2024"), Path::new("/photos")));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn comparison_is_case_sensitive_elsewhere() {
        assert!(!same_path(Path::new("/A"), Path::new("/a")));
        assert!(!is_within(Path::new("/Photos/2024"), Path::new("/photos")));
    }

    #[test]
    fn common_ancestor_of_siblings_is_their_parent() {
        assert_eq!(
            common_ancestor(Path::new("/p/a"), Path::new("/p/b")),
            Path::new("/p")
        );
        assert_eq!(
            common_ancestor(Path::new("/p/a/deep"), Path::new("/p/b/deeper")),
            Path::new("/p")
        );
    }

    #[test]
    fn common_ancestor_of_an_ancestor_and_its_descendant_is_the_ancestor() {
        assert_eq!(
            common_ancestor(Path::new("/p/a"), Path::new("/p/a/b")),
            Path::new("/p/a")
        );
    }
}
