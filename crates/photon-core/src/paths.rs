//! Path comparison for watched-folder rules: component-wise, and case-insensitive on
//! the platforms whose default filesystems are (macOS, Windows). Plus the one canonical
//! form every stored path takes.

use std::path::{Component, Path, PathBuf};

/// The prefix Windows puts on a canonicalized network path: `\\?\UNC\server\share\...`.
const VERBATIM_UNC: &str = r"\\?\UNC\";

/// Like `dunce::canonicalize`, but a network share comes back as `\\server\share\...` rather
/// than in the verbatim `\\?\UNC\` form `fs::canonicalize` returns.
///
/// dunce strips the verbatim prefix only from disk paths, so before this every path under an
/// SMB share was stored, shown and revealed as `\\?\UNC\10.0.0.1\photos\...`. The Windows
/// shell rejects that form outright - `ILCreateFromPathW` returns null for it, which is the
/// "failed to convert path to ITEMIDLIST" behind a dead Reveal - and it is not what a person
/// recognises as their share either.
///
/// On non-Windows platforms no path can carry the prefix, so this is `dunce::canonicalize`.
pub fn canonicalize<P: AsRef<Path>>(path: P) -> std::io::Result<PathBuf> {
    Ok(simplified_unc(&dunce::canonicalize(path)?))
}

/// Rewrites `\\?\UNC\server\share\x` to `\\server\share\x`, and returns anything else
/// unchanged.
///
/// The rewrite is unconditional, unlike dunce's disk one, which backs out when a component
/// is a reserved DOS name or the path is longer than `MAX_PATH`. Two reasons. The length
/// limit does not bind here: `std` converts a long absolute path back to the verbatim form
/// on its way into the file APIs, so photon's own reads are not capped at 260 characters by
/// storing the short form. And the migration that rewrites libraries indexed before this
/// (schema 10) is SQL, which cannot test a component for `CON` or a trailing space: a rule
/// Rust applied and SQL could not would leave the two forms side by side in one library,
/// where `same_path` sees two different folders and `scan_subtree`'s `strip_prefix` of an
/// event directory misses its own watched root. One rule both can express is worth more
/// than the handful of share paths whose last component names a DOS device.
pub fn simplified_unc(path: &Path) -> PathBuf {
    match path.to_str().and_then(|s| s.strip_prefix(VERBATIM_UNC)) {
        Some(rest) => PathBuf::from(format!(r"\\{rest}")),
        None => path.to_path_buf(),
    }
}

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

/// True when `a` and `b` are the same folder or one contains the other. Public because an
/// export has to refuse a destination anywhere inside a watched root, not only the root.
pub fn overlaps(a: &Path, b: &Path) -> bool {
    is_within(a, b) || is_within(b, a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_verbatim_unc_path_is_simplified_to_the_share_windows_and_people_understand() {
        assert_eq!(
            simplified_unc(Path::new(r"\\?\UNC\10.0.0.1\photos\2024\a.jpg")),
            PathBuf::from(r"\\10.0.0.1\photos\2024\a.jpg")
        );
        assert_eq!(
            simplified_unc(Path::new(r"\\?\UNC\nas\photos")),
            PathBuf::from(r"\\nas\photos")
        );
    }

    #[test]
    fn nothing_else_is_touched() {
        // A verbatim *disk* path is dunce's business, not this one, and a path that merely
        // begins with a backslash pair is already the form we want.
        for path in [
            r"\\?\C:\photos",
            r"\\nas\photos",
            r"C:\photos",
            "/home/dh/photos",
            r"\\?\UNCLE\nope",
        ] {
            assert_eq!(simplified_unc(Path::new(path)), PathBuf::from(path));
        }
    }

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
}
