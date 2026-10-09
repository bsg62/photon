//! Where the library and the thumbnail cache are: the directories the Tauri shell resolves,
//! so both applications open the same library.
//!
//! Tauri's `app_data_dir` is `dirs::data_dir()` joined with the bundle identifier, and
//! `app_cache_dir` is `dirs::cache_dir()` joined with it (tauri 2.11.5,
//! `src/path/desktop.rs`); `app.rs` then names `library.db` in the first and `thumbs` in
//! the second. A different answer here is not an error anyone sees: it is an empty library.

use std::path::{Path, PathBuf};

/// `identifier` in `crates/photon-app/tauri.conf.json`.
pub const IDENTIFIER: &str = "io.github.bsg62.photon";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    pub db_path: PathBuf,
    pub cache_dir: PathBuf,
}

/// The paths inside an app data directory and an app cache directory. This is what
/// `--data-dir` and `--cache-dir` name: the directories that hold `library.db` and `thumbs`.
pub fn within(app_data: &Path, app_cache: &Path) -> Dirs {
    Dirs {
        db_path: app_data.join("library.db"),
        cache_dir: app_cache.join("thumbs"),
    }
}

/// The standard places, or `None` on a system that has no home directory to put them in.
pub fn standard() -> Option<Dirs> {
    Some(within(
        &dirs::data_dir()?.join(IDENTIFIER),
        &dirs::cache_dir()?.join(IDENTIFIER),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_and_the_cache_are_named_as_the_tauri_shell_names_them() {
        let dirs = within(Path::new("/data/app"), Path::new("/cache/app"));
        assert_eq!(dirs.db_path, Path::new("/data/app/library.db"));
        assert_eq!(dirs.cache_dir, Path::new("/cache/app/thumbs"));
    }

    #[test]
    fn the_identifier_is_the_tauri_shells() {
        let conf = include_str!("../../photon-app/tauri.conf.json");
        assert!(
            conf.contains(&format!("\"identifier\": \"{IDENTIFIER}\"")),
            "tauri.conf.json names another identifier"
        );
    }

    /// The directory a platform keeps application data in, read from the environment the
    /// way the platform defines it and not through the `dirs` crate.
    #[cfg(target_os = "linux")]
    fn platform_dirs() -> (PathBuf, PathBuf) {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let or = |var: &str, fallback: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
        };
        (
            or("XDG_DATA_HOME", ".local/share"),
            or("XDG_CACHE_HOME", ".cache"),
        )
    }

    #[cfg(target_os = "macos")]
    fn platform_dirs() -> (PathBuf, PathBuf) {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        (
            home.join("Library/Application Support"),
            home.join("Library/Caches"),
        )
    }

    #[cfg(target_os = "windows")]
    fn platform_dirs() -> (PathBuf, PathBuf) {
        let var = |name: &str| PathBuf::from(std::env::var_os(name).unwrap());
        (var("APPDATA"), var("LOCALAPPDATA"))
    }

    // What an existing library depends on: the data is in the roaming or shared data
    // directory and the cache in the cache directory, each under the identifier.
    #[test]
    fn the_standard_places_are_the_platforms_data_and_cache_directories() {
        let (data, cache) = platform_dirs();
        let dirs = standard().unwrap();
        assert_eq!(dirs.db_path, data.join(IDENTIFIER).join("library.db"));
        assert_eq!(dirs.cache_dir, cache.join(IDENTIFIER).join("thumbs"));
    }
}
