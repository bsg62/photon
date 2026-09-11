# photon Core Engine (Plan 1 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `photon-core`, the headless Rust engine for photon v1: SQLite library, folder scanner, EXIF metadata, image decoding, a prioritized thumbnail pipeline and the in-memory grid index. It ends with a runnable `index` example and benchmarks.

**Architecture:** A single library crate with no Tauri dependency, so every unit is tested headless. `library` is the only module that runs SQL. `scanner` diffs the filesystem against it, `thumbs` turns items into cached WebP files through a priority queue and a worker pool, and `grid` holds the ordered in-memory index the UI will page through.

**Tech Stack:** Rust (edition 2024), rusqlite (bundled SQLite), image 0.25, kamadak-exif, webp (libwebp), walkdir, parking_lot, xxhash-rust, thiserror, tracing, serde, criterion.

**Spec:** `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`

**Roadmap:** v1 is split into three plans, each producing working software:
1. **This plan: core engine.** Headless and fully tested. Common formats (JPEG/PNG/GIF/WebP).
2. **App + UI.** The Tauri shell (`photon-app`), the `photon://` protocol, IPC and events, the Svelte folder tree, virtualized grid and viewer, and startup rescan.
3. **Formats, watching, shipping.** HEIC/AVIF (libheif), video (ffmpeg poster frames plus `photon://video` range streaming), the `notify` filesystem watcher, and packaging (AppImage/deb, dmg, msi) with bundled native libraries.

Plans 2 and 3 are written after this plan lands, against the real interfaces produced here.

## Global Constraints

- Platforms: Linux, macOS, Windows. Every test must pass on all three, so no hard-coded `/` path assumptions in assertions; build expected paths with `Path::join`.
- photon never writes to, moves or deletes files inside watched folders. Only the database file and the thumbnail cache directory are written.
- Thumbnail sizes: grid `256` px, preview `1600` px (longest edge, aspect preserved, never upscaled), WebP quality `85`.
- Thumbnail cache key: `xxh3_64(path bytes ‖ size (i64 LE) ‖ mtime_ms (i64 LE))`, as 16 lowercase hex characters.
- The scanner writes to the database in batches of `500` rows per transaction.
- A missing file is soft-deleted (`missing_since`) first and hard-deleted only by a later scan of a *reachable* folder.
- A failed thumbnail is not retried until the file's fingerprint changes.
- Thumbnail workers: CPU cores − 1 (minimum 1).
- Performance budgets (100k items): `grid_rows` page query under 50ms; loading grid data from a warm database under 1s.
- Rust edition 2024, stable toolchain. Every task ends with `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Out of scope for this plan: HEIC/AVIF, video, RAW, fs watching, edits, albums, tags, XMP.
- Commit messages follow Conventional Commits (`feat:`, `test:`, `chore:`, …).

## File Structure

```
Cargo.toml                         workspace root
mise.toml                          pins the Rust toolchain
.gitignore
.github/workflows/ci.yml           Linux/macOS/Windows fmt + clippy + test
crates/photon-core/
  Cargo.toml
  src/lib.rs                       module declarations, re-exports, now_ms()
  src/error.rs                     Error enum + Result alias
  src/media.rs                     MediaKind, ThumbState, fingerprint()
  src/library/mod.rs               Library (writer + reader connections), open()
  src/library/schema.rs            user_version-based migrations
  src/library/folders.rs           watched folders + folder tree queries
  src/library/items.rs             media item queries (+ grid_entries)
  src/metadata.rs                  EXIF orientation/date, dimensions
  src/decode.rs                    decode + resize + apply EXIF orientation
  src/thumbs/mod.rs                re-exports
  src/thumbs/cache.rs              ThumbCache: paths, generation, GC
  src/thumbs/queue.rs              ThumbQueue: priority queue with idle tracking
  src/thumbs/service.rs            ThumbService: worker pool, on-demand generation
  src/scanner.rs                   scan_watched(): walk + diff + batched writes
  src/grid.rs                      GridEntry, Section, GridIndex
  src/testutil.rs                  test-only fixtures (cfg(test))
  benches/grid.rs                  100k-item grid benchmarks
  examples/index.rs                end-to-end CLI: scan a folder, build thumbnails
```

---

### Task 1: Workspace scaffold, error types, media types, library open and watched folders

**Files:**
- Create: `mise.toml`, `.gitignore`, `Cargo.toml`
- Create: `crates/photon-core/Cargo.toml`
- Create: `crates/photon-core/src/lib.rs`, `src/error.rs`, `src/media.rs`, `src/testutil.rs`
- Create: `crates/photon-core/src/library/mod.rs`, `src/library/schema.rs`, `src/library/folders.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `photon_core::{Error, Result}`, where `Result<T, E = Error>`. The variants are `Db`, `Io`, `Image`, `NonUtf8Path(PathBuf)`, `SchemaTooNew { found: i64, supported: i64 }`, `NotFound(i64)` and `ThumbFailed(String)`.
  - `photon_core::now_ms() -> i64`.
  - `media::MediaKind { Image }`, with `from_path(&Path) -> Option<MediaKind>`, `to_db(self) -> i64` and `from_db(i64) -> Option<MediaKind>`.
  - `media::ThumbState { Pending, Ready, Failed }`, with `to_db(self) -> i64` and `from_db(i64) -> ThumbState`.
  - `media::fingerprint(path: &str, size: i64, mtime_ms: i64) -> u64`.
  - `library::Library`, with `open(&Path) -> Result<Library>`.
  - `library::WatchedFolder { id: i64, path: String, online: bool }`.
  - Library methods: `add_watched_folder(&Path) -> Result<WatchedFolder>`, `watched_folders() -> Result<Vec<WatchedFolder>>`, `set_watched_online(i64, bool) -> Result<()>` and `remove_watched_folder(i64) -> Result<()>`.
  - `testutil::temp_library() -> (TempDir, Library)`.

- [ ] **Step 1: Install the toolchain and create the workspace files**

Run: `mise use rust@stable && cargo --version`
Expected: this creates `mise.toml` with `[tools] rust = "stable"` and prints a cargo version of 1.88 or newer (let-chains need 1.88).

Create `.gitignore`:

```gitignore
/target
**/*.rs.bk
.DS_Store
```

Create `Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/photon-core"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.88"
```

Create `crates/photon-core/Cargo.toml`:

```toml
[package]
name = "photon-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
image = { version = "0.25.10", default-features = false, features = ["jpeg", "png", "gif", "webp"] }
kamadak-exif = "0.6.1"
parking_lot = "0.12.5"
rusqlite = { version = "0.40.2", features = ["bundled"] }
serde = { version = "1.0.229", features = ["derive"] }
tempfile = "3.27.0"
thiserror = "2.0.20"
tracing = "0.1.44"
walkdir = "2.5.0"
webp = "0.3.1"
xxhash-rust = { version = "0.8.18", features = ["xxh3"] }

[dev-dependencies]
criterion = "0.8.2"
```

- [ ] **Step 2: Write `error.rs`, `lib.rs` and `testutil.rs`**

`crates/photon-core/src/error.rs`:

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("path is not valid UTF-8: {0:?}")]
    NonUtf8Path(PathBuf),
    #[error("library schema version {found} is newer than supported version {supported}")]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("item {0} not found")]
    NotFound(i64),
    #[error("thumbnail generation failed: {0}")]
    ThumbFailed(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
```

`crates/photon-core/src/lib.rs`:

```rust
//! photon-core: headless library, scanning and thumbnail engine for photon.

pub mod error;
pub mod library;
pub mod media;

#[cfg(test)]
mod testutil;

pub use error::{Error, Result};

/// Current wall-clock time in milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

`crates/photon-core/src/testutil.rs`:

```rust
#![allow(dead_code)]

use crate::library::Library;
use tempfile::TempDir;

/// A fresh library in its own temporary directory. Keep the `TempDir` alive for the test.
pub fn temp_library() -> (TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(&dir.path().join("library.db")).unwrap();
    (dir, lib)
}
```

- [ ] **Step 3: Write failing tests for media types**

Create `crates/photon-core/src/media.rs` with only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn recognises_common_image_extensions_case_insensitively() {
        for name in ["a.jpg", "a.JPEG", "a.jpe", "a.png", "a.gif", "a.WebP"] {
            assert_eq!(MediaKind::from_path(Path::new(name)), Some(MediaKind::Image), "{name}");
        }
        for name in ["a.txt", "a.heic", "a", ".jpg"] {
            assert_eq!(MediaKind::from_path(Path::new(name)), None, "{name}");
        }
    }

    #[test]
    fn db_round_trips() {
        assert_eq!(MediaKind::from_db(MediaKind::Image.to_db()), Some(MediaKind::Image));
        assert_eq!(MediaKind::from_db(42), None);
        for s in [ThumbState::Pending, ThumbState::Ready, ThumbState::Failed] {
            assert_eq!(ThumbState::from_db(s.to_db()), s);
        }
    }

    #[test]
    fn fingerprint_changes_with_every_input() {
        let base = fingerprint("/p/a.jpg", 100, 1_000);
        assert_eq!(base, fingerprint("/p/a.jpg", 100, 1_000));
        assert_ne!(base, fingerprint("/p/b.jpg", 100, 1_000));
        assert_ne!(base, fingerprint("/p/a.jpg", 101, 1_000));
        assert_ne!(base, fingerprint("/p/a.jpg", 100, 1_001));
    }
}
```

Note: `Path::new(".jpg").extension()` is `None` (it is treated as a hidden file name), so `.jpg` is expected to be `None`.

- [ ] **Step 4: Run the tests and confirm they fail**

Run: `cargo test -p photon-core media`
Expected: compile error, because `MediaKind`, `ThumbState` and `fingerprint` are not defined. (It may also fail because `library` does not exist yet. That is created in Step 7, so creating an empty `src/library/mod.rs` now to get past it is fine.)

- [ ] **Step 5: Implement `media.rs`**

Prepend the following to `crates/photon-core/src/media.rs`, above the tests:

```rust
use serde::Serialize;
use std::path::Path;

/// What kind of media a library item is. Plan 3 adds `Video`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
}

impl MediaKind {
    /// Classifies a file by its extension; `None` means photon ignores the file.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "jpg" | "jpeg" | "jpe" | "png" | "gif" | "webp" => Some(Self::Image),
            _ => None,
        }
    }

    pub fn to_db(self) -> i64 {
        match self {
            Self::Image => 0,
        }
    }

    pub fn from_db(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Image),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbState {
    Pending,
    Ready,
    Failed,
}

impl ThumbState {
    pub fn to_db(self) -> i64 {
        match self {
            Self::Pending => 0,
            Self::Ready => 1,
            Self::Failed => 2,
        }
    }

    pub fn from_db(value: i64) -> Self {
        match value {
            1 => Self::Ready,
            2 => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// Content fingerprint used as the thumbnail cache key.
pub fn fingerprint(path: &str, size: i64, mtime_ms: i64) -> u64 {
    let mut buf = Vec::with_capacity(path.len() + 16);
    buf.extend_from_slice(path.as_bytes());
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(&mtime_ms.to_le_bytes());
    xxhash_rust::xxh3::xxh3_64(&buf)
}
```

- [ ] **Step 6: Write failing tests for opening the library and managing watched folders**

Create `crates/photon-core/src/library/mod.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, testutil::temp_library};
    use std::path::Path;

    #[test]
    fn open_creates_schema_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("library.db");
        drop(Library::open(&path).unwrap());
        let lib = Library::open(&path).unwrap();
        let version: i64 = lib
            .reader()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
        let tables: i64 = lib
            .reader()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('watched_folders', 'folders', 'items')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 3);
    }

    #[test]
    fn refuses_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        drop(Library::open(&path).unwrap());
        rusqlite::Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        assert!(matches!(
            Library::open(&path),
            Err(Error::SchemaTooNew { found: 99, supported: 1 })
        ));
    }

    #[test]
    fn watched_folder_lifecycle() {
        let (_dir, lib) = temp_library();
        let a = lib.add_watched_folder(Path::new("/photos/a")).unwrap();
        let again = lib.add_watched_folder(Path::new("/photos/a")).unwrap();
        assert_eq!(a, again);
        assert!(a.online);
        let b = lib.add_watched_folder(Path::new("/photos/b")).unwrap();

        lib.set_watched_online(b.id, false).unwrap();
        let all = lib.watched_folders().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1], WatchedFolder { id: b.id, path: "/photos/b".into(), online: false });

        lib.remove_watched_folder(a.id).unwrap();
        assert_eq!(lib.watched_folders().unwrap().len(), 1);
    }
}
```

- [ ] **Step 7: Run the tests and confirm they fail**

Run: `cargo test -p photon-core library`
Expected: compile error, because `Library` and `WatchedFolder` are not defined.

- [ ] **Step 8: Implement the schema, `Library` and watched folders**

`crates/photon-core/src/library/schema.rs`:

```rust
use crate::{Error, Result};
use rusqlite::Connection;

