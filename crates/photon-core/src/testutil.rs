#![allow(dead_code)]

use crate::library::{Library, NewItem};
use crate::media::MediaKind;
use std::path::Path;
use tempfile::TempDir;

/// A fresh library in its own temporary directory. Keep the `TempDir` alive for the test.
pub fn temp_library() -> (TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(&dir.path().join("library.db")).unwrap();
    (dir, lib)
}

/// Registers `path` as a watched folder with a root folder row. Returns (watched_id, folder_id).
pub fn seed_folder(lib: &Library, path: &Path) -> (i64, i64) {
    let watched = lib.add_watched_folder(path).unwrap();
    let folder = lib
        .upsert_folder(watched.id, None, path.to_str().unwrap(), 1)
        .unwrap();
    (watched.id, folder)
}

pub fn new_item(folder_id: i64, path: &str, taken_at: i64) -> NewItem {
    NewItem {
        folder_id,
        path: path.to_string(),
        file_name: Path::new(path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
        kind: MediaKind::Image,
        size: 100,
        mtime_ms: 1_000,
        width: 400,
        height: 300,
        orientation: 1,
        taken_at,
    }
}
