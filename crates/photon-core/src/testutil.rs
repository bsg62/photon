#![allow(dead_code)]

use crate::library::Library;
use tempfile::TempDir;

/// A fresh library in its own temporary directory. Keep the `TempDir` alive for the test.
pub fn temp_library() -> (TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(&dir.path().join("library.db")).unwrap();
    (dir, lib)
}