/// Each entry upgrades the schema by one version; `PRAGMA user_version` records the current one.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE watched_folders (
    id     INTEGER PRIMARY KEY,
    path   TEXT NOT NULL UNIQUE,
    online INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE folders (
    id         INTEGER PRIMARY KEY,
    watched_id INTEGER NOT NULL REFERENCES watched_folders(id) ON DELETE CASCADE,
    parent_id  INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    path       TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    sort_key   TEXT NOT NULL,
    seen_scan  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX folders_watched ON folders(watched_id);
CREATE TABLE items (
    id            INTEGER PRIMARY KEY,
    folder_id     INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    path          TEXT NOT NULL UNIQUE,
    file_name     TEXT NOT NULL,
    kind          INTEGER NOT NULL,
    size          INTEGER NOT NULL,
    mtime_ms      INTEGER NOT NULL,
    width         INTEGER NOT NULL,
    height        INTEGER NOT NULL,
    orientation   INTEGER NOT NULL,
    taken_at      INTEGER NOT NULL,
    thumb_state   INTEGER NOT NULL DEFAULT 0,
    thumb_error   TEXT,
    missing_since INTEGER
);
CREATE INDEX items_folder ON items(folder_id, taken_at);
CREATE INDEX items_pending ON items(thumb_state) WHERE missing_since IS NULL;
"#];

pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let supported = MIGRATIONS.len() as i64;
    if current > supported {
        return Err(Error::SchemaTooNew { found: current, supported });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}
```

`crates/photon-core/src/library/mod.rs` (above the tests):

```rust
mod folders;
mod schema;

pub use folders::WatchedFolder;

use crate::Result;
use parking_lot::{Mutex, MutexGuard};
use rusqlite::Connection;
use std::path::Path;

/// The photon library database. One connection is reserved for writes, a second
/// serves reads so the UI can query while a scan is writing (SQLite WAL mode).
pub struct Library {
    write: Mutex<Connection>,
    read: Mutex<Connection>,
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let write = Connection::open(path)?;
        configure(&write)?;
        schema::migrate(&write)?;
        let read = Connection::open(path)?;
        configure(&read)?;
        Ok(Self { write: Mutex::new(write), read: Mutex::new(read) })
    }

    fn writer(&self) -> MutexGuard<'_, Connection> {
        self.write.lock()
    }

    fn reader(&self) -> MutexGuard<'_, Connection> {
        self.read.lock()
    }
}

fn configure(conn: &Connection) -> Result<()> {
    // journal_mode returns a row, so it cannot go through execute_batch.
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;")?;
    Ok(())
}
```

`crates/photon-core/src/library/folders.rs`:

```rust
use super::Library;
use crate::{Error, Result};
use rusqlite::{Row, params};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WatchedFolder {
    pub id: i64,
    pub path: String,
    pub online: bool,
}

impl Library {
    /// Registers a folder to watch. Adding the same path twice returns the existing entry.
    pub fn add_watched_folder(&self, path: &Path) -> Result<WatchedFolder> {
        let path_str = path.to_str().ok_or_else(|| Error::NonUtf8Path(path.to_path_buf()))?;
        let conn = self.writer();
        conn.execute("INSERT OR IGNORE INTO watched_folders (path) VALUES (?1)", params![path_str])?;
        let watched = conn.query_row(
            "SELECT id, path, online FROM watched_folders WHERE path = ?1",
            params![path_str],
            row_to_watched,
        )?;
        Ok(watched)
    }

    pub fn watched_folders(&self) -> Result<Vec<WatchedFolder>> {
        let conn = self.reader();
        let mut stmt = conn.prepare("SELECT id, path, online FROM watched_folders ORDER BY path")?;
        let rows = stmt.query_map([], row_to_watched)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_watched_online(&self, id: i64, online: bool) -> Result<()> {
        self.writer()
            .execute("UPDATE watched_folders SET online = ?2 WHERE id = ?1", params![id, online])?;
        Ok(())
    }

    /// Forgets a watched folder and everything indexed under it. Files on disk are untouched.
    pub fn remove_watched_folder(&self, id: i64) -> Result<()> {
        self.writer().execute("DELETE FROM watched_folders WHERE id = ?1", params![id])?;
        Ok(())
    }
}

fn row_to_watched(row: &Row<'_>) -> rusqlite::Result<WatchedFolder> {
    Ok(WatchedFolder { id: row.get(0)?, path: row.get(1)?, online: row.get(2)? })
}
```

- [ ] **Step 9: Run all tests and confirm they pass**

Run: `cargo test -p photon-core`
Expected: PASS, 6 tests.

- [ ] **Step 10: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add mise.toml .gitignore Cargo.toml Cargo.lock crates/
git commit -m "feat(core): scaffold workspace with library database and watched folders"
```

---

### Task 2: Folder tree and media item queries

**Files:**
- Modify: `crates/photon-core/src/library/folders.rs` (add `Folder`, `upsert_folder`, `prune_folders`, `folders`, `sort_key`)
- Create: `crates/photon-core/src/library/items.rs`
- Modify: `crates/photon-core/src/library/mod.rs` (add `mod items;` and re-exports)
- Modify: `crates/photon-core/src/testutil.rs` (add `seed_folder` and `new_item`)

**Interfaces:**
- Consumes: `Library`, `media::{MediaKind, ThumbState, fingerprint}` from Task 1.
- Produces:
  - `library::Folder { id: i64, watched_id: i64, parent_id: Option<i64>, path: String, name: String }`.
  - `upsert_folder(&self, watched_id: i64, parent_id: Option<i64>, path: &str, scan_id: i64) -> Result<i64>`.
  - `prune_folders(&self, watched_id: i64, scan_id: i64) -> Result<usize>`.
  - `folders(&self) -> Result<Vec<Folder>>`, in tree order.
  - `library::NewItem { folder_id: i64, path: String, file_name: String, kind: MediaKind, size: i64, mtime_ms: i64, width: u32, height: u32, orientation: u8, taken_at: i64 }`.
  - `library::KnownItem { id: i64, size: i64, mtime_ms: i64, missing: bool }`.
  - `library::Item { id, folder_id, path: String, kind, size, mtime_ms, width: u32, height: u32, orientation: u8, taken_at, thumb_state: ThumbState, thumb_error: Option<String>, missing_since: Option<i64> }`, with `Item::fingerprint(&self) -> u64`.
  - `insert_items(&[NewItem]) -> Result<Vec<i64>>` and `update_items(&[(i64, NewItem)]) -> Result<()>`. Updating resets the thumbnail state and clears `missing_since`.
  - `mark_missing(&[i64], now_ms: i64) -> Result<()>` and `purge_items(&[i64]) -> Result<()>`.
  - `known_items(watched_id: i64) -> Result<HashMap<String, KnownItem>>` and `item(id: i64) -> Result<Option<Item>>`.
  - `set_thumb_state(id: i64, state: ThumbState, error: Option<&str>) -> Result<()>`.
  - `pending_thumb_ids() -> Result<Vec<i64>>`, in grid order (folder sort key, then `taken_at`, then file name), excluding missing items.
  - `live_fingerprints() -> Result<HashSet<u64>>`.
  - `testutil::seed_folder(&Library, &Path) -> (i64 /*watched*/, i64 /*folder*/)` and `testutil::new_item(folder_id: i64, path: &str, taken_at: i64) -> NewItem`.

- [ ] **Step 1: Add the test helpers**

Append to `crates/photon-core/src/testutil.rs`:

```rust
use crate::library::NewItem;
use crate::media::MediaKind;
use std::path::Path;

/// Registers `path` as a watched folder with a root folder row. Returns (watched_id, folder_id).
pub fn seed_folder(lib: &Library, path: &Path) -> (i64, i64) {
    let watched = lib.add_watched_folder(path).unwrap();
    let folder = lib.upsert_folder(watched.id, None, path.to_str().unwrap(), 1).unwrap();
    (watched.id, folder)
}

pub fn new_item(folder_id: i64, path: &str, taken_at: i64) -> NewItem {
    NewItem {
        folder_id,
        path: path.to_string(),
        file_name: Path::new(path).file_name().unwrap().to_str().unwrap().to_string(),
        kind: MediaKind::Image,
        size: 100,
        mtime_ms: 1_000,
        width: 400,
        height: 300,
        orientation: 1,
        taken_at,
    }
}
```

Merge the `use` lines with the existing ones at the top of the file.

- [ ] **Step 2: Write failing folder tests**

Append to `crates/photon-core/src/library/folders.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{new_item, temp_library};

    #[test]
    fn upsert_folder_is_idempotent_and_tracks_parent() {
        let (_dir, lib) = temp_library();
        let w = lib.add_watched_folder(Path::new("/photos")).unwrap();
        let root = lib.upsert_folder(w.id, None, "/photos", 1).unwrap();
        let child = lib.upsert_folder(w.id, Some(root), "/photos/2024", 1).unwrap();
        assert_eq!(lib.upsert_folder(w.id, Some(root), "/photos/2024", 2).unwrap(), child);

        let folders = lib.folders().unwrap();
        assert_eq!(folders.len(), 2);
        assert_eq!(
            folders[1],
            Folder {
                id: child,
                watched_id: w.id,
                parent_id: Some(root),
                path: "/photos/2024".into(),
                name: "2024".into(),
            }
        );
    }

    #[test]
    fn folders_are_listed_in_tree_order() {
        let (_dir, lib) = temp_library();
        let w = lib.add_watched_folder(Path::new("/p")).unwrap();
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/a b", 1).unwrap();
        let a = lib.upsert_folder(w.id, Some(root), "/p/a", 1).unwrap();
        lib.upsert_folder(w.id, Some(a), "/p/a/z", 1).unwrap();

        let names: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["p", "a", "z", "a b"]);
    }

    #[test]
    fn prune_removes_only_unseen_empty_leaf_folders() {
        let (_dir, lib) = temp_library();
        let w = lib.add_watched_folder(Path::new("/p")).unwrap();
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/gone", 1).unwrap();
        let kept = lib.upsert_folder(w.id, Some(root), "/p/kept", 1).unwrap();
        let parent = lib.upsert_folder(w.id, Some(root), "/p/parent", 1).unwrap();
        let child = lib.upsert_folder(w.id, Some(parent), "/p/parent/child", 1).unwrap();
        lib.insert_items(&[new_item(kept, "/p/kept/a.jpg", 0), new_item(child, "/p/parent/child/b.jpg", 0)])
            .unwrap();

        // Second scan only saw the root.
        lib.upsert_folder(w.id, None, "/p", 2).unwrap();
        assert_eq!(lib.prune_folders(w.id, 2).unwrap(), 1);

        let paths: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["/p", "/p/kept", "/p/parent", "/p/parent/child"]);
    }
}
```

- [ ] **Step 3: Write failing item tests**

Create `crates/photon-core/src/library/items.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    #[test]
    fn insert_and_read_back_item() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib.insert_items(&[new_item(folder, "/p/a.jpg", 10)]).unwrap();

        let item = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(item.path, "/p/a.jpg");
        assert_eq!(item.folder_id, folder);
        assert_eq!((item.width, item.height, item.orientation, item.taken_at), (400, 300, 1, 10));
        assert_eq!(item.thumb_state, ThumbState::Pending);
        assert_eq!(item.missing_since, None);
        assert_eq!(item.fingerprint(), fingerprint("/p/a.jpg", 100, 1_000));
        assert!(lib.item(9_999).unwrap().is_none());
    }

    #[test]
    fn known_items_track_missing_update_and_purge() {
        let (_dir, lib) = temp_library();
        let (watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[new_item(folder, "/p/a.jpg", 1), new_item(folder, "/p/b.jpg", 2)])
            .unwrap();
        let (a, b) = (ids[0], ids[1]);

        let known = lib.known_items(watched).unwrap();
        assert_eq!(known.len(), 2);
        assert_eq!(known["/p/a.jpg"], KnownItem { id: a, size: 100, mtime_ms: 1_000, missing: false });

        lib.mark_missing(&[a], 50).unwrap();
        assert!(lib.known_items(watched).unwrap()["/p/a.jpg"].missing);
        assert_eq!(lib.item(a).unwrap().unwrap().missing_since, Some(50));

        lib.set_thumb_state(a, ThumbState::Ready, None).unwrap();
        let changed = NewItem { size: 200, ..new_item(folder, "/p/a.jpg", 1) };
        lib.update_items(&[(a, changed)]).unwrap();
        let item = lib.item(a).unwrap().unwrap();
        assert_eq!((item.size, item.missing_since, item.thumb_state), (200, None, ThumbState::Pending));

        lib.purge_items(&[b]).unwrap();
        assert!(lib.item(b).unwrap().is_none());
    }

    #[test]
    fn thumb_state_records_errors() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap()[0];
        lib.set_thumb_state(id, ThumbState::Failed, Some("corrupt")).unwrap();
        let item = lib.item(id).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Failed);
        assert_eq!(item.thumb_error.as_deref(), Some("corrupt"));
    }

    #[test]
    fn pending_ids_follow_grid_order_and_skip_done_or_missing() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(b, "/p/b/1.jpg", 1),
                new_item(a, "/p/a/2.jpg", 5),
                new_item(a, "/p/a/1.jpg", 2),
            ])
            .unwrap();
        let (b1, a2, a1) = (ids[0], ids[1], ids[2]);
        assert_eq!(lib.pending_thumb_ids().unwrap(), [a1, a2, b1]);

        lib.set_thumb_state(a1, ThumbState::Ready, None).unwrap();
        lib.mark_missing(&[b1], 99).unwrap();
        assert_eq!(lib.pending_thumb_ids().unwrap(), [a2]);
    }

    #[test]
    fn live_fingerprints_cover_all_items() {
        let (_dir, lib) = temp_library();
        let (_, folder) = seed_folder(&lib, Path::new("/p"));
        let id = lib.insert_items(&[new_item(folder, "/p/a.jpg", 1)]).unwrap()[0];
        let expected = lib.item(id).unwrap().unwrap().fingerprint();
        assert_eq!(lib.live_fingerprints().unwrap(), HashSet::from([expected]));
    }
}
```

Add `mod items;` and `pub use items::{Item, KnownItem, NewItem};` to `library/mod.rs`, and change `pub use folders::WatchedFolder;` to `pub use folders::{Folder, WatchedFolder};`.

- [ ] **Step 4: Run the tests and confirm they fail**

Run: `cargo test -p photon-core library`
Expected: compile errors, because `Folder`, `upsert_folder`, `NewItem` and the rest are not defined.

- [ ] **Step 5: Implement the folder queries**

Add to `crates/photon-core/src/library/folders.rs`, above the tests (extend the existing `impl Library` block with the methods):

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Folder {
    pub id: i64,
    pub watched_id: i64,
    pub parent_id: Option<i64>,
    pub path: String,
    pub name: String,
}

impl Library {
    /// Inserts a folder, or refreshes its parent and scan marker if it already exists.
    pub fn upsert_folder(&self, watched_id: i64, parent_id: Option<i64>, path: &str, scan_id: i64) -> Result<i64> {
        let name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();
        let id = self.writer().query_row(
            "INSERT INTO folders (watched_id, parent_id, path, name, sort_key, seen_scan)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET parent_id = excluded.parent_id, seen_scan = excluded.seen_scan
             RETURNING id",
            params![watched_id, parent_id, path, name, sort_key(path), scan_id],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Deletes folders not seen in scan `scan_id` that hold no items and no subfolders.
    /// Folders still holding soft-deleted items survive until those items are purged.
    pub fn prune_folders(&self, watched_id: i64, scan_id: i64) -> Result<usize> {
        let conn = self.writer();
        let mut total = 0;
        loop {
            let removed = conn.execute(
                "DELETE FROM folders
                 WHERE watched_id = ?1 AND seen_scan < ?2
                   AND NOT EXISTS (SELECT 1 FROM items WHERE items.folder_id = folders.id)
                   AND NOT EXISTS (SELECT 1 FROM folders c WHERE c.parent_id = folders.id)",
                params![watched_id, scan_id],
            )?;
            if removed == 0 {
                return Ok(total);
            }
            total += removed;
        }
    }

    /// All folders in tree order (parents before children, siblings alphabetical).
    pub fn folders(&self) -> Result<Vec<Folder>> {
        let conn = self.reader();
        let mut stmt =
            conn.prepare("SELECT id, watched_id, parent_id, path, name FROM folders ORDER BY sort_key")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Folder {
                    id: r.get(0)?,
                    watched_id: r.get(1)?,
                    parent_id: r.get(2)?,
                    path: r.get(3)?,
                    name: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// Case-insensitive key whose byte order is depth-first tree order: separators map to
/// \u{1}, which sorts below every printable character, so "/p/a/z" < "/p/a b".
pub(crate) fn sort_key(path: &str) -> String {
    path.to_lowercase().replace(['/', '\\'], "\u{1}")
}
```

- [ ] **Step 6: Implement the item queries**

Prepend to `crates/photon-core/src/library/items.rs`:

```rust
use super::Library;
use crate::Result;
use crate::media::{MediaKind, ThumbState, fingerprint};
use rusqlite::{OptionalExtension, Row, params};
use std::collections::{HashMap, HashSet};

/// A file discovered by the scanner, ready to be inserted or to replace an existing row.
#[derive(Clone, Debug, PartialEq)]
pub struct NewItem {
    pub folder_id: i64,
    pub path: String,
    pub file_name: String,
    pub kind: MediaKind,
    pub size: i64,
    pub mtime_ms: i64,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
}

/// What the scanner needs to know about an indexed file to detect changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownItem {
    pub id: i64,
    pub size: i64,
    pub mtime_ms: i64,
    pub missing: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: i64,
    pub folder_id: i64,
    pub path: String,
    pub kind: MediaKind,
    pub size: i64,
    pub mtime_ms: i64,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
    pub thumb_state: ThumbState,
    pub thumb_error: Option<String>,
    pub missing_since: Option<i64>,
}

impl Item {
    pub fn fingerprint(&self) -> u64 {
        fingerprint(&self.path, self.size, self.mtime_ms)
    }
}

/// Grid order, shared by every query that walks items the way the grid shows them.
pub(crate) const GRID_ORDER: &str = "ORDER BY f.sort_key, i.taken_at, i.file_name";

fn row_to_item(r: &Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        path: r.get(2)?,
        kind: MediaKind::from_db(r.get(3)?).unwrap_or(MediaKind::Image),
        size: r.get(4)?,
        mtime_ms: r.get(5)?,
        width: r.get(6)?,
        height: r.get(7)?,
        orientation: r.get(8)?,
        taken_at: r.get(9)?,
        thumb_state: ThumbState::from_db(r.get(10)?),
        thumb_error: r.get(11)?,
        missing_since: r.get(12)?,
    })
}

impl Library {
    pub fn insert_items(&self, items: &[NewItem]) -> Result<Vec<i64>> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let mut ids = Vec::with_capacity(items.len());
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO items (folder_id, path, file_name, kind, size, mtime_ms, width, height, orientation, taken_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for it in items {
                stmt.execute(params![
                    it.folder_id, it.path, it.file_name, it.kind.to_db(), it.size,
                    it.mtime_ms, it.width, it.height, it.orientation, it.taken_at
                ])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Replaces changed (or reappeared) items. Their thumbnails must be rebuilt.
    pub fn update_items(&self, items: &[(i64, NewItem)]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE items SET folder_id = ?2, path = ?3, file_name = ?4, kind = ?5, size = ?6, mtime_ms = ?7,
                        width = ?8, height = ?9, orientation = ?10, taken_at = ?11,
                        thumb_state = 0, thumb_error = NULL, missing_since = NULL
                 WHERE id = ?1",
            )?;
            for (id, it) in items {
                stmt.execute(params![
                    id, it.folder_id, it.path, it.file_name, it.kind.to_db(), it.size,
                    it.mtime_ms, it.width, it.height, it.orientation, it.taken_at
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Soft-deletes items; they stay hidden until a later scan purges or restores them.
    pub fn mark_missing(&self, ids: &[i64], now_ms: i64) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt =
                tx.prepare_cached("UPDATE items SET missing_since = ?2 WHERE id = ?1 AND missing_since IS NULL")?;
            for id in ids {
                stmt.execute(params![id, now_ms])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn purge_items(&self, ids: &[i64]) -> Result<()> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM items WHERE id = ?1")?;
            for id in ids {
                stmt.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every item under a watched folder, keyed by path, including soft-deleted ones.
    pub fn known_items(&self, watched_id: i64) -> Result<HashMap<String, KnownItem>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(
            "SELECT i.path, i.id, i.size, i.mtime_ms, i.missing_since IS NOT NULL
             FROM items i JOIN folders f ON f.id = i.folder_id WHERE f.watched_id = ?1",
        )?;
        let rows = stmt
            .query_map(params![watched_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    KnownItem { id: r.get(1)?, size: r.get(2)?, mtime_ms: r.get(3)?, missing: r.get(4)? },
                ))
            })?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }

    pub fn item(&self, id: i64) -> Result<Option<Item>> {
        let item = self
            .reader()
            .query_row(
                "SELECT id, folder_id, path, kind, size, mtime_ms, width, height, orientation, taken_at,
                        thumb_state, thumb_error, missing_since
                 FROM items WHERE id = ?1",
                params![id],
                row_to_item,
            )
            .optional()?;
        Ok(item)
    }

    pub fn set_thumb_state(&self, id: i64, state: ThumbState, error: Option<&str>) -> Result<()> {
        self.writer().execute(
            "UPDATE items SET thumb_state = ?2, thumb_error = ?3 WHERE id = ?1",
            params![id, state.to_db(), error],
        )?;
        Ok(())
    }

    /// Items still waiting for thumbnails, in grid order.
    pub fn pending_thumb_ids(&self) -> Result<Vec<i64>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.thumb_state = 0 AND i.missing_since IS NULL {GRID_ORDER}"
        ))?;
        let ids = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids)
    }

    /// Fingerprints of every indexed item; thumbnails for anything else are garbage.
    pub fn live_fingerprints(&self) -> Result<HashSet<u64>> {
        let conn = self.reader();
        let mut stmt = conn.prepare("SELECT path, size, mtime_ms FROM items")?;
        let set = stmt
            .query_map([], |r| Ok(fingerprint(&r.get::<_, String>(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<HashSet<u64>>>()?;
        Ok(set)
    }
}
```

- [ ] **Step 7: Run the tests and confirm they pass**

Run: `cargo test -p photon-core`
Expected: PASS, 14 tests.

- [ ] **Step 8: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/
git commit -m "feat(core): add folder tree and media item queries"
```

---

### Task 3: EXIF metadata extraction

**Files:**
- Create: `crates/photon-core/src/metadata.rs`
- Modify: `crates/photon-core/src/lib.rs` (add `pub mod metadata;`)
- Modify: `crates/photon-core/src/testutil.rs` (add image fixture helpers)

**Interfaces:**
- Consumes: nothing from earlier tasks except `testutil`.
- Produces:
  - `metadata::ImageMeta { width: u32, height: u32, orientation: u8, taken_at: Option<i64> }`. `width` and `height` are the stored (pre-orientation) dimensions.
  - `metadata::read_image_meta(&Path) -> ImageMeta`. It never fails: an unreadable file gives `0×0`, orientation `1`, and no date.
  - `metadata::oriented_dims(width: u32, height: u32, orientation: u8) -> (u32, u32)`.
  - `taken_at` is the EXIF local time interpreted as if it were UTC, in seconds (EXIF has no time zone).
  - testutil helpers:
    - `encode(&DynamicImage, ImageFormat) -> Vec<u8>`
    - `jpeg_bytes(w, h) -> Vec<u8>`
    - `png_bytes(w, h) -> Vec<u8>`
    - `jpeg_with_exif(w, h, orientation: u16, datetime: &str) -> Vec<u8>`
    - `write_file(dir: &Path, rel: &str, bytes: &[u8]) -> PathBuf`

- [ ] **Step 1: Add the fixture helpers**

Append to `crates/photon-core/src/testutil.rs`, merging the imports:

```rust
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use std::io::Cursor;
use std::path::PathBuf;

pub fn encode(img: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), format).unwrap();
    buf
}

fn solid(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([200, 100, 50])))
}

pub fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    encode(&solid(w, h), ImageFormat::Jpeg)
}

pub fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    encode(&solid(w, h), ImageFormat::Png)
}

/// A JPEG carrying a minimal little-endian EXIF block with Orientation and DateTimeOriginal.
/// `datetime` must be exactly "YYYY:MM:DD HH:MM:SS".
pub fn jpeg_with_exif(w: u32, h: u32, orientation: u16, datetime: &str) -> Vec<u8> {
    fn entry(t: &mut Vec<u8>, tag: u16, typ: u16, count: u32, value: u32) {
        t.extend_from_slice(&tag.to_le_bytes());
        t.extend_from_slice(&typ.to_le_bytes());
        t.extend_from_slice(&count.to_le_bytes());
        t.extend_from_slice(&value.to_le_bytes());
    }
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2a\x00");
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
    tiff.extend_from_slice(&2u16.to_le_bytes()); // IFD0: 2 entries
    entry(&mut tiff, 0x0112, 3, 1, orientation as u32); // Orientation, SHORT
    entry(&mut tiff, 0x8769, 4, 1, 38); // Exif IFD pointer, LONG
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no IFD1
    tiff.extend_from_slice(&1u16.to_le_bytes()); // Exif IFD at 38: 1 entry
    entry(&mut tiff, 0x9003, 2, 20, 56); // DateTimeOriginal, ASCII[20] at 56
    tiff.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(tiff.len(), 56);
    let mut date = datetime.as_bytes().to_vec();
    date.push(0);
    assert_eq!(date.len(), 20, "datetime must be YYYY:MM:DD HH:MM:SS");
    tiff.extend_from_slice(&date);

    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);

    let jpeg = jpeg_bytes(w, h);
    let mut out = jpeg[..2].to_vec(); // SOI
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

pub fn write_file(dir: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}
```

- [ ] **Step 2: Write failing tests**

Create `crates/photon-core/src/metadata.rs` with only the tests, and add `pub mod metadata;` to `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_with_exif, png_bytes, write_file};

    #[test]
    fn converts_naive_datetime_to_unix_seconds() {
        assert_eq!(naive_to_unix(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(naive_to_unix(2000, 3, 1, 0, 0, 0), 951_868_800);
        assert_eq!(naive_to_unix(2024, 6, 15, 12, 30, 45), 1_718_454_645);
    }

    #[test]
    fn reads_exif_orientation_and_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.jpg", &jpeg_with_exif(4, 2, 6, "2024:06:15 12:30:45"));
        assert_eq!(
            read_image_meta(&path),
            ImageMeta { width: 4, height: 2, orientation: 6, taken_at: Some(1_718_454_645) }
        );
    }

    #[test]
    fn png_without_exif_gets_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.png", &png_bytes(3, 5));
        assert_eq!(read_image_meta(&path), ImageMeta { width: 3, height: 5, orientation: 1, taken_at: None });
    }

    #[test]
    fn unreadable_file_yields_zeroed_meta() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "bad.jpg", b"not an image");
        assert_eq!(read_image_meta(&path), ImageMeta { width: 0, height: 0, orientation: 1, taken_at: None });
    }

    #[test]
    fn rejects_zeroed_exif_dates() {
        let value = exif::Value::Ascii(vec![b"0000:00:00 00:00:00".to_vec()]);
        assert_eq!(parse_exif_datetime(&value), None);
    }

    #[test]
    fn oriented_dims_swap_for_quarter_turns() {
        assert_eq!(oriented_dims(4, 2, 1), (4, 2));
        assert_eq!(oriented_dims(4, 2, 3), (4, 2));
        for o in 5..=8 {
            assert_eq!(oriented_dims(4, 2, o), (2, 4));
        }
    }
}
```

- [ ] **Step 3: Run the tests and confirm they fail**

Run: `cargo test -p photon-core metadata`
Expected: compile error, because `read_image_meta`, `ImageMeta` and the other items are not defined.

- [ ] **Step 4: Implement `metadata.rs`**

Prepend to `crates/photon-core/src/metadata.rs`:

```rust
use std::{fs::File, io::BufReader, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageMeta {
    /// Stored pixel dimensions, before applying `orientation`.
    pub width: u32,
    pub height: u32,
    /// EXIF orientation 1..=8 (1 = upright).
    pub orientation: u8,
    /// Capture time as naive local time interpreted as UTC seconds.
    pub taken_at: Option<i64>,
}

/// Reads dimensions and EXIF data. Never fails: missing data falls back to defaults.
pub fn read_image_meta(path: &Path) -> ImageMeta {
    let (width, height) = image::image_dimensions(path).unwrap_or((0, 0));
    let mut meta = ImageMeta { width, height, orientation: 1, taken_at: None };
    if let Some(exif) = read_exif(path) {
        if let Some(o) = exif
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            && (1..=8).contains(&o)
        {
            meta.orientation = o as u8;
        }
        meta.taken_at = [exif::Tag::DateTimeOriginal, exif::Tag::DateTimeDigitized, exif::Tag::DateTime]
            .iter()
            .find_map(|&tag| {
                exif.get_field(tag, exif::In::PRIMARY)
                    .and_then(|f| parse_exif_datetime(&f.value))
            });
    }
    meta
}

/// Dimensions as displayed, after applying the EXIF orientation.
pub fn oriented_dims(width: u32, height: u32, orientation: u8) -> (u32, u32) {
    if (5..=8).contains(&orientation) { (height, width) } else { (width, height) }
}

fn read_exif(path: &Path) -> Option<exif::Exif> {
    let file = File::open(path).ok()?;
    exif::Reader::new().read_from_container(&mut BufReader::new(file)).ok()
}

fn parse_exif_datetime(value: &exif::Value) -> Option<i64> {
    let exif::Value::Ascii(parts) = value else { return None };
    let dt = exif::DateTime::from_ascii(parts.first()?).ok()?;
    if dt.year == 0 || dt.month == 0 || dt.day == 0 {
        return None;
    }
    Some(naive_to_unix(
        dt.year as i64,
        dt.month as u32,
        dt.day as u32,
        dt.hour as u32,
        dt.minute as u32,
        dt.second as u32,
    ))
}

/// Civil date to Unix seconds (Howard Hinnant's days-from-civil algorithm).
pub(crate) fn naive_to_unix(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + hour as i64 * 3_600 + minute as i64 * 60 + second as i64
}
```

(`if let … && …` let-chains need edition 2024, which the workspace sets.)

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p photon-core metadata`
Expected: PASS, 6 tests.

- [ ] **Step 6: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/
git commit -m "feat(core): extract EXIF orientation, capture date and dimensions"
```

---

### Task 4: Decoding with orientation

**Files:**
- Create: `crates/photon-core/src/decode.rs`
- Modify: `crates/photon-core/src/lib.rs` (add `pub mod decode;`)

**Interfaces:**
- Consumes: `Error`/`Result` from Task 1; the `jpeg_bytes` and `write_file` testutil helpers from Task 3.
- Produces:
  - `decode::apply_orientation(DynamicImage, orientation: u8) -> DynamicImage`.
  - `decode::decode_oriented(path: &Path, orientation: u8, max_edge: u32) -> Result<DynamicImage>`. This sniffs the format from magic bytes, downscales so the longest edge is at most `max_edge` (never upscales), then applies the orientation.

- [ ] **Step 1: Write failing tests**

Create `crates/photon-core/src/decode.rs` with the tests, and add `pub mod decode;` to `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use crate::testutil::{jpeg_bytes, write_file};
    use image::{Rgba, RgbaImage};

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);

    /// 2×1 image: red on the left, blue on the right.
    fn red_blue() -> DynamicImage {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, RED);
        img.put_pixel(1, 0, BLUE);
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn orientation_changes_dimensions() {
        for o in 1..=8u8 {
            let img = apply_orientation(red_blue(), o);
            let expected = if o >= 5 { (1, 2) } else { (2, 1) };
            assert_eq!((img.width(), img.height()), expected, "orientation {o}");
        }
    }

    #[test]
    fn orientation_moves_pixels() {
        let flipped = apply_orientation(red_blue(), 2).to_rgba8();
        assert_eq!(*flipped.get_pixel(0, 0), BLUE);
        let cw = apply_orientation(red_blue(), 6).to_rgba8();
        assert_eq!((*cw.get_pixel(0, 0), *cw.get_pixel(0, 1)), (RED, BLUE));
        let ccw = apply_orientation(red_blue(), 8).to_rgba8();
        assert_eq!((*ccw.get_pixel(0, 0), *ccw.get_pixel(0, 1)), (BLUE, RED));
    }

    #[test]
    fn decode_downscales_and_orients() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.jpg", &jpeg_bytes(400, 200));
        let img = decode_oriented(&path, 1, 100).unwrap();
        assert_eq!((img.width(), img.height()), (100, 50));
        let img = decode_oriented(&path, 6, 100).unwrap();
        assert_eq!((img.width(), img.height()), (50, 100));
    }

    #[test]
    fn decode_never_upscales() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.jpg", &jpeg_bytes(40, 20));
        let img = decode_oriented(&path, 1, 100).unwrap();
        assert_eq!((img.width(), img.height()), (40, 20));
    }

    #[test]
    fn decode_reports_corrupt_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let bad = write_file(dir.path(), "bad.jpg", b"definitely not a jpeg");
        assert!(matches!(decode_oriented(&bad, 1, 100), Err(Error::Image(_))));
        let missing = dir.path().join("missing.jpg");
        assert!(matches!(decode_oriented(&missing, 1, 100), Err(Error::Io(_))));
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core decode`
Expected: compile error, because `apply_orientation` and `decode_oriented` are not defined.

- [ ] **Step 3: Implement `decode.rs`**

Prepend to `crates/photon-core/src/decode.rs`:

```rust
use crate::Result;
use image::{DynamicImage, ImageReader, imageops::FilterType};
use std::path::Path;

/// Rotates/flips `img` so an image with EXIF `orientation` displays upright.
pub fn apply_orientation(img: DynamicImage, orientation: u8) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// Decodes `path` (format sniffed from its bytes), fits it within `max_edge` and orients it.
pub fn decode_oriented(path: &Path, orientation: u8, max_edge: u32) -> Result<DynamicImage> {
    let img = ImageReader::open(path)?.with_guessed_format()?.decode()?;
    let img = if img.width().max(img.height()) > max_edge {
        img.resize(max_edge, max_edge, FilterType::Triangle)
    } else {
        img
    };
    Ok(apply_orientation(img, orientation))
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p photon-core decode`
Expected: PASS, 5 tests.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/
git commit -m "feat(core): decode images with downscaling and EXIF orientation"
```

---

### Task 5: Thumbnail cache

**Files:**
- Create: `crates/photon-core/src/thumbs/mod.rs`, `crates/photon-core/src/thumbs/cache.rs`
- Modify: `crates/photon-core/src/lib.rs` (add `pub mod thumbs;`)

**Interfaces:**
- Consumes: `decode::decode_oriented` from Task 4; the testutil helpers from Task 3.
- Produces:
  - `thumbs::ThumbSize { Grid, Preview }`, with `ThumbSize::ALL` and `max_edge(self) -> u32` (256 and 1600).
  - `thumbs::ThumbCache`, with these methods:
    - `new(root: impl Into<PathBuf>)`
    - `path_for(fp: u64, size: ThumbSize) -> PathBuf`, laid out as `<root>/<grid|preview>/<first two hex chars>/<16 hex chars>.webp`
    - `is_complete(fp: u64) -> bool`
    - `generate(source: &Path, orientation: u8, fp: u64) -> Result<()>`, which writes both sizes atomically
    - `collect_garbage(live: &HashSet<u64>) -> Result<usize>`, which returns the number of files removed

- [ ] **Step 1: Write failing tests**

Create `crates/photon-core/src/thumbs/mod.rs`:

```rust
mod cache;

pub use cache::{ThumbCache, ThumbSize};
```

Add `pub mod thumbs;` to `lib.rs`. Create `crates/photon-core/src/thumbs/cache.rs` with the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_bytes, write_file};

    fn dims(path: &Path) -> (u32, u32) {
        let img = image::open(path).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn paths_are_sharded_by_fingerprint() {
        let cache = ThumbCache::new("cache-root");
        assert_eq!(
            cache.path_for(0xabcd_ef01_2345_6789, ThumbSize::Grid),
            Path::new("cache-root").join("grid").join("ab").join("abcdef0123456789.webp")
        );
        assert_eq!(
            cache.path_for(0x1, ThumbSize::Preview),
            Path::new("cache-root").join("preview").join("00").join("0000000000000001.webp")
        );
    }

    #[test]
    fn generates_both_sizes_without_upscaling() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(800, 400));
        let cache = ThumbCache::new(dir.path().join("cache"));
        assert!(!cache.is_complete(42));
        cache.generate(&src, 1, 42).unwrap();
        assert!(cache.is_complete(42));
        assert_eq!(dims(&cache.path_for(42, ThumbSize::Grid)), (256, 128));
        assert_eq!(dims(&cache.path_for(42, ThumbSize::Preview)), (800, 400));
    }

    #[test]
    fn thumbnails_are_oriented() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(800, 400));
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.generate(&src, 6, 7).unwrap();
        assert_eq!(dims(&cache.path_for(7, ThumbSize::Grid)), (128, 256));
    }

    #[test]
    fn failed_generation_leaves_no_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "bad.jpg", b"garbage");
        let cache = ThumbCache::new(dir.path().join("cache"));
        assert!(cache.generate(&src, 1, 9).is_err());
        assert!(!cache.is_complete(9));
        assert!(!cache.path_for(9, ThumbSize::Grid).exists());
    }

    #[test]
    fn garbage_collection_keeps_live_fingerprints() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(64, 64));
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.generate(&src, 1, 1).unwrap();
        cache.generate(&src, 1, 2).unwrap();
        assert_eq!(cache.collect_garbage(&HashSet::from([1])).unwrap(), 2);
        assert!(cache.is_complete(1));
        assert!(!cache.is_complete(2));
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core thumbs`
Expected: compile error, because `ThumbCache` and `ThumbSize` are not defined.

- [ ] **Step 3: Implement `cache.rs`**

Prepend to `crates/photon-core/src/thumbs/cache.rs`:

```rust
use crate::{Result, decode::decode_oriented};
use image::DynamicImage;
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThumbSize {
    Grid,
    Preview,
}

impl ThumbSize {
    pub const ALL: [ThumbSize; 2] = [ThumbSize::Grid, ThumbSize::Preview];

    /// Longest edge in pixels.
    pub fn max_edge(self) -> u32 {
        match self {
            Self::Grid => 256,
            Self::Preview => 1600,
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Preview => "preview",
        }
    }
}

const WEBP_QUALITY: f32 = 85.0;

/// On-disk WebP thumbnails keyed by content fingerprint.
pub struct ThumbCache {
    root: PathBuf,
}

impl ThumbCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn path_for(&self, fp: u64, size: ThumbSize) -> PathBuf {
        let hex = format!("{fp:016x}");
        self.root.join(size.dir_name()).join(&hex[..2]).join(format!("{hex}.webp"))
    }

    pub fn is_complete(&self, fp: u64) -> bool {
        ThumbSize::ALL.iter().all(|&size| self.path_for(fp, size).is_file())
    }

    /// Decodes `source` once and writes the preview and grid thumbnails.
    pub fn generate(&self, source: &Path, orientation: u8, fp: u64) -> Result<()> {
        let preview = decode_oriented(source, orientation, ThumbSize::Preview.max_edge())?;
        let grid = shrink(&preview, ThumbSize::Grid.max_edge());
        write_webp(&preview, &self.path_for(fp, ThumbSize::Preview))?;
        write_webp(&grid, &self.path_for(fp, ThumbSize::Grid))?;
        Ok(())
    }

    /// Removes thumbnails whose fingerprint is not in `live`. Returns the number of files removed.
    pub fn collect_garbage(&self, live: &HashSet<u64>) -> Result<usize> {
        let mut removed = 0;
        for entry in walkdir::WalkDir::new(&self.root).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("webp") {
                continue;
            }
            let Some(fp) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| u64::from_str_radix(s, 16).ok())
            else {
                continue;
            };
            if !live.contains(&fp) {
                fs::remove_file(path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

fn shrink(img: &DynamicImage, max_edge: u32) -> DynamicImage {
    if img.width().max(img.height()) > max_edge {
        img.thumbnail(max_edge, max_edge)
    } else {
        img.clone()
    }
}

/// Writes through a temp file + rename so readers never see a half-written thumbnail.
fn write_webp(img: &DynamicImage, dest: &Path) -> Result<()> {
    let rgba = img.to_rgba8();
    let data = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()).encode(WEBP_QUALITY);
    let dir = dest.parent().expect("thumbnail path has a parent");
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&data)?;
    tmp.persist(dest).map_err(|e| e.error)?;
    Ok(())
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p photon-core thumbs`
Expected: PASS, 5 tests.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/
git commit -m "feat(core): add fingerprint-keyed WebP thumbnail cache"
```

---

### Task 6: Priority thumbnail queue

**Files:**
- Create: `crates/photon-core/src/thumbs/queue.rs`
- Modify: `crates/photon-core/src/thumbs/mod.rs` (add `mod queue;` and `pub use queue::{Priority, ThumbQueue};`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `thumbs::Priority { Visible, Neighbour, Background }`. `Visible` is served first.
  - `thumbs::ThumbQueue` (`Default`, `Send + Sync`), with these methods:
    - `new()`
    - `push(id: i64, Priority)`, which only raises an entry's priority, never lowers it
    - `push_many(&[i64], Priority)`
    - `set_visible(&[i64])`, which demotes the previous visible set to `Background`
    - `pop_blocking() -> Option<i64>`, which returns `None` once the queue is closed
    - `done()`, which the worker calls after each popped job
    - `wait_idle()`, which returns once the queue is empty and nothing is in flight, or when it is closed
    - `len()`, `is_empty()` and `close()`

- [ ] **Step 1: Write failing tests**

Create `crates/photon-core/src/thumbs/queue.rs` with the tests, and register the module in `thumbs/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn drain(q: &ThumbQueue) -> Vec<i64> {
        let mut out = Vec::new();
        while !q.is_empty() {
            out.push(q.pop_blocking().unwrap());
            q.done();
        }
        out
    }

    #[test]
    fn serves_by_priority_then_fifo() {
        let q = ThumbQueue::new();
        q.push_many(&[1, 2], Priority::Background);
        q.push(3, Priority::Neighbour);
        q.push(4, Priority::Visible);
        assert_eq!(drain(&q), [4, 3, 1, 2]);
    }

    #[test]
    fn push_only_raises_priority() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Background);
        q.push(2, Priority::Visible);
        q.push(2, Priority::Background); // ignored: already higher
        q.push(1, Priority::Neighbour); // raised
        assert_eq!(q.len(), 2);
        assert_eq!(drain(&q), [2, 1]);
    }

    #[test]
    fn set_visible_demotes_previous_visible_items() {
        let q = ThumbQueue::new();
        q.push(9, Priority::Neighbour);
        q.set_visible(&[1, 2]);
        q.set_visible(&[3]);
        assert_eq!(drain(&q), [3, 9, 1, 2]);
    }

    #[test]
    fn close_releases_blocked_poppers() {
        let q = Arc::new(ThumbQueue::new());
        let worker = {
            let q = q.clone();
            std::thread::spawn(move || q.pop_blocking())
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        q.close();
        assert_eq!(worker.join().unwrap(), None);
    }

    #[test]
    fn wait_idle_waits_for_in_flight_work() {
        let q = Arc::new(ThumbQueue::new());
        q.push(1, Priority::Background);
        let worker = {
            let q = q.clone();
            std::thread::spawn(move || {
                let id = q.pop_blocking().unwrap();
                std::thread::sleep(std::time::Duration::from_millis(50));
                q.done();
                id
            })
        };
        q.wait_idle();
        assert!(q.is_empty());
        assert_eq!(worker.join().unwrap(), 1);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core queue`
Expected: compile error, because `ThumbQueue` and `Priority` are not defined.

- [ ] **Step 3: Implement `queue.rs`**

Prepend to `crates/photon-core/src/thumbs/queue.rs`:

```rust
use parking_lot::{Condvar, Mutex};
use std::collections::{BTreeSet, HashMap};

/// Lower sorts first: visible grid cells beat viewer neighbours beat background fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Visible,
    Neighbour,
    Background,
}

#[derive(Default)]
struct State {
    /// (priority, insertion sequence, item id) — pop_first yields the next job.
    order: BTreeSet<(Priority, u64, i64)>,
    entries: HashMap<i64, (Priority, u64)>,
    visible: Vec<i64>,
    next_seq: u64,
    active: usize,
    closed: bool,
}

impl State {
    fn push(&mut self, id: i64, priority: Priority) {
        if let Some(&(current, seq)) = self.entries.get(&id) {
            if current <= priority {
                return;
            }
            self.order.remove(&(current, seq, id));
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.order.insert((priority, seq, id));
        self.entries.insert(id, (priority, seq));
    }

    fn demote(&mut self, id: i64, priority: Priority) {
        if let Some(&(current, seq)) = self.entries.get(&id)
            && current < priority
        {
            self.order.remove(&(current, seq, id));
            self.order.insert((priority, seq, id));
            self.entries.insert(id, (priority, seq));
        }
    }

    fn pop(&mut self) -> Option<i64> {
        let (_, _, id) = self.order.pop_first()?;
        self.entries.remove(&id);
        Some(id)
    }
}

/// Thumbnail job queue shared by the worker pool. Tracks in-flight jobs so callers can wait for idle.
#[derive(Default)]
pub struct ThumbQueue {
    state: Mutex<State>,
    // One condvar serves both workers and idle-waiters, so every change uses notify_all.
    changed: Condvar,
}

impl ThumbQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, id: i64, priority: Priority) {
        self.state.lock().push(id, priority);
        self.changed.notify_all();
    }

    pub fn push_many(&self, ids: &[i64], priority: Priority) {
        let mut state = self.state.lock();
        for &id in ids {
            state.push(id, priority);
        }
        drop(state);
        self.changed.notify_all();
    }

    /// Replaces the set of items currently on screen.
    pub fn set_visible(&self, ids: &[i64]) {
        let mut state = self.state.lock();
        for id in std::mem::take(&mut state.visible) {
            state.demote(id, Priority::Background);
        }
        for &id in ids {
            state.push(id, Priority::Visible);
        }
        state.visible = ids.to_vec();
        drop(state);
        self.changed.notify_all();
    }

    /// Blocks until a job is available. Every `Some` must be followed by `done()`.
    pub fn pop_blocking(&self) -> Option<i64> {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(id) = state.pop() {
                state.active += 1;
                return Some(id);
            }
            self.changed.wait(&mut state);
        }
    }

    pub fn done(&self) {
        let mut state = self.state.lock();
        state.active = state.active.saturating_sub(1);
        drop(state);
        self.changed.notify_all();
    }

    pub fn wait_idle(&self) {
        let mut state = self.state.lock();
        while !state.closed && !(state.order.is_empty() && state.active == 0) {
            self.changed.wait(&mut state);
        }
    }

    pub fn len(&self) -> usize {
        self.state.lock().order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn close(&self) {
        self.state.lock().closed = true;
        self.changed.notify_all();
    }
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p photon-core queue`
Expected: PASS, 5 tests.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/
git commit -m "feat(core): add priority thumbnail queue with idle tracking"
```

---

### Task 7: Thumbnail service (worker pool and on-demand generation)

**Files:**
- Create: `crates/photon-core/src/thumbs/service.rs`
- Modify: `crates/photon-core/src/thumbs/mod.rs` (add `mod service;` and `pub use service::{ThumbService, default_workers};`)

**Interfaces:**
- Consumes:
  - `Library`: `item`, `set_thumb_state`, `pending_thumb_ids` and `live_fingerprints` (Task 2).
  - `ThumbCache` and `ThumbSize` (Task 5); `ThumbQueue` and `Priority` (Task 6).
  - testutil: `seed_folder`, `new_item`, `write_file` and `jpeg_bytes`.
- Produces:
  - `thumbs::default_workers() -> usize`, which is cores − 1 with a minimum of 1.
  - `thumbs::ThumbService` (dropping it closes the queue and joins the workers), with these methods:
    - `start(lib: Arc<Library>, cache: Arc<ThumbCache>, workers: usize) -> ThumbService`
    - `enqueue_pending() -> Result<usize>`
    - `set_visible(&[i64])`
    - `prioritize(&[i64], Priority)`
    - `get_or_generate(id: i64, size: ThumbSize) -> Result<PathBuf>`
    - `wait_idle()`
    - `queued() -> usize`
    - `collect_garbage() -> Result<usize>`
  - Errors from `get_or_generate`: `Error::NotFound(id)` for an unknown item, and `Error::ThumbFailed(message)` for an item that previously failed or fails now.

- [ ] **Step 1: Write failing tests**

Create `crates/photon-core/src/thumbs/service.rs` with the tests, and register the module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::NewItem;
    use crate::testutil::{jpeg_bytes, new_item, seed_folder, write_file};
    use tempfile::TempDir;

    fn setup(files: &[(&str, Vec<u8>)]) -> (TempDir, Arc<Library>, Arc<ThumbCache>, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Arc::new(Library::open(&dir.path().join("library.db")).unwrap());
        let photos = dir.path().join("photos");
        let (_, folder) = seed_folder(&lib, &photos);
        let items: Vec<NewItem> = files
            .iter()
            .map(|(name, bytes)| {
                let path = write_file(&photos, name, bytes);
                new_item(folder, path.to_str().unwrap(), 0)
            })
            .collect();
        let ids = lib.insert_items(&items).unwrap();
        let cache = Arc::new(ThumbCache::new(dir.path().join("cache")));
        (dir, lib, cache, ids)
    }

    fn state(lib: &Library, id: i64) -> ThumbState {
        lib.item(id).unwrap().unwrap().thumb_state
    }

    #[test]
    fn processes_pending_items_in_background() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32)), ("b.jpg", jpeg_bytes(32, 64))]);
        let service = ThumbService::start(lib.clone(), cache.clone(), 2);
        assert_eq!(service.enqueue_pending().unwrap(), 2);
        service.wait_idle();
        for id in ids {
            assert_eq!(state(&lib, id), ThumbState::Ready);
            assert!(cache.is_complete(lib.item(id).unwrap().unwrap().fingerprint()));
        }
        assert!(lib.pending_thumb_ids().unwrap().is_empty());
    }

    #[test]
    fn records_failures_instead_of_retrying() {
        let (_dir, lib, cache, ids) = setup(&[("bad.jpg", b"garbage".to_vec())]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        service.enqueue_pending().unwrap();
        service.wait_idle();
        let item = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Failed);
        assert!(item.thumb_error.is_some());
        assert_eq!(service.enqueue_pending().unwrap(), 0);
    }

    #[test]
    fn get_or_generate_builds_on_demand() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        let path = service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert!(path.is_file());
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
        assert_eq!(service.get_or_generate(ids[0], ThumbSize::Preview).unwrap().extension().unwrap(), "webp");
    }

    #[test]
    fn get_or_generate_reports_failures_and_unknown_items() {
        let (_dir, lib, cache, ids) = setup(&[("bad.jpg", b"garbage".to_vec())]);
        let service = ThumbService::start(lib, cache, 1);
        assert!(matches!(service.get_or_generate(ids[0], ThumbSize::Grid), Err(Error::ThumbFailed(_))));
        assert!(matches!(service.get_or_generate(ids[0], ThumbSize::Grid), Err(Error::ThumbFailed(_))));
        assert!(matches!(service.get_or_generate(9_999, ThumbSize::Grid), Err(Error::NotFound(9_999))));
    }

    #[test]
    fn collect_garbage_removes_thumbnails_of_purged_items() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32)), ("b.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        service.enqueue_pending().unwrap();
        service.wait_idle();
        lib.purge_items(&[ids[1]]).unwrap();
        assert_eq!(service.collect_garbage().unwrap(), 2);
        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_ok());
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core service`
Expected: compile error, because `ThumbService` is not defined.

- [ ] **Step 3: Implement `service.rs`**

Prepend to `crates/photon-core/src/thumbs/service.rs`:

```rust
use super::{Priority, ThumbCache, ThumbQueue, ThumbSize};
use crate::{Error, Result, library::Library, media::ThumbState};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
};

/// Worker threads for thumbnail generation: all cores but one, so the UI stays responsive.
pub fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1))
        .unwrap_or(1)
        .max(1)
}

pub struct ThumbService {
    lib: Arc<Library>,
    cache: Arc<ThumbCache>,
    queue: Arc<ThumbQueue>,
    workers: Vec<JoinHandle<()>>,
}

impl ThumbService {
    pub fn start(lib: Arc<Library>, cache: Arc<ThumbCache>, workers: usize) -> Self {
        let queue = Arc::new(ThumbQueue::new());
        let workers = (0..workers.max(1))
            .map(|i| {
                let (lib, cache, queue) = (lib.clone(), cache.clone(), queue.clone());
                std::thread::Builder::new()
                    .name(format!("photon-thumb-{i}"))
                    .spawn(move || {
                        while let Some(id) = queue.pop_blocking() {
                            if let Err(err) = process(&lib, &cache, id) {
                                tracing::warn!(id, %err, "thumbnail job failed");
                            }
                            queue.done();
                        }
                    })
                    .expect("failed to spawn thumbnail worker")
            })
            .collect();
        Self { lib, cache, queue, workers }
    }

    /// Queues every item still waiting for thumbnails at background priority.
    pub fn enqueue_pending(&self) -> Result<usize> {
        let ids = self.lib.pending_thumb_ids()?;
        self.queue.push_many(&ids, Priority::Background);
        Ok(ids.len())
    }

    pub fn set_visible(&self, ids: &[i64]) {
        self.queue.set_visible(ids);
    }

    pub fn prioritize(&self, ids: &[i64], priority: Priority) {
        self.queue.push_many(ids, priority);
    }

    /// Returns the cached thumbnail, generating it on the calling thread if needed.
    pub fn get_or_generate(&self, id: i64, size: ThumbSize) -> Result<PathBuf> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.thumb_state == ThumbState::Failed {
            return Err(Error::ThumbFailed(item.thumb_error.unwrap_or_default()));
        }
        let path = self.cache.path_for(item.fingerprint(), size);
        if path.is_file() {
            return Ok(path);
        }
        process(&self.lib, &self.cache, id)?;
        if path.is_file() {
            return Ok(path);
        }
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        Err(Error::ThumbFailed(item.thumb_error.unwrap_or_else(|| "thumbnail unavailable".into())))
    }

    pub fn wait_idle(&self) {
        self.queue.wait_idle();
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    pub fn collect_garbage(&self) -> Result<usize> {
        let live = self.lib.live_fingerprints()?;
        self.cache.collect_garbage(&live)
    }
}

impl Drop for ThumbService {
    fn drop(&mut self) {
        self.queue.close();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

/// Generates thumbnails for one item. Decode failures are recorded on the item, not returned.
fn process(lib: &Library, cache: &ThumbCache, id: i64) -> Result<()> {
    let Some(item) = lib.item(id)? else { return Ok(()) };
    if item.missing_since.is_some() || item.thumb_state == ThumbState::Failed {
        return Ok(());
    }
    let fp = item.fingerprint();
    if !cache.is_complete(fp)
        && let Err(err) = cache.generate(Path::new(&item.path), item.orientation, fp)
    {
        lib.set_thumb_state(id, ThumbState::Failed, Some(&err.to_string()))?;
        return Ok(());
    }
    if item.thumb_state != ThumbState::Ready {
        lib.set_thumb_state(id, ThumbState::Ready, None)?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p photon-core service`
Expected: PASS, 5 tests.

- [ ] **Step 5: Run the whole suite, lint and commit**

```bash
cargo test -p photon-core
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/
git commit -m "feat(core): add thumbnail worker pool with on-demand generation"
```

---

### Task 8: Scanner

**Files:**
- Create: `crates/photon-core/src/scanner.rs`
- Modify: `crates/photon-core/src/lib.rs` (add `pub mod scanner;`)

**Interfaces:**
- Consumes:
  - `Library`: `set_watched_online`, `known_items`, `upsert_folder`, `insert_items`, `update_items`, `mark_missing`, `purge_items` and `prune_folders`.
  - `NewItem` and `WatchedFolder`; `MediaKind::from_path`; `metadata::read_image_meta`.
- Produces:
  - `scanner::ScanProgress { files_seen: u64, added: u64, changed: u64 }`.
  - `scanner::ScanReport { offline: bool, added: u64, changed: u64, unchanged: u64, marked_missing: u64, purged: u64 }` (`Debug`, `Default`).
  - `scanner::scan_watched(lib: &Library, watched: &WatchedFolder, scan_id: i64, progress: &mut dyn FnMut(&ScanProgress)) -> Result<ScanReport>`. `scan_id` must increase between scans; use `now_ms()`. It is also the `missing_since` timestamp. `progress` is called after each batch flush and once at the end.

- [ ] **Step 1: Write failing tests**

Create `crates/photon-core/src/scanner.rs` with the tests, and add `pub mod scanner;` to `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_bytes, jpeg_with_exif, png_bytes, temp_library, write_file};
    use std::fs;

    fn scan(lib: &Library, watched: &WatchedFolder, scan_id: i64) -> ScanReport {
        scan_watched(lib, watched, scan_id, &mut |_| {}).unwrap()
    }

    fn key(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn indexes_supported_files_and_folders() {
        let (dir, lib) = temp_library();
        let root = dir.path().join("photos");
        let a = write_file(&root, "a.jpg", &jpeg_with_exif(4, 2, 6, "2024:06:15 12:30:45"));
        write_file(&root, "2024/b.png", &png_bytes(3, 3));
        write_file(&root, "notes.txt", b"ignored");
        write_file(&root, ".hidden/c.jpg", &jpeg_bytes(8, 8));
        write_file(&root, ".d.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root).unwrap();

        let mut last = None;
        let report = scan_watched(&lib, &watched, 1, &mut |p| last = Some(*p)).unwrap();
        assert_eq!((report.added, report.changed, report.offline), (2, 0, false));
        assert_eq!(last.unwrap().files_seen, 2);

        let known = lib.known_items(watched.id).unwrap();
        assert_eq!(known.len(), 2);
        let item = lib.item(known[&key(&a)].id).unwrap().unwrap();
        assert_eq!((item.orientation, item.taken_at, item.width), (6, 1_718_454_645, 4));

        let names: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["photos", "2024"]);
        let sub = &lib.folders().unwrap()[1];
        assert_eq!(sub.parent_id, Some(lib.folders().unwrap()[0].id));
    }

    #[test]
    fn capture_date_falls_back_to_mtime() {
        let (dir, lib) = temp_library();
        let root = dir.path().join("photos");
        let p = write_file(&root, "a.png", &png_bytes(2, 2));
        let watched = lib.add_watched_folder(&root).unwrap();
        scan(&lib, &watched, 1);
        let mtime_s = fs::metadata(&p)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let id = lib.known_items(watched.id).unwrap()[&key(&p)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().taken_at, mtime_s);
    }

    #[test]
    fn rescans_detect_unchanged_and_changed_files() {
        let (dir, lib) = temp_library();
        let root = dir.path().join("photos");
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let b = write_file(&root, "b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root).unwrap();
        scan(&lib, &watched, 1);

        write_file(&root, "b.jpg", &jpeg_bytes(64, 64));
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.added, report.changed, report.unchanged), (0, 1, 1));
        let id = lib.known_items(watched.id).unwrap()[&key(&b)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().width, 64);
    }

    #[test]
    fn missing_files_are_soft_deleted_then_purged() {
        let (dir, lib) = temp_library();
        let root = dir.path().join("photos");
        let gone = write_file(&root, "trip/a.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&gone)].id;

        fs::remove_dir_all(root.join("trip")).unwrap();
        let report = scan(&lib, &watched, 2);
        assert_eq!((report.marked_missing, report.purged), (1, 0));
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, Some(2));
        assert_eq!(lib.folders().unwrap().len(), 2, "folder kept while it holds a soft-deleted item");

        let report = scan(&lib, &watched, 3);
        assert_eq!((report.marked_missing, report.purged), (0, 1));
        assert!(lib.item(id).unwrap().is_none());
        assert_eq!(lib.folders().unwrap().len(), 1, "empty folder pruned");
    }

    #[test]
    fn reappearing_files_are_restored() {
        let (dir, lib) = temp_library();
        let root = dir.path().join("photos");
        let a = write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let bytes = fs::read(&a).unwrap();
        let watched = lib.add_watched_folder(&root).unwrap();
        scan(&lib, &watched, 1);
        fs::remove_file(&a).unwrap();
        scan(&lib, &watched, 2);
        fs::write(&a, bytes).unwrap();
        let report = scan(&lib, &watched, 3);
        assert_eq!(report.changed, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&a)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
    }

    #[test]
    fn unreachable_folder_goes_offline_and_keeps_items() {
        let (dir, lib) = temp_library();
        let root = dir.path().join("photos");
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root).unwrap();
        scan(&lib, &watched, 1);

        fs::rename(&root, dir.path().join("unplugged")).unwrap();
        let report = scan(&lib, &watched, 2);
        assert!(report.offline);
        assert!(!lib.watched_folders().unwrap()[0].online);
        assert_eq!(lib.known_items(watched.id).unwrap().len(), 1);
        assert!(lib.known_items(watched.id).unwrap().values().all(|k| !k.missing));

        fs::rename(dir.path().join("unplugged"), &root).unwrap();
        assert!(!scan(&lib, &watched, 3).offline);
        assert!(lib.watched_folders().unwrap()[0].online);
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core scanner`
Expected: compile error, because `scan_watched` and the report types are not defined.

- [ ] **Step 3: Implement `scanner.rs`**

Prepend to `crates/photon-core/src/scanner.rs`:

```rust
use crate::{
    Result,
    library::{Library, NewItem, WatchedFolder},
    media::MediaKind,
    metadata::read_image_meta,
};
use std::{
    collections::HashMap,
    fs::Metadata,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use walkdir::{DirEntry, WalkDir};

const BATCH: usize = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files_seen: u64,
    pub added: u64,
    pub changed: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub offline: bool,
    pub added: u64,
    pub changed: u64,
    pub unchanged: u64,
    pub marked_missing: u64,
    pub purged: u64,
}

/// Brings the library in line with what is on disk under `watched`.
/// Never modifies files; only reads directory listings, metadata and EXIF.
pub fn scan_watched(
    lib: &Library,
    watched: &WatchedFolder,
    scan_id: i64,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport> {
    let root = Path::new(&watched.path);
    if !root.is_dir() {
        lib.set_watched_online(watched.id, false)?;
        return Ok(ScanReport { offline: true, ..ScanReport::default() });
    }
    lib.set_watched_online(watched.id, true)?;

    let mut known = lib.known_items(watched.id)?;
    let mut folder_ids: HashMap<PathBuf, i64> = HashMap::new();
    let mut report = ScanReport::default();
    let mut seen = ScanProgress::default();
    let mut new_batch: Vec<NewItem> = Vec::new();
    let mut changed_batch: Vec<(i64, NewItem)> = Vec::new();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e));
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(%err, "skipping unreadable entry");
                continue;
            }
        };
        let path = entry.path();
        let Some(path_str) = path.to_str() else {
            tracing::warn!(?path, "skipping non-UTF-8 path");
            continue;
        };

        if entry.file_type().is_dir() {
            let parent = if entry.depth() == 0 {
                None
            } else {
                path.parent().and_then(|p| folder_ids.get(p)).copied()
            };
            let id = lib.upsert_folder(watched.id, parent, path_str, scan_id)?;
            folder_ids.insert(path.to_path_buf(), id);
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(kind) = MediaKind::from_path(path) else { continue };
        let Some(&folder_id) = path.parent().and_then(|p| folder_ids.get(p)) else { continue };
        let md = match entry.metadata() {
            Ok(md) => md,
            Err(err) => {
                tracing::warn!(%err, ?path, "skipping file without metadata");
                continue;
            }
        };
        let (size, mtime_ms) = (md.len() as i64, mtime_ms(&md));
        seen.files_seen += 1;

        match known.remove(path_str) {
            Some(k) if k.size == size && k.mtime_ms == mtime_ms && !k.missing => report.unchanged += 1,
            Some(k) => changed_batch.push((k.id, describe(&entry, path_str, folder_id, kind, size, mtime_ms))),
            None => new_batch.push(describe(&entry, path_str, folder_id, kind, size, mtime_ms)),
        }

        if new_batch.len() >= BATCH {
            flush_new(lib, &mut new_batch, &mut report, &mut seen, progress)?;
        }
        if changed_batch.len() >= BATCH {
            flush_changed(lib, &mut changed_batch, &mut report, &mut seen, progress)?;
        }
    }
    flush_new(lib, &mut new_batch, &mut report, &mut seen, progress)?;
    flush_changed(lib, &mut changed_batch, &mut report, &mut seen, progress)?;

    // Anything left in `known` was not found on this (reachable) scan: soft-delete it,
    // or purge it if it was already missing last time.
    let (mut to_mark, mut to_purge) = (Vec::new(), Vec::new());
    for k in known.into_values() {
        if k.missing { to_purge.push(k.id) } else { to_mark.push(k.id) }
    }
    lib.mark_missing(&to_mark, scan_id)?;
    lib.purge_items(&to_purge)?;
    lib.prune_folders(watched.id, scan_id)?;
    report.marked_missing = to_mark.len() as u64;
    report.purged = to_purge.len() as u64;

    progress(&seen);
    Ok(report)
}

fn describe(entry: &DirEntry, path: &str, folder_id: i64, kind: MediaKind, size: i64, mtime_ms: i64) -> NewItem {
    let meta = read_image_meta(entry.path());
    NewItem {
        folder_id,
        path: path.to_string(),
        file_name: entry.file_name().to_string_lossy().into_owned(),
        kind,
        size,
        mtime_ms,
        width: meta.width,
        height: meta.height,
        orientation: meta.orientation,
        taken_at: meta.taken_at.unwrap_or(mtime_ms.div_euclid(1000)),
    }
}

fn flush_new(
    lib: &Library,
    batch: &mut Vec<NewItem>,
    report: &mut ScanReport,
    seen: &mut ScanProgress,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    lib.insert_items(batch)?;
    report.added += batch.len() as u64;
    seen.added = report.added;
    batch.clear();
    progress(seen);
    Ok(())
}

fn flush_changed(
    lib: &Library,
    batch: &mut Vec<(i64, NewItem)>,
    report: &mut ScanReport,
    seen: &mut ScanProgress,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    lib.update_items(batch)?;
    report.changed += batch.len() as u64;
    seen.changed = report.changed;
    batch.clear();
    progress(seen);
    Ok(())
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry.file_name().to_str().is_some_and(|name| name.starts_with('.'))
}

fn mtime_ms(md: &Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p photon-core scanner`
Expected: PASS, 6 tests.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/
git commit -m "feat(core): add incremental folder scanner with soft-delete and offline handling"
```

---

### Task 9: Grid index

**Files:**
- Create: `crates/photon-core/src/grid.rs`
- Modify: `crates/photon-core/src/lib.rs` (add `pub mod grid;`)
- Modify: `crates/photon-core/src/library/items.rs` (add `grid_entries`)

**Interfaces:**
- Consumes: `Library`, `items::GRID_ORDER` (Task 2), `metadata::oriented_dims` (Task 3) and `MediaKind`.
- Produces:
  - `grid::GridEntry { id: i64, folder_id: i64, taken_at: i64, aspect: f32, kind: MediaKind }`. `aspect` is the oriented width divided by height, `1.0` when unknown. It serialises as camelCase.
  - `grid::Section { folder_id: i64, offset: usize, count: usize }`, which serialises as camelCase.
  - `grid::GridIndex`, with these methods:
    - `build(Vec<GridEntry>) -> GridIndex`
    - `len()` and `is_empty()`
    - `rows(offset: usize, count: usize) -> &[GridEntry]`, clamped to the available rows
    - `sections() -> &[Section]`
    - `offset_of_folder(folder_id: i64) -> Option<usize>`
    - `position_of(id: i64) -> Option<usize>`
    - `neighbours(id: i64, radius: usize) -> Vec<i64>`, nearest first: +1, −1, +2, −2, and so on
  - `Library::grid_entries() -> Result<Vec<GridEntry>>`, in grid order, excluding missing items.

- [ ] **Step 1: Write failing tests**

Create `crates/photon-core/src/grid.rs` with the tests, and add `pub mod grid;` to `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, folder_id: i64) -> GridEntry {
        GridEntry { id, folder_id, taken_at: id, aspect: 1.5, kind: MediaKind::Image }
    }

    fn sample() -> GridIndex {
        GridIndex::build(vec![entry(1, 10), entry(2, 10), entry(3, 20), entry(4, 30), entry(5, 30)])
    }

    #[test]
    fn builds_sections_per_folder_run() {
        let grid = sample();
        assert_eq!(grid.len(), 5);
        assert_eq!(
            grid.sections(),
            [
                Section { folder_id: 10, offset: 0, count: 2 },
                Section { folder_id: 20, offset: 2, count: 1 },
                Section { folder_id: 30, offset: 3, count: 2 },
            ]
        );
        assert_eq!(grid.offset_of_folder(30), Some(3));
        assert_eq!(grid.offset_of_folder(99), None);
    }

    #[test]
    fn rows_are_clamped() {
        let grid = sample();
        let ids = |rows: &[GridEntry]| rows.iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(grid.rows(1, 2)), [2, 3]);
        assert_eq!(ids(grid.rows(4, 10)), [5]);
        assert!(grid.rows(10, 5).is_empty());
        assert!(GridIndex::build(Vec::new()).rows(0, 5).is_empty());
    }

    #[test]
    fn positions_and_neighbours() {
        let grid = sample();
        assert_eq!(grid.position_of(4), Some(3));
        assert_eq!(grid.position_of(99), None);
        assert_eq!(grid.neighbours(3, 2), [4, 2, 5, 1]);
        assert_eq!(grid.neighbours(1, 2), [2, 3]);
        assert!(grid.neighbours(99, 2).is_empty());
    }

    #[test]
    fn serialises_as_camel_case() {
        let json = serde_json::to_string(&Section { folder_id: 1, offset: 2, count: 3 }).unwrap();
        assert_eq!(json, r#"{"folderId":1,"offset":2,"count":3}"#);
        let json = serde_json::to_string(&entry(7, 1)).unwrap();
        assert_eq!(json, r#"{"id":7,"folderId":1,"takenAt":7,"aspect":1.5,"kind":"image"}"#);
    }
}
```

Add `serde_json = "1"` under `[dev-dependencies]` in `crates/photon-core/Cargo.toml`. Plan 2 uses it too.

Add these library tests to the `tests` module in `crates/photon-core/src/library/items.rs`:

```rust
    #[test]
    fn grid_entries_are_ordered_oriented_and_skip_missing() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let rotated = NewItem { orientation: 6, ..new_item(a, "/p/a/2.jpg", 5) };
        let unknown = NewItem { width: 0, height: 0, ..new_item(b, "/p/b/1.jpg", 1) };
        let ids = lib
            .insert_items(&[unknown, rotated, new_item(a, "/p/a/1.jpg", 2), new_item(a, "/p/a/3.jpg", 9)])
            .unwrap();
        lib.mark_missing(&[ids[3]], 1).unwrap();

        let entries = lib.grid_entries().unwrap();
        let order: Vec<i64> = entries.iter().map(|e| e.id).collect();
        assert_eq!(order, [ids[2], ids[1], ids[0]]);
        assert_eq!(entries[0].aspect, 400.0 / 300.0);
        assert_eq!(entries[1].aspect, 300.0 / 400.0);
        assert_eq!(entries[2].aspect, 1.0);
        assert_eq!(entries[0].folder_id, a);
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core grid`
Expected: compile error, because `GridIndex`, `GridEntry` and `grid_entries` are not defined.

- [ ] **Step 3: Implement `grid.rs`**

Prepend to `crates/photon-core/src/grid.rs`:

```rust
use crate::media::MediaKind;
use serde::Serialize;
use std::collections::HashMap;

/// One cell of the library grid. Small and `Copy`: 100k of them stay in memory.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridEntry {
    pub id: i64,
    pub folder_id: i64,
    pub taken_at: i64,
    /// Displayed width / height (orientation applied); 1.0 when unknown.
    pub aspect: f32,
    pub kind: MediaKind,
}

/// A run of consecutive grid entries from one folder, shown under one header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub folder_id: i64,
    pub offset: usize,
    pub count: usize,
}

/// Ordered in-memory index the UI pages through by position.
#[derive(Debug, Default)]
pub struct GridIndex {
    entries: Vec<GridEntry>,
    sections: Vec<Section>,
    positions: HashMap<i64, usize>,
}

impl GridIndex {
    /// `entries` must already be in grid order (see `Library::grid_entries`).
    pub fn build(entries: Vec<GridEntry>) -> Self {
        let mut sections: Vec<Section> = Vec::new();
        let mut positions = HashMap::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            positions.insert(entry.id, index);
            match sections.last_mut() {
                Some(section) if section.folder_id == entry.folder_id => section.count += 1,
                _ => sections.push(Section { folder_id: entry.folder_id, offset: index, count: 1 }),
            }
        }
        Self { entries, sections, positions }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn rows(&self, offset: usize, count: usize) -> &[GridEntry] {
        let start = offset.min(self.entries.len());
        let end = start.saturating_add(count).min(self.entries.len());
        &self.entries[start..end]
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn offset_of_folder(&self, folder_id: i64) -> Option<usize> {
        self.sections.iter().find(|s| s.folder_id == folder_id).map(|s| s.offset)
    }

    pub fn position_of(&self, id: i64) -> Option<usize> {
        self.positions.get(&id).copied()
    }

    /// Items around `id`, nearest first (+1, -1, +2, -2, …), for viewer preloading.
    pub fn neighbours(&self, id: i64, radius: usize) -> Vec<i64> {
        let Some(pos) = self.position_of(id) else { return Vec::new() };
        let mut out = Vec::with_capacity(radius * 2);
        for distance in 1..=radius {
            if let Some(next) = self.entries.get(pos + distance) {
                out.push(next.id);
            }
            if let Some(prev) = pos.checked_sub(distance) {
                out.push(self.entries[prev].id);
            }
        }
        out
    }
}
```

- [ ] **Step 4: Implement `Library::grid_entries`**

Add these imports to `crates/photon-core/src/library/items.rs`:

```rust
use crate::grid::GridEntry;
use crate::metadata::oriented_dims;
```

Add this method to its `impl Library` block:

```rust
    /// Every visible item in grid order: folder tree order, then capture time, then name.
    pub fn grid_entries(&self) -> Result<Vec<GridEntry>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(&format!(
            "SELECT i.id, i.folder_id, i.taken_at, i.width, i.height, i.orientation, i.kind
             FROM items i JOIN folders f ON f.id = i.folder_id
             WHERE i.missing_since IS NULL {GRID_ORDER}"
        ))?;
        let rows = stmt
            .query_map([], |r| {
                let (w, h) = oriented_dims(r.get(3)?, r.get(4)?, r.get(5)?);
                Ok(GridEntry {
                    id: r.get(0)?,
                    folder_id: r.get(1)?,
                    taken_at: r.get(2)?,
                    aspect: if w == 0 || h == 0 { 1.0 } else { w as f32 / h as f32 },
                    kind: MediaKind::from_db(r.get(6)?).unwrap_or(MediaKind::Image),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
```

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p photon-core`
Expected: PASS for the whole suite (about 50 tests), including `grid::tests` and `grid_entries_are_ordered_oriented_and_skip_missing`.

- [ ] **Step 6: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/
git commit -m "feat(core): add in-memory grid index with sections and neighbours"
```

---

### Task 10: Benchmarks, end-to-end example and CI

**Files:**
- Create: `crates/photon-core/benches/grid.rs`
- Create: `crates/photon-core/examples/index.rs`
- Create: `.github/workflows/ci.yml`
- Modify: `crates/photon-core/Cargo.toml` (add a `[[bench]]` section)

**Interfaces:**
- Consumes the whole public API:
  - `Library` (`open`, `add_watched_folder`, `upsert_folder`, `insert_items`, `grid_entries`) and `NewItem`.
  - `MediaKind`, `GridIndex` and `scan_watched`.
  - `ThumbCache`, `ThumbService` and `default_workers`, plus `now_ms`.
- Produces: the `cargo bench` targets `startup_grid_100k` and `grid_rows_page`, the `cargo run --example index` CLI, and a CI workflow.

- [ ] **Step 1: Write the benchmark**

Append to `crates/photon-core/Cargo.toml`:

```toml
[[bench]]
name = "grid"
harness = false
```

Create `crates/photon-core/benches/grid.rs`:

```rust
use criterion::{Criterion, criterion_group, criterion_main};
use photon_core::{
    grid::GridIndex,
    library::{Library, NewItem},
    media::MediaKind,
};
use std::{hint::black_box, path::Path};

/// 1,000 folders × 100 photos = 100k items, the spec's target library size.
fn synthetic_library(dir: &Path, folders: usize, per_folder: usize) -> Library {
    let lib = Library::open(&dir.join("bench.db")).unwrap();
    let root = dir.join("photos");
    let watched = lib.add_watched_folder(&root).unwrap();
    let root_id = lib.upsert_folder(watched.id, None, root.to_str().unwrap(), 1).unwrap();
    let mut items = Vec::with_capacity(folders * per_folder);
    for f in 0..folders {
        let folder_path = root.join(format!("folder-{f:04}"));
        let folder_str = folder_path.to_str().unwrap().to_string();
        let folder_id = lib.upsert_folder(watched.id, Some(root_id), &folder_str, 1).unwrap();
        for i in 0..per_folder {
            let name = format!("IMG_{i:05}.jpg");
            items.push(NewItem {
                folder_id,
                path: folder_path.join(&name).to_str().unwrap().to_string(),
                file_name: name,
                kind: MediaKind::Image,
                size: 4_000_000,
                mtime_ms: 1_700_000_000_000 + i as i64,
                width: 4000,
                height: 3000,
                orientation: 1,
                taken_at: 1_700_000_000 + (f * per_folder + i) as i64,
            });
        }
    }
    lib.insert_items(&items).unwrap();
    lib
}

fn bench_grid(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let lib = synthetic_library(dir.path(), 1_000, 100);

    // Spec budget: warm startup to first grid data < 1s.
    c.bench_function("startup_grid_100k", |b| {
        b.iter(|| black_box(GridIndex::build(lib.grid_entries().unwrap())))
    });

    // Spec budget: grid_rows page query < 50ms.
    let index = GridIndex::build(lib.grid_entries().unwrap());
    c.bench_function("grid_rows_page", |b| {
        b.iter(|| black_box(index.rows(black_box(50_000), 200).len()))
    });
}

criterion_group!(benches, bench_grid);
criterion_main!(benches);
```

- [ ] **Step 2: Run the benchmark and check it against the budgets**

Run: `cargo bench -p photon-core --bench grid`
Expected:
- `startup_grid_100k` reports a mean well under **1 s**. If it is over, add `CREATE INDEX` coverage or cache the folder `sort_key` join before continuing.
- `grid_rows_page` reports a mean under **50 ms** (it should be microseconds).

Record both numbers in the commit message.

- [ ] **Step 3: Write the end-to-end example**

Create `crates/photon-core/examples/index.rs`:

```rust
//! Scans a folder into a library and generates all thumbnails.
//! Usage: cargo run --release --example index -- <library.db> <cache-dir> <photo-folder>

use photon_core::{
    grid::GridIndex,
    library::Library,
    now_ms,
    scanner::scan_watched,
    thumbs::{ThumbCache, ThumbService, default_workers},
};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() -> photon_core::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(db), Some(cache), Some(folder)) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: index <library.db> <cache-dir> <photo-folder>");
        std::process::exit(2);
    };

    let lib = Arc::new(Library::open(&PathBuf::from(db))?);
    let watched = lib.add_watched_folder(&PathBuf::from(folder))?;

    let started = Instant::now();
    let report = scan_watched(&lib, &watched, now_ms(), &mut |p| eprint!("\rscanned {} files", p.files_seen))?;
    eprintln!("\n{report:?} in {:?}", started.elapsed());

    let started = Instant::now();
    let grid = GridIndex::build(lib.grid_entries()?);
    eprintln!("grid: {} items in {} sections, built in {:?}", grid.len(), grid.sections().len(), started.elapsed());

    let service = ThumbService::start(lib.clone(), Arc::new(ThumbCache::new(cache)), default_workers());
    let started = Instant::now();
    let queued = service.enqueue_pending()?;
    service.wait_idle();
    eprintln!("thumbnails for {queued} items in {:?}", started.elapsed());
    Ok(())
}
```

- [ ] **Step 4: Run the example against real photos**

Run (pick any folder with JPEGs, e.g. `~/Pictures`):

```bash
S=$(mktemp -d)
cargo run --release -p photon-core --example index -- "$S/library.db" "$S/cache" ~/Pictures
cargo run --release -p photon-core --example index -- "$S/library.db" "$S/cache" ~/Pictures
```

Expected:
- First run: a non-zero `added`, the grid summary, and a thumbnail count equal to `added` minus any failures.
- Second run: `added: 0, changed: 0, unchanged: N`, and `thumbnails for 0 items`, which shows the incremental rescan works.
- `find "$S/cache" -name '*.webp' | head` lists files, and opening one shows a correctly oriented thumbnail.
- `git status` in the photo folder, or checking its mtimes, shows nothing was modified.

- [ ] **Step 5: Add CI**

Create `.github/workflows/ci.yml`:

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

jobs:
  core:
    name: core (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - run: cargo bench -p photon-core --bench grid --no-run
```

- [ ] **Step 6: Lint and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add .github crates/
git commit -m "feat(core): add grid benchmarks, index example and CI matrix

startup_grid_100k: <mean from step 2>
grid_rows_page: <mean from step 2>"
```

If a GitHub remote exists, push the branch and confirm all three CI jobs pass. Fix any platform-specific failures before calling Plan 1 done.

---

## Spec coverage (Plan 1)

| Spec section | Covered by |
|---|---|
| §1 watched folders, never modify files | Tasks 1, 8 (read-only walk), Task 10 step 4 check |
| §3 `library` (sole SQL owner, schema) | Tasks 1–2, 9 |
| §3 `scanner` walk, classify, diff, batch | Task 8 (extension-based classification; magic bytes are sniffed at decode time in Task 4) |
| §3 `metadata` | Task 3 |
| §3 `decode` (pure-Rust formats) | Task 4 (HEIC/AVIF/video → Plan 3) |
| §3 `thumbs` queue, workers, 256/1600 WebP, sync generate | Tasks 5–7 |
| §3 `grid` index, row ranges, sections | Task 9 |
| §5.1 batches of 500, soft delete, mtime fallback | Task 8 |
| §5.2 priority order, fingerprint key, orphan GC | Tasks 5–7 |
| §5.3 `grid_rows`, `grid_sections`, `grid_offset_of_section` | Task 9 core (`rows`, `sections`, `offset_of_folder`); the IPC wrappers come in Plan 2 |
| §5.4 viewer neighbours, orientation | Task 9 `neighbours`; Tasks 4–5 orientation (viewer UI → Plan 2) |
| §5.5 fs watching, startup rescan | Plan 3 (watcher) / Plan 2 (startup rescan wiring) |
| §6 decode failure state, offline folders, DB WAL + writer/reader, migrations refuse newer schema | Tasks 1, 7, 8 |
| §7 unit tests, orientation fixtures, corrupt files, criterion 100k benchmarks, CI matrix | Tasks 1–10 |

Deviation from the spec: thumbnail readiness is not part of the grid rows. The UI learns about failures from `photon://thumb` returning an error (Plan 2), so the grid index does not have to be rebuilt every time a thumbnail finishes.
