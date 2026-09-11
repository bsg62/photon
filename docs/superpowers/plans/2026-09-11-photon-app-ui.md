# photon App Shell + UI (Plan 2 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `photon-core` into a runnable desktop app. That means a Tauri 2 shell with a `photon://` media protocol, typed commands and events, and a Svelte 5 UI with a Picasa-style folder tree, a virtualized square-tile grid and a full-window viewer. The first four tasks are the engine changes deferred from Plan 1.

**Architecture:** `crates/photon-app` holds an `Engine` (library, thumbnail service, grid snapshot, scan manager) as Tauri managed state. The command, protocol and engine logic are plain Rust functions over `Engine`, so they can be tested without a webview. A thin `ipc.rs`/`app.rs` layer wires them into Tauri. The UI lives in `ui/`. Every pixel arrives through `photon://`, and all IPC goes through one module, `ui/src/lib/api.ts`.

**Tech Stack:** Rust (edition 2024, rust-version 1.88), Tauri 2.11, tauri-plugin-dialog 2.7, tauri-plugin-opener 2.5, dunce 1.0. Svelte 5.57 with runes, Vite 8, TypeScript 5.9, Vitest 5, svelte-check 4. The npm workspace lives at the repo root.

**Spec:** `docs/superpowers/specs/2026-09-11-photon-plan2-app-ui-design.md`. Its parent, `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`, stays binding.

## Global Constraints

- **Platforms:** Linux, macOS and Windows. Rust tests must pass on all three. Never hard-code `/` in path assertions; build paths with `Path::join`, or split test names on `/` and join.
- **Read-only watched folders:** photon never writes to, moves or deletes files inside watched folders.
- **Excluded folders:** watched folders are refused when they are equal to or inside an excluded folder (the thumbnail cache dir or the database dir). Folders that contain an excluded folder are allowed, and the scanner skips the excluded part.
- **Path comparison:** by component, and case-insensitive on macOS and Windows only.
- **Thumbnail requests:**
  - The protocol waits at most `30` seconds per thumbnail request.
  - Many requests for one id cause exactly one decode.
- **Commands:**
  - `grid_rows` returns at most `1000` rows.
  - `neighbours` uses a radius of at most `10`.
- **Throttling:** grid rebuilds and `scan-progress` events happen at most every `250` ms per scan.
- **Grid geometry:**
  - tile `160` px square, gap `8` px, section header `32` px;
  - page size `200` items;
  - render overscan of 2 viewport heights;
  - `set_visible` debounced by `150` ms;
  - tile image retried once after `2000` ms.
- **Viewer:** preloads 2 neighbours on each side.
- **Serialization:** IPC field names are camelCase. u64 fingerprints cross IPC as 16-character lowercase hex strings.
- **App identity:**
  - Tauri identifier: `io.github.bsg62.photon`.
  - Window label `main`, title `photon`, size 1280×800, minimum 800×500.
- **Checks every task must pass:**
  - `cargo fmt --all`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - once `ui/` exists: `npm run check` (svelte-check reports 0 errors and 0 warnings) and `npm test`.
- **Commits:** Conventional Commits. Never push. The controller handles remotes.

## File Structure

```
package.json                        npm workspace root (workspaces: ["ui"]) + @tauri-apps/cli
Cargo.toml                          workspace adds crates/photon-app
.github/workflows/ci.yml            rust job (3 OS, Linux webkit deps) + ui job
crates/photon-core/
  src/paths.rs                      NEW: component-wise, case-aware path comparison
  src/error.rs                      + FolderNotFound/FolderOverlap/FolderExcluded/ThumbTimeout/ThumbUnavailable
  src/library/folders.rs            add_watched_folder(path, excluded) validation; register_watched_folder
  src/scanner.rs                    ScanOptions { excluded, cancel }; ScanReport.cancelled
  src/thumbs/queue.rs               in-flight set, done(id), wait_for(id, deadline)
  src/thumbs/service.rs             request(id, size, timeout)
  src/grid.rs                       GridEntry.thumb_key (hex-serialised)
  src/library/items.rs              grid_entries fills thumb_key
crates/photon-app/
  Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/*
  src/main.rs                       calls photon_app::run()
  src/lib.rs                        module declarations + run()
  src/error.rs                      AppError { kind, message }
  src/events.rs                     Events trait, payload structs, test Recorder
  src/engine.rs                     Engine: open, grid snapshot, scans, startup, shutdown
  src/commands.rs                   plain command functions + DTOs
  src/protocol.rs                   photon:// request → http::Response
  src/ipc.rs                        #[tauri::command] wrappers
  src/app.rs                        Tauri builder: plugins, protocol, setup, events impl
  src/testutil.rs                   cfg(test) fixtures
ui/
  package.json, index.html, vite.config.ts, svelte.config.js, tsconfig.json
  src/main.ts, src/app.css, src/App.svelte
  src/lib/url.ts                    mediaUrl()
  src/lib/layout.ts                 row model + hit testing (pure)
  src/lib/pages.ts                  PageCache (pure)
  src/lib/nav.ts                    keyboard navigation (pure)
  src/lib/api.ts                    typed invoke/listen wrappers + TS types
  src/lib/library.svelte.ts         runes store
  src/components/Grid.svelte, Tile.svelte, FolderTree.svelte, Viewer.svelte, StatusBar.svelte, Toasts.svelte
README.md                           dev setup + manual smoke checklist
```

---

### Task 1: Watched-folder validation and path comparison

**Files:**
- Create: `crates/photon-core/src/paths.rs`
- Modify:
  - `crates/photon-core/Cargo.toml` (add `dunce = "1.0.5"`)
  - `src/lib.rs` (add `mod paths;`)
  - `src/error.rs`
  - `src/library/folders.rs`
  - `src/testutil.rs`
  - `src/library/mod.rs` (tests)
  - `src/scanner.rs` (tests only)
  - `examples/index.rs`
  - `benches/grid.rs`

**Interfaces:**
- Consumes: the current `Library` API.
- Produces:
  - `pub(crate) mod paths` with `same_path(a: &Path, b: &Path) -> bool`, `is_within(child: &Path, parent: &Path) -> bool` (true when the paths are equal) and `overlaps(a, b) -> bool`.
  - `Error::FolderNotFound(PathBuf)`, `Error::FolderOverlap { existing: String }` and `Error::FolderExcluded { path: String }`.
  - `Library::add_watched_folder(&self, path: &Path, excluded: &[PathBuf]) -> Result<WatchedFolder>`.
  - `pub(crate) Library::register_watched_folder(&self, path: &str) -> Result<WatchedFolder>`, which inserts without validation.
  - `WatchedFolder` and `Folder` serialise as camelCase (`watchedId`, `parentId`).
  - `testutil::watch(&Library, &str) -> WatchedFolder`.

- [ ] **Step 1: Write the `paths` tests**

Create `crates/photon-core/src/paths.rs` containing only the tests, and add `mod paths;` to `lib.rs`:

```rust
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
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core paths`
Expected: compile error, because `is_within`, `overlaps` and `same_path` are not defined.

- [ ] **Step 3: Implement `paths.rs`**

Prepend:

```rust
//! Path comparison for watched-folder rules: component-wise, and case-insensitive on
//! the platforms whose default filesystems are (macOS, Windows).

use std::path::{Component, Path};

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
```

Run: `cargo test -p photon-core paths`. Expected: PASS.

- [ ] **Step 4: Add the error variants**

In `crates/photon-core/src/error.rs`, add these after `ThumbFailed`:

```rust
    #[error("folder not found: {0:?}")]
    FolderNotFound(PathBuf),
    #[error("folder overlaps the watched folder {existing}")]
    FolderOverlap { existing: String },
    #[error("{path} is used by photon itself and cannot be watched")]
    FolderExcluded { path: String },
```

- [ ] **Step 5: Write the failing validation tests**

Add `dunce = "1.0.5"` to `[dependencies]` in `crates/photon-core/Cargo.toml`. Then add to the `tests` module in `crates/photon-core/src/library/folders.rs`:

```rust
    use crate::Error;
    use std::path::PathBuf;

    /// Creates each directory under `root` and returns its canonical path.
    fn dirs(root: &Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| {
                let path = name.split('/').fold(root.to_path_buf(), |p, part| p.join(part));
                std::fs::create_dir_all(&path).unwrap();
                dunce::canonicalize(path).unwrap()
            })
            .collect()
    }

    #[test]
    fn add_watched_folder_canonicalises_and_dedupes() {
        let (dir, lib) = temp_library();
        let d = dirs(dir.path(), &["photos"]);
        let a = lib
            .add_watched_folder(&dir.path().join("photos").join("."), &[])
            .unwrap();
        assert_eq!(Path::new(&a.path), d[0].as_path());
        assert_eq!(lib.add_watched_folder(&d[0], &[]).unwrap(), a);
        assert_eq!(lib.watched_folders().unwrap().len(), 1);
    }

    #[test]
    fn add_watched_folder_rejects_missing_and_overlapping_folders() {
        let (dir, lib) = temp_library();
        let d = dirs(dir.path(), &["photos/2024", "other"]);
        let photos = d[0].parent().unwrap().to_path_buf();
        lib.add_watched_folder(&photos, &[]).unwrap();

        assert!(matches!(
            lib.add_watched_folder(&d[0], &[]),
            Err(Error::FolderOverlap { .. })
        ));
        assert!(matches!(
            lib.add_watched_folder(dir.path(), &[]),
            Err(Error::FolderOverlap { .. })
        ));
        assert!(lib.add_watched_folder(&d[1], &[]).is_ok());
        assert!(matches!(
            lib.add_watched_folder(&dir.path().join("missing"), &[]),
            Err(Error::FolderNotFound(_))
        ));
    }

    #[test]
    fn add_watched_folder_rejects_photons_own_directories() {
        let (dir, lib) = temp_library();
        let d = dirs(dir.path(), &["cache/thumbs"]);
        let cache = d[0].parent().unwrap().to_path_buf();
        let excluded = [cache.clone()];
        assert!(matches!(
            lib.add_watched_folder(&cache, &excluded),
            Err(Error::FolderExcluded { .. })
        ));
        assert!(matches!(
            lib.add_watched_folder(&d[0], &excluded),
            Err(Error::FolderExcluded { .. })
        ));
        // A folder containing an excluded one is fine: the scanner skips the excluded part.
        assert!(lib.add_watched_folder(dir.path(), &excluded).is_ok());
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn duplicate_detection_ignores_case() {
        let (dir, lib) = temp_library();
        let d = dirs(dir.path(), &["Photos"]);
        let a = lib.add_watched_folder(&d[0], &[]).unwrap();
        let again = lib.add_watched_folder(&dir.path().join("PHOTOS"), &[]).unwrap();
        assert_eq!(again.id, a.id);
    }

    #[test]
    fn folders_serialise_as_camel_case() {
        let folder = Folder {
            id: 1,
            watched_id: 2,
            parent_id: None,
            path: "p".into(),
            name: "p".into(),
        };
        let json = serde_json::to_string(&folder).unwrap();
        assert!(json.contains("\"watchedId\":2") && json.contains("\"parentId\":null"));
    }
```

- [ ] **Step 6: Run the tests and confirm they fail**

Run: `cargo test -p photon-core folders`
Expected: compile errors, because `add_watched_folder` still takes one argument.

- [ ] **Step 7: Implement validation**

In `crates/photon-core/src/library/folders.rs`:

- Add `use crate::paths;` and `use std::path::PathBuf;`.
- Add `#[serde(rename_all = "camelCase")]` to both `WatchedFolder` and `Folder`.
- Replace `add_watched_folder` with:

```rust
    /// Registers a folder to watch after validating it. The path is canonicalised, and adding
    /// an already watched folder returns the existing entry. A folder that contains or sits
    /// inside another watched folder is refused, as is anything equal to or inside
    /// photon's own directories in `excluded`.
    pub fn add_watched_folder(&self, path: &Path, excluded: &[PathBuf]) -> Result<WatchedFolder> {
        let canonical =
            dunce::canonicalize(path).map_err(|_| Error::FolderNotFound(path.to_path_buf()))?;
        if !canonical.is_dir() {
            return Err(Error::FolderNotFound(path.to_path_buf()));
        }
        for ex in excluded {
            let ex = dunce::canonicalize(ex).unwrap_or_else(|_| ex.clone());
            if paths::is_within(&canonical, &ex) {
                return Err(Error::FolderExcluded {
                    path: ex.display().to_string(),
                });
            }
        }
        for existing in self.watched_folders()? {
            let existing_path = Path::new(&existing.path);
            if paths::same_path(&canonical, existing_path) {
                return Ok(existing);
            }
            if paths::overlaps(&canonical, existing_path) {
                return Err(Error::FolderOverlap {
                    existing: existing.path,
                });
            }
        }
        let path_str = canonical
            .to_str()
            .ok_or_else(|| Error::NonUtf8Path(canonical.clone()))?;
        self.register_watched_folder(path_str)
    }

    /// Inserts a watched-folder row as given, without validation. Used by
    /// `add_watched_folder` and by tests that work with synthetic paths.
    pub(crate) fn register_watched_folder(&self, path: &str) -> Result<WatchedFolder> {
        let conn = self.writer();
        conn.execute(
            "INSERT OR IGNORE INTO watched_folders (path) VALUES (?1)",
            params![path],
        )?;
        let watched = conn.query_row(
            "SELECT id, path, online FROM watched_folders WHERE path = ?1",
            params![path],
            row_to_watched,
        )?;
        Ok(watched)
    }
```

- [ ] **Step 8: Update the callers**

Change each caller as follows:

- **`src/testutil.rs`:** add this helper, and make `seed_folder` use it instead of `add_watched_folder`:

  ```rust
  pub fn watch(lib: &Library, path: &str) -> crate::library::WatchedFolder {
      lib.register_watched_folder(path).unwrap()
  }
  ```

  Inside `seed_folder`: `let watched = watch(lib, path.to_str().unwrap());`

- **Tests in `src/library/mod.rs` and `src/library/folders.rs` that use synthetic paths:** replace every `lib.add_watched_folder(Path::new("X")).unwrap()` with `watch(&lib, "X")`, importing `crate::testutil::watch`.

- **Tests in `src/scanner.rs`:**
  - Every `lib.add_watched_folder(&root).unwrap()` becomes `lib.add_watched_folder(&root, &[]).unwrap()`.
  - Every `let root = dir.path().join("photos");` becomes `let root = photos_root(&dir);`, using this helper in the tests module:

    ```rust
    /// The watched root, canonicalised the way `add_watched_folder` will store it (on macOS
    /// the temp dir lives behind the /var → /private/var symlink).
    fn photos_root(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let root = dir.path().join("photos");
        std::fs::create_dir_all(&root).unwrap();
        dunce::canonicalize(root).unwrap()
    }
    ```

  - `unreachable_folder_goes_offline_and_keeps_items` renames `root` after adding it. That still works, because `root` is already canonical.

- **`examples/index.rs`:** `lib.add_watched_folder(&PathBuf::from(folder), &[PathBuf::from(&cache)])?`. Keep a copy of `cache` before it is moved into `ThumbCache::new`, e.g. `ThumbCache::new(cache.clone())`.

- **`benches/grid.rs`:** add `std::fs::create_dir_all(&root).unwrap();` before `lib.add_watched_folder(&root, &[]).unwrap()`.

- [ ] **Step 9: Run everything**

Run: `cargo test -p photon-core && cargo bench -p photon-core --bench grid --no-run && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all tests pass (64 existing plus the new ones), and the build and clippy are clean.

- [ ] **Step 10: Commit**

```bash
git add crates/photon-core
git commit -m "feat(core): validate watched folders against overlaps and photon's own directories"
```

---

### Task 2: Scanner exclusions and cancellation

**Files:**
- Modify: `crates/photon-core/src/scanner.rs` and `crates/photon-core/examples/index.rs`

**Interfaces:**
- Consumes: `paths::is_within` (Task 1).
- Produces:
  - `scanner::ScanOptions { pub excluded: Vec<PathBuf>, pub cancel: Arc<AtomicBool> }` (derives `Clone`, `Debug`, `Default`).
  - `ScanReport.cancelled: bool`.
  - `scan_watched(lib, watched, scan_id, options: &ScanOptions, progress) -> Result<ScanReport>`.
  - A cancelled scan returns `cancelled: true`, and never marks, purges or prunes anything.

- [ ] **Step 1: Write failing tests**

In the `scanner.rs` tests module, change the `scan` helper to `scan_watched(lib, watched, scan_id, &ScanOptions::default(), &mut |_| {}).unwrap()`. Update the two direct `scan_watched` calls the same way, passing `&ScanOptions::default()`. Then add:

```rust
    #[test]
    fn excluded_subtrees_are_not_indexed() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "cache/b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        let options = ScanOptions {
            excluded: vec![root.join("cache")],
            ..ScanOptions::default()
        };
        let report = scan_watched(&lib, &watched, 1, &options, &mut |_| {}).unwrap();
        assert_eq!(report.added, 1);
        assert!(lib.folders().unwrap().iter().all(|f| f.name != "cache"));
    }

    #[test]
    fn cancelled_scan_leaves_unseen_items_alone() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(8, 8));
        let b = write_file(&root, "b.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        fs::remove_file(&b).unwrap();

        let options = ScanOptions::default();
        options.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let report = scan_watched(&lib, &watched, 2, &options, &mut |_| {}).unwrap();

        assert!(report.cancelled);
        assert_eq!((report.marked_missing, report.purged), (0, 0));
        let id = lib.known_items(watched.id).unwrap()[&key(&b)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core scanner`
Expected: compile errors, because `ScanOptions` does not exist and `scan_watched` still has the old signature.

- [ ] **Step 3: Implement**

In `crates/photon-core/src/scanner.rs`:

```rust
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

/// Per-scan settings.
#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    /// Directories never walked, e.g. photon's own cache and database folders.
    pub excluded: Vec<PathBuf>,
    /// Checked before each entry; once set, the scan stops without marking anything missing.
    pub cancel: Arc<AtomicBool>,
}
```

- Add `pub cancelled: bool` to `ScanReport`.
- Add the `options: &ScanOptions` parameter after `scan_id`, and document it in the doc comment.
- Change the walker filter to:

```rust
        .filter_entry(|e| {
            (e.depth() == 0 || !is_hidden(e))
                && !options
                    .excluded
                    .iter()
                    .any(|x| crate::paths::is_within(e.path(), x))
        });
```

- Add `let mut cancelled = false;` next to the other locals. Make this the first statement inside the `for entry in walker` loop:

```rust
        if options.cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
```

- Right after the two final `flush_*` calls, before `if skip_mark_purge`, add:

```rust
    if cancelled {
        // We stopped early, so everything we didn't reach is unknown, not missing.
        progress(&seen);
        return Ok(ScanReport {
            cancelled: true,
            ..report
        });
    }
```

- In `examples/index.rs`, pass `&ScanOptions { excluded: vec![PathBuf::from(&cache)], ..Default::default() }` to `scan_watched`, and import `photon_core::scanner::ScanOptions`.

- [ ] **Step 4: Run everything**

Run: `cargo test -p photon-core && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/photon-core
git commit -m "feat(core): let scans skip excluded directories and stop on cancel"
```

---

### Task 3: Waiting thumbnail requests with shared decodes

**Files:**
- Modify:
  - `crates/photon-core/src/error.rs`
  - `crates/photon-core/src/thumbs/queue.rs`
  - `crates/photon-core/src/thumbs/service.rs`

**Interfaces:**
- Consumes: `ThumbQueue`, `ThumbService`, `Library::item`.
- Produces:
  - `Error::ThumbTimeout(i64)` and `Error::ThumbUnavailable(i64)`.
  - `ThumbQueue::done(&self, id: i64)`. It replaces `done()`.
  - `ThumbQueue::wait_for(&self, id: i64, deadline: Instant) -> bool`. It returns false only on timeout.
  - `ThumbQueue::push` ignores ids that are currently being processed.
  - `ThumbService::request(&self, id: i64, size: ThumbSize, timeout: Duration) -> Result<PathBuf>`.
  - Errors from `request`: `NotFound` for an unknown or missing item; `ThumbFailed(msg)` for a failed item; `ThumbTimeout` for a timeout; `ThumbUnavailable` when the source can't be read right now.

- [ ] **Step 1: Write failing queue tests**

In the `queue.rs` tests:

- Change every existing `q.done()` to `q.done(id)`, using the id returned by the preceding `pop_blocking()`. In `drain`: `let id = q.pop_blocking().unwrap(); out.push(id); q.done(id);`. In `wait_idle_waits_for_in_flight_work`, the worker thread calls `q.done(id)`.
- Add:

```rust
    use std::time::{Duration, Instant};

    #[test]
    fn push_skips_items_in_flight() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Background);
        let id = q.pop_blocking().unwrap();
        q.push(1, Priority::Visible);
        assert!(q.is_empty());
        q.done(id);
        q.push(1, Priority::Visible);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn wait_for_returns_once_the_job_finishes() {
        let q = Arc::new(ThumbQueue::new());
        q.push(1, Priority::Visible);
        let worker = {
            let q = q.clone();
            std::thread::spawn(move || {
                let id = q.pop_blocking().unwrap();
                std::thread::sleep(Duration::from_millis(50));
                q.done(id);
            })
        };
        assert!(q.wait_for(1, Instant::now() + Duration::from_secs(5)));
        worker.join().unwrap();
    }

    #[test]
    fn wait_for_times_out_while_queued() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Visible);
        assert!(!q.wait_for(1, Instant::now() + Duration::from_millis(50)));
    }

    #[test]
    fn wait_for_unknown_id_returns_immediately() {
        assert!(ThumbQueue::new().wait_for(7, Instant::now()));
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core queue`
Expected: compile errors, because `done` takes no argument and `wait_for` does not exist.

- [ ] **Step 3: Implement the queue changes**

In `queue.rs`:

- In `State`, replace `active: usize` with `in_flight: HashSet<i64>`, and import `HashSet`.
- Make the first line of `State::push` `if self.in_flight.contains(&id) { return; }`.
- In `pop_blocking`, replace `state.active += 1;` with `state.in_flight.insert(id);`.
- Replace `done` with:

```rust
    /// Marks the popped job `id` finished and wakes idle-waiters and `wait_for` callers.
    pub fn done(&self, id: i64) {
        self.state.lock().in_flight.remove(&id);
        self.changed.notify_all();
    }
```

- In `wait_idle`, change the condition to `state.order.is_empty() && state.in_flight.is_empty()`.
- Add:

```rust
    /// Blocks until `id` is neither queued nor being processed, the queue closes, or
    /// `deadline` passes. Returns false only on timeout.
    pub fn wait_for(&self, id: i64, deadline: std::time::Instant) -> bool {
        let mut state = self.state.lock();
        loop {
            let busy = state.entries.contains_key(&id) || state.in_flight.contains(&id);
            if state.closed || !busy {
                return true;
            }
            if self.changed.wait_until(&mut state, deadline).timed_out() {
                let busy = state.entries.contains_key(&id) || state.in_flight.contains(&id);
                return !busy;
            }
        }
    }
```

In `service.rs`, change `DoneGuard` to carry the id. Use `struct DoneGuard<'a>(&'a ThumbQueue, i64);` with `Drop` calling `self.0.done(self.1)`, and create it in the worker as `let _guard = DoneGuard(&queue, id);`.

Run: `cargo test -p photon-core thumbs`. Expected: PASS.

- [ ] **Step 4: Write failing `request` tests**

Add to `crates/photon-core/src/error.rs`:

```rust
    #[error("thumbnail for item {0} timed out")]
    ThumbTimeout(i64),
    #[error("thumbnail for item {0} is temporarily unavailable")]
    ThumbUnavailable(i64),
```

Add to the `service.rs` tests:

```rust
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static COUNTED_RENDERS: AtomicUsize = AtomicUsize::new(0);

    fn counting_render(cache: &ThumbCache, source: &Path, orientation: u8) -> Result<(DynamicImage, DynamicImage)> {
        COUNTED_RENDERS.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        cache.render(source, orientation)
    }

    fn slow_render(cache: &ThumbCache, source: &Path, orientation: u8) -> Result<(DynamicImage, DynamicImage)> {
        std::thread::sleep(Duration::from_millis(500));
        cache.render(source, orientation)
    }

    #[test]
    fn concurrent_requests_share_one_decode() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = Arc::new(ThumbService::start_with(lib, cache, 2, counting_render));
        let id = ids[0];
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let service = service.clone();
                std::thread::spawn(move || service.request(id, ThumbSize::Grid, Duration::from_secs(10)))
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap().unwrap().is_file());
        }
        assert_eq!(COUNTED_RENDERS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn request_times_out_but_keeps_the_job() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, slow_render);
        assert!(matches!(
            service.request(ids[0], ThumbSize::Grid, Duration::from_millis(50)),
            Err(Error::ThumbTimeout(_))
        ));
        service.wait_idle();
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
    }

    #[test]
    fn request_reports_failed_unknown_and_unreadable_items() {
        let (dir, lib, cache, ids) = setup(&[("bad.jpg", b"garbage".to_vec()), ("gone.jpg", jpeg_bytes(8, 8))]);
        std::fs::remove_file(dir.path().join("photos").join("gone.jpg")).unwrap();
        let service = ThumbService::start(lib, cache, 1);
        let t = Duration::from_secs(10);
        assert!(matches!(service.request(ids[0], ThumbSize::Grid, t), Err(Error::ThumbFailed(_))));
        assert!(matches!(service.request(ids[1], ThumbSize::Grid, t), Err(Error::ThumbUnavailable(_))));
        assert!(matches!(service.request(9_999, ThumbSize::Grid, t), Err(Error::NotFound(9_999))));
    }
```

- [ ] **Step 5: Run the tests and confirm they fail**

Run: `cargo test -p photon-core service`
Expected: compile error, because `request` is not defined.

- [ ] **Step 6: Implement `request`**

Add `use std::time::{Duration, Instant};` to `service.rs`, and add to `impl ThumbService`:

```rust
    /// Returns the cached thumbnail, or moves the item to the front of the queue and waits
    /// for a worker to build it. Concurrent requests for one item share the same decode,
    /// and CPU use stays within the worker pool. Used by the `photon://` protocol.
    pub fn request(&self, id: i64, size: ThumbSize, timeout: Duration) -> Result<PathBuf> {
        let deadline = Instant::now() + timeout;
        // Two rounds: the first may only wait out a job already running for an older
        // version of the file.
        for _ in 0..2 {
            let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
            if item.missing_since.is_some() {
                return Err(Error::NotFound(id));
            }
            if item.thumb_state == ThumbState::Failed {
                return Err(Error::ThumbFailed(item.thumb_error.unwrap_or_default()));
            }
            let path = self.cache.path_for(item.fingerprint(), size);
            if path.is_file() {
                return Ok(path);
            }
            self.queue.push(id, Priority::Visible);
            if !self.queue.wait_for(id, deadline) {
                return Err(Error::ThumbTimeout(id));
            }
        }
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.thumb_state == ThumbState::Failed {
            return Err(Error::ThumbFailed(item.thumb_error.unwrap_or_default()));
        }
        let path = self.cache.path_for(item.fingerprint(), size);
        if path.is_file() {
            Ok(path)
        } else {
            Err(Error::ThumbUnavailable(id))
        }
    }
```

- [ ] **Step 7: Run everything**

Run: `for i in 1 2 3; do cargo test -p photon-core thumbs || break; done && cargo test -p photon-core && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS on every repeat, with no flakiness.

- [ ] **Step 8: Commit**

```bash
git add crates/photon-core
git commit -m "feat(core): add waiting thumbnail requests that share one decode per item"
```

---

### Task 4: Thumbnail cache key in grid rows

**Files:**
- Modify: `crates/photon-core/src/grid.rs` and `crates/photon-core/src/library/items.rs`

**Interfaces:**
- Produces:
  - `GridEntry.thumb_key: u64`, the item's fingerprint. It serialises as `"thumbKey": "<16 lowercase hex>"`.
  - `pub fn hex_key(value: u64) -> String` in `grid.rs`. The app reuses it.

- [ ] **Step 1: Write the failing tests**

In the `grid.rs` tests, add `thumb_key: 42` to `entry()`'s `GridEntry`. Then change the serialisation assertion to:

```rust
        let json = serde_json::to_string(&entry(7, 1)).unwrap();
        assert_eq!(
            json,
            r#"{"id":7,"folderId":1,"takenAt":7,"aspect":1.5,"kind":"image","thumbKey":"000000000000002a"}"#
        );
```

In `items.rs`, extend `grid_entries_are_ordered_oriented_and_skip_missing` with:

```rust
        let expected = lib.item(ids[2]).unwrap().unwrap().fingerprint();
        assert_eq!(entries[0].thumb_key, expected);
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core grid`
Expected: compile error, because `thumb_key` is not a field.

- [ ] **Step 3: Implement**

In `grid.rs`:

```rust
use serde::{Serialize, Serializer};

/// A u64 as 16 lowercase hex characters: exact in JavaScript, unlike a JSON number.
pub fn hex_key(value: u64) -> String {
    format!("{value:016x}")
}

fn serialize_hex<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&hex_key(*value))
}
```

Add this field to `GridEntry`, after `kind`:

```rust
    /// Fingerprint of the file version. Part of thumbnail URLs, so they can be cached forever.
    #[serde(serialize_with = "serialize_hex")]
    pub thumb_key: u64,
```

In `items.rs` `grid_entries`:

- Extend the SELECT with `, i.path, i.size, i.mtime_ms`.
- In the row closure, set `thumb_key: fingerprint(&r.get::<_, String>(7)?, r.get(8)?, r.get(9)?)`.

- [ ] **Step 4: Run everything, including the benchmark budget**

Run: `cargo test -p photon-core && cargo bench -p photon-core --bench grid -- startup_grid_100k && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: tests pass, and `startup_grid_100k` stays under 1 s (it measured about 37 ms before this change).

- [ ] **Step 5: Commit**

```bash
git add crates/photon-core
git commit -m "feat(core): carry each item's fingerprint in grid rows as a cache key"
```

---

### Task 5: App and UI scaffold

**Files:**
- Create:
  - `package.json`
  - `ui/package.json`, `ui/index.html`, `ui/vite.config.ts`, `ui/svelte.config.js`, `ui/tsconfig.json`
  - `ui/src/main.ts`, `ui/src/app.css`, `ui/src/App.svelte`
  - `ui/src/lib/url.ts`, `ui/src/lib/url.test.ts`
  - `crates/photon-app/Cargo.toml`, `crates/photon-app/build.rs`, `crates/photon-app/tauri.conf.json`
  - `crates/photon-app/capabilities/default.json`
  - `crates/photon-app/icons/*` (generated)
  - `crates/photon-app/src/main.rs`, `crates/photon-app/src/lib.rs`
- Modify: `Cargo.toml` (workspace members) and `.gitignore`

**Interfaces:**
- Produces:
  - An npm workspace. From the repo root, `npm test`, `npm run check` and `npm run tauri <args>` work.
  - `ui/src/lib/url.ts` exports `mediaUrl(path: string, windows?: boolean): string`.
  - The `photon-app` crate: `photon_app::run()` builds and launches an empty window.

- [ ] **Step 1: Write the root and UI manifests**

`package.json` (repo root):

```json
{
  "name": "photon",
  "private": true,
  "workspaces": ["ui"],
  "scripts": {
    "tauri": "tauri",
    "dev": "tauri dev",
    "test": "npm run -w ui test",
    "check": "npm run -w ui check"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.11.4"
  }
}
```

`ui/package.json`:

```json
{
  "name": "photon-ui",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "check": "svelte-check --tsconfig ./tsconfig.json --fail-on-warnings",
    "test": "vitest run"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.11.1",
    "@tauri-apps/plugin-dialog": "^2.7.3"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^7.3.0",
    "@tsconfig/svelte": "^5.0.8",
    "svelte": "^5.57.0",
    "svelte-check": "^4.7.6",
    "typescript": "^5.9.3",
    "vite": "^8.3.0",
    "vitest": "^5.0.0"
  }
}
```

If npm reports peer-dependency conflicts between these versions (for example, whether vite-plugin-svelte 7 supports vite 8), choose the newest mutually compatible set and record it in your report. TypeScript stays on 5.x because svelte-check targets the TS 5 compiler API.

`ui/vite.config.ts`:

```ts
/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: 'es2022' },
  test: { include: ['src/**/*.test.ts'], environment: 'node' },
});
```

`ui/svelte.config.js`:

```js
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default { preprocess: vitePreprocess() };
```

`ui/tsconfig.json`:

```json
{
  "extends": "@tsconfig/svelte/tsconfig.json",
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "verbatimModuleSyntax": true,
    "isolatedModules": true,
    "types": ["vite/client"]
  },
  "include": ["src/**/*.ts", "src/**/*.svelte", "vite.config.ts"]
}
```

`ui/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>photon</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`ui/src/main.ts`:

```ts
import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';

mount(App, { target: document.getElementById('app')! });
```

`ui/src/app.css`:

```css
:root {
  --bg: #1d1f23;
  --panel: #25282d;
  --panel-2: #2d3036;
  --text: #e6e6e6;
  --muted: #9aa0a6;
  --accent: #4c8dff;
  --danger: #ff6b6b;
  color-scheme: dark;
  font-family: system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
  font-size: 14px;
  color: var(--text);
  background: var(--bg);
}
* { box-sizing: border-box; }
html, body, #app { margin: 0; height: 100%; overflow: hidden; }
button { font: inherit; color: inherit; }
```

`ui/src/App.svelte` (a placeholder, replaced in Task 13):

```svelte
<main class="placeholder">photon</main>

<style>
  .placeholder { display: grid; place-items: center; height: 100%; color: var(--muted); }
</style>
```

- [ ] **Step 2: Write the failing `mediaUrl` test**

`ui/src/lib/url.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { mediaUrl } from './url';

describe('mediaUrl', () => {
  it('uses the custom scheme on Linux and macOS', () => {
    expect(mediaUrl('thumb/1/grid/00ff', false)).toBe('photon://localhost/thumb/1/grid/00ff');
  });
  it('uses the http localhost form on Windows', () => {
    expect(mediaUrl('image/7', true)).toBe('http://photon.localhost/image/7');
  });
});
```

Run: `npm install && npm test`
Expected: FAIL, because `./url` does not exist.

- [ ] **Step 3: Implement `url.ts`**

```ts
/** Tauri serves custom schemes as `scheme://localhost/…`, except on Windows (WebView2),
 *  where they are `http://scheme.localhost/…`. */
export function isWindows(): boolean {
  return typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows');
}

/** URL for a path served by the `photon://` protocol, e.g. `thumb/12/grid/<key>`. */
export function mediaUrl(path: string, windows: boolean = isWindows()): string {
  return windows ? `http://photon.localhost/${path}` : `photon://localhost/${path}`;
}
```

Run: `npm test && npm run check && npm run -w ui build`
Expected: the tests pass, svelte-check reports 0 errors and 0 warnings, and `ui/dist` is built.

- [ ] **Step 4: Create the Tauri crate**

In the root `Cargo.toml`, set `members = ["crates/photon-core", "crates/photon-app"]`.

Append to `.gitignore`:

```gitignore
node_modules/
ui/dist/
crates/photon-app/gen/
```

`crates/photon-app/Cargo.toml`:

```toml
[package]
name = "photon-app"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[build-dependencies]
tauri-build = { version = "2.6.3", features = [] }

[dependencies]
photon-core = { path = "../photon-core" }
parking_lot = "0.12.5"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1"
tauri = { version = "2.11.5", features = [] }
tauri-plugin-dialog = "2.7.3"
tauri-plugin-opener = "2.5.5"
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }

[dev-dependencies]
image = { version = "0.25.10", default-features = false, features = ["jpeg"] }
tempfile = "3.27.0"
```

`crates/photon-app/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

`crates/photon-app/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "photon",
  "version": "0.1.0",
  "identifier": "io.github.bsg62.photon",
  "build": {
    "devUrl": "http://localhost:5173",
    "frontendDist": "../../ui/dist",
    "beforeDevCommand": "npm run -w ui dev",
    "beforeBuildCommand": "npm run -w ui build"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "photon",
        "width": 1280,
        "height": 800,
        "minWidth": 800,
        "minHeight": 500
      }
    ],
    "security": {
      "csp": "default-src 'self'; img-src 'self' photon: http://photon.localhost data: blob:; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost photon: http://photon.localhost"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

`crates/photon-app/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Permissions for the main window",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:allow-open", "dialog:allow-ask"]
}
```

`crates/photon-app/src/main.rs`:

```rust
// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    photon_app::run();
}
```

`crates/photon-app/src/lib.rs` (a placeholder; Task 9 replaces `run`):

```rust
//! photon-app: the Tauri shell around photon-core.

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running photon");
}
```

- [ ] **Step 5: Generate the icons**

```bash
S=/tmp/claude-1000/-home-dh-Projects-photon/0961354f-2d3b-47d1-8c24-a7fcf6de21fe/scratchpad
magick -size 1024x1024 xc:none -fill '#4c8dff' -draw 'roundrectangle 64,64 960,960 180,180' \
  -fill white -draw 'circle 512,512 512,300' "$S/icon-source.png"
npm run tauri icon -- "$S/icon-source.png" -o crates/photon-app/icons
```

Expected: `crates/photon-app/icons/` contains `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico` and more. Commit all of them.

- [ ] **Step 6: Build**

Run: `cargo build -p photon-app && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: the build succeeds. If `generate_context!` complains that `frontendDist` doesn't exist, run `npm run -w ui build` first and note in your report that the build needs it. Do not launch the app, because it opens a window on the user's desktop.

- [ ] **Step 7: Commit**

```bash
git add package.json package-lock.json ui Cargo.toml Cargo.lock .gitignore crates/photon-app
git commit -m "chore(app): scaffold the Tauri shell and the Svelte UI workspace"
```

---

### Task 6: Engine, events and scan manager

**Files:**
- Create: `crates/photon-app/src/events.rs`, `crates/photon-app/src/engine.rs`, `crates/photon-app/src/testutil.rs`
- Modify: `crates/photon-app/src/lib.rs` (module declarations)

**Interfaces:**
- Consumes: photon-core, including Task 1's `add_watched_folder(path, excluded)`, Task 2's `ScanOptions` and Task 3's `ThumbService::request`.
- Produces:
  - `events::{LibraryChanged { version: u64, len: usize }, ScanProgressEvent { watched_id, files_seen, added, changed, done, cancelled }, FolderStatus { watched_id, online }}`. All serialise as camelCase.
  - `trait Events: Send + Sync + 'static { fn library_changed(&self, LibraryChanged); fn scan_progress(&self, ScanProgressEvent); fn folder_status(&self, FolderStatus); }`.
  - `events::Recorder`, which is `cfg(test)`.
  - `engine::EngineConfig { db_path, cache_dir, workers }`.
  - `engine::Engine` with:
    - `open(EngineConfig, Arc<dyn Events>) -> photon_core::Result<Arc<Engine>>`;
    - public fields `lib: Arc<Library>` and `thumbs: ThumbService`;
    - `excluded() -> &[PathBuf]`;
    - `grid() -> (u64, Arc<GridIndex>)`;
    - `refresh_grid() -> Result<()>`;
    - `add_folder(self: &Arc<Self>, &Path) -> Result<WatchedFolder>`;
    - `remove_folder(&self, i64) -> Result<()>`;
    - `start_scan(self: &Arc<Self>, WatchedFolder) -> bool`;
    - `cancel_scan(&self, i64)`;
    - `is_scanning(&self, i64) -> bool`;
    - `wait_for_scans(&self)`;
    - `startup(self: &Arc<Self>, pictures: Option<PathBuf>) -> JoinHandle<()>`;
    - `shutdown(&self)`.
  - `testutil::{jpeg, Fixture, fixture}`.

- [ ] **Step 1: Write the events module and test helpers**

`crates/photon-app/src/events.rs`:

```rust
//! Events the engine sends to the UI. The Tauri implementation lives in `app.rs`.

use photon_core::scanner::ScanProgress;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChanged {
    pub version: u64,
    pub len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgressEvent {
    pub watched_id: i64,
    pub files_seen: u64,
    pub added: u64,
    pub changed: u64,
    pub done: bool,
    pub cancelled: bool,
}

impl ScanProgressEvent {
    pub fn new(watched_id: i64, p: &ScanProgress, done: bool, cancelled: bool) -> Self {
        Self {
            watched_id,
            files_seen: p.files_seen,
            added: p.added,
            changed: p.changed,
            done,
            cancelled,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderStatus {
    pub watched_id: i64,
    pub online: bool,
}

pub trait Events: Send + Sync + 'static {
    fn library_changed(&self, event: LibraryChanged);
    fn scan_progress(&self, event: ScanProgressEvent);
    fn folder_status(&self, event: FolderStatus);
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recorded {
    Library(LibraryChanged),
    Scan(ScanProgressEvent),
    Folder(FolderStatus),
}

/// Test sink that keeps every event.
#[cfg(test)]
#[derive(Default)]
pub struct Recorder(pub parking_lot::Mutex<Vec<Recorded>>);

#[cfg(test)]
impl Recorder {
    pub fn all(&self) -> Vec<Recorded> {
        self.0.lock().clone()
    }
}

#[cfg(test)]
impl Events for Recorder {
    fn library_changed(&self, e: LibraryChanged) {
        self.0.lock().push(Recorded::Library(e));
    }
    fn scan_progress(&self, e: ScanProgressEvent) {
        self.0.lock().push(Recorded::Scan(e));
    }
    fn folder_status(&self, e: FolderStatus) {
        self.0.lock().push(Recorded::Folder(e));
    }
}
```

`crates/photon-app/src/testutil.rs`:

```rust
#![allow(dead_code)]

use crate::engine::{Engine, EngineConfig};
use crate::events::Recorder;
use photon_core::library::WatchedFolder;
use std::{io::Cursor, path::PathBuf, sync::Arc};
use tempfile::TempDir;

pub fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([90, 120, 200])));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg).unwrap();
    buf
}

pub struct Fixture {
    pub dir: TempDir,
    pub photos: PathBuf,
    pub events: Arc<Recorder>,
    pub engine: Arc<Engine>,
}

impl Fixture {
    pub fn config(&self) -> EngineConfig {
        config_in(&self.dir)
    }

    /// Watches `photos` and waits for its first scan to finish.
    pub fn add_photos(&self) -> WatchedFolder {
        let watched = self.engine.add_folder(&self.photos).unwrap();
        self.engine.wait_for_scans();
        watched
    }

    /// Item ids in grid order.
    pub fn ids(&self) -> Vec<i64> {
        let (_, grid) = self.engine.grid();
        grid.rows(0, grid.len()).iter().map(|e| e.id).collect()
    }
}

fn config_in(dir: &TempDir) -> EngineConfig {
    EngineConfig {
        db_path: dir.path().join("data").join("library.db"),
        cache_dir: dir.path().join("cache").join("thumbs"),
        workers: 1,
    }
}

/// A temp dir with `photos/` holding `files` (names may contain '/') and an engine whose
/// data and cache live elsewhere in the same temp dir.
pub fn fixture(files: &[(&str, &[u8])]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    for (name, bytes) in files {
        let path = name.split('/').fold(photos.clone(), |p, part| p.join(part));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let events = Arc::new(Recorder::default());
    let engine = Engine::open(config_in(&dir), events.clone()).unwrap();
    Fixture { dir, photos, events, engine }
}
```

Set `crates/photon-app/src/lib.rs` to:

```rust
//! photon-app: the Tauri shell around photon-core.

pub mod engine;
pub mod events;

#[cfg(test)]
mod testutil;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running photon");
}
```

- [ ] **Step 2: Write failing engine tests**

Create `crates/photon-app/src/engine.rs` containing only these tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Recorded;
    use crate::testutil::{fixture, jpeg};

    #[test]
    fn add_folder_scans_and_publishes_the_grid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        let watched = f.add_photos();
        let (version, grid) = f.engine.grid();
        assert_eq!(grid.len(), 2);
        assert!(version >= 1);
        let events = f.events.all();
        assert!(events.contains(&Recorded::Library(LibraryChanged { version, len: 2 })));
        assert!(events.iter().any(|e| matches!(e,
            Recorded::Scan(s) if s.watched_id == watched.id && s.done && !s.cancelled && s.added == 2)));
        assert!(events.contains(&Recorded::Folder(FolderStatus { watched_id: watched.id, online: true })));
        assert!(!f.engine.is_scanning(watched.id));
    }

    #[test]
    fn open_loads_the_existing_grid_without_scanning() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let reopened = Engine::open(f.config(), Arc::new(crate::events::Recorder::default())).unwrap();
        assert_eq!(reopened.grid().1.len(), 1);
    }

    #[test]
    fn add_folder_refuses_photons_own_directories() {
        let f = fixture(&[]);
        let cache = f.dir.path().join("cache").join("thumbs");
        let data = f.dir.path().join("data");
        assert!(matches!(f.engine.add_folder(&cache), Err(photon_core::Error::FolderExcluded { .. })));
        assert!(matches!(f.engine.add_folder(&data), Err(photon_core::Error::FolderExcluded { .. })));
    }

    #[test]
    fn remove_folder_clears_its_items_from_the_grid() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        f.engine.remove_folder(watched.id).unwrap();
        assert_eq!(f.engine.grid().1.len(), 0);
        assert!(f.engine.lib.watched_folders().unwrap().is_empty());
        assert!(matches!(f.events.all().last(), Some(Recorded::Library(LibraryChanged { len: 0, .. }))));
    }

    #[test]
    fn startup_adds_pictures_only_to_an_empty_library() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        f.engine.startup(Some(f.photos.clone())).join().unwrap();
        assert_eq!(f.engine.lib.watched_folders().unwrap().len(), 1);
        assert_eq!(f.engine.grid().1.len(), 1);

        let other = f.dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        f.engine.startup(Some(other)).join().unwrap();
        assert_eq!(f.engine.lib.watched_folders().unwrap().len(), 1);
    }

    #[test]
    fn cancelling_an_idle_folder_is_a_no_op() {
        let f = fixture(&[]);
        f.engine.cancel_scan(42);
        f.engine.shutdown();
        assert!(!f.engine.is_scanning(42));
    }
}
```

Add `pub mod engine;` to `lib.rs` if Step 1 hasn't already. Run: `cargo test -p photon-app engine`
Expected: compile errors, because `Engine` is not defined.

- [ ] **Step 3: Implement `engine.rs`**

Prepend to `crates/photon-app/src/engine.rs`:

```rust
//! The running library: photon-core services plus the current grid snapshot and the
//! background scans. Plain Rust, so it can be tested without a webview.

use crate::events::{Events, FolderStatus, LibraryChanged, ScanProgressEvent};
use parking_lot::{Mutex, RwLock};
use photon_core::{
    Result,
    grid::GridIndex,
    library::{Library, WatchedFolder},
    now_ms,
    scanner::{ScanOptions, ScanProgress, scan_watched},
    thumbs::{ThumbCache, ThumbService},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Minimum time between grid rebuilds and between progress events during one scan.
const THROTTLE: Duration = Duration::from_millis(250);

pub struct EngineConfig {
    pub db_path: PathBuf,
    pub cache_dir: PathBuf,
    pub workers: usize,
}

struct RunningScan {
    token: u64,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

pub struct Engine {
    pub lib: Arc<Library>,
    pub thumbs: ThumbService,
    excluded: Vec<PathBuf>,
    grid: RwLock<(u64, Arc<GridIndex>)>,
    events: Arc<dyn Events>,
    scans: Mutex<HashMap<i64, RunningScan>>,
    next_token: AtomicU64,
}

impl Engine {
    pub fn open(config: EngineConfig, events: Arc<dyn Events>) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&config.cache_dir)?;
        let lib = Arc::new(Library::open(&config.db_path)?);
        let cache = Arc::new(ThumbCache::new(config.cache_dir.clone()));
        let thumbs = ThumbService::start(lib.clone(), cache, config.workers);
        let mut excluded = vec![config.cache_dir.clone()];
        if let Some(data_dir) = config.db_path.parent() {
            excluded.push(data_dir.to_path_buf());
        }
        let grid = Arc::new(GridIndex::build(lib.grid_entries()?));
        Ok(Arc::new(Self {
            lib,
            thumbs,
            excluded,
            grid: RwLock::new((0, grid)),
            events,
            scans: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(0),
        }))
    }

    /// photon's own directories, never watched or scanned.
    pub fn excluded(&self) -> &[PathBuf] {
        &self.excluded
    }

    /// The current grid and its version. The version increases with every rebuild.
    pub fn grid(&self) -> (u64, Arc<GridIndex>) {
        let grid = self.grid.read();
        (grid.0, grid.1.clone())
    }

    /// Rebuilds the grid from the database and tells the UI.
    pub fn refresh_grid(&self) -> Result<()> {
        let index = Arc::new(GridIndex::build(self.lib.grid_entries()?));
        let (version, len) = {
            let mut grid = self.grid.write();
            grid.0 += 1;
            grid.1 = index;
            (grid.0, grid.1.len())
        };
        self.events.library_changed(LibraryChanged { version, len });
        Ok(())
    }

    /// Validates and watches `path`, then starts its first scan.
    pub fn add_folder(self: &Arc<Self>, path: &Path) -> Result<WatchedFolder> {
        let watched = self.lib.add_watched_folder(path, &self.excluded)?;
        self.start_scan(watched.clone());
        Ok(watched)
    }

    /// Stops any scan of the folder, forgets it and everything under it, and refreshes the grid.
    pub fn remove_folder(&self, watched_id: i64) -> Result<()> {
        self.cancel_scan(watched_id);
        self.lib.remove_watched_folder(watched_id)?;
        self.refresh_grid()
    }

    /// Starts a background scan unless one is already running for this folder.
    pub fn start_scan(self: &Arc<Self>, watched: WatchedFolder) -> bool {
        let mut scans = self.scans.lock();
        if scans.contains_key(&watched.id) {
            return false;
        }
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let engine = Arc::clone(self);
        let thread_cancel = cancel.clone();
        let id = watched.id;
        let handle = std::thread::Builder::new()
            .name(format!("photon-scan-{id}"))
            .spawn(move || {
                engine.run_scan(&watched, thread_cancel);
                let mut scans = engine.scans.lock();
                if scans.get(&id).is_some_and(|r| r.token == token) {
                    scans.remove(&id);
                }
            })
            .expect("failed to spawn scan thread");
        scans.insert(
            id,
            RunningScan {
                token,
                cancel,
                handle: Some(handle),
            },
        );
        true
    }

    /// Cancels the folder's scan, if any, and waits for it to stop.
    pub fn cancel_scan(&self, watched_id: i64) {
        let running = self.scans.lock().remove(&watched_id);
        if let Some(mut running) = running {
            running.cancel.store(true, Ordering::Relaxed);
            if let Some(handle) = running.handle.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn is_scanning(&self, watched_id: i64) -> bool {
        self.scans.lock().contains_key(&watched_id)
    }

    /// Blocks until no scan is running.
    pub fn wait_for_scans(&self) {
        while !self.scans.lock().is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Background start-up work: watch `pictures` if the library is empty, queue pending
    /// thumbnails, rescan every folder, then collect thumbnail garbage.
    pub fn startup(self: &Arc<Self>, pictures: Option<PathBuf>) -> JoinHandle<()> {
        let engine = Arc::clone(self);
        std::thread::Builder::new()
            .name("photon-startup".into())
            .spawn(move || {
                match engine.lib.watched_folders() {
                    Ok(watched) if watched.is_empty() => {
                        if let Some(pictures) = pictures.filter(|p| p.is_dir()) {
                            if let Err(err) = engine.lib.add_watched_folder(&pictures, &engine.excluded) {
                                tracing::warn!(%err, "could not watch the Pictures folder");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!(%err, "could not list watched folders"),
                }
                if let Err(err) = engine.thumbs.enqueue_pending() {
                    tracing::warn!(%err, "could not queue pending thumbnails");
                }
                for watched in engine.lib.watched_folders().unwrap_or_default() {
                    engine.start_scan(watched);
                }
                engine.wait_for_scans();
                match engine.thumbs.collect_garbage() {
                    Ok(removed) => tracing::info!(removed, "thumbnail garbage collected"),
                    Err(err) => tracing::warn!(%err, "thumbnail garbage collection failed"),
                }
            })
            .expect("failed to spawn startup thread")
    }

    /// Cancels all scans. Thumbnail workers stop when the engine is dropped.
    pub fn shutdown(&self) {
        let ids: Vec<i64> = self.scans.lock().keys().copied().collect();
        for id in ids {
            self.cancel_scan(id);
        }
    }

    fn run_scan(&self, watched: &WatchedFolder, cancel: Arc<AtomicBool>) {
        let options = ScanOptions {
            excluded: self.excluded.clone(),
            cancel,
        };
        let mut last = ScanProgress::default();
        let mut last_refresh = Instant::now();
        let mut refreshed_total = 0;
        let mut last_progress: Option<Instant> = None;
        let result = scan_watched(&self.lib, watched, now_ms(), &options, &mut |p| {
            last = *p;
            let total = p.added + p.changed;
            if total != refreshed_total && last_refresh.elapsed() >= THROTTLE {
                if let Err(err) = self.refresh_grid() {
                    tracing::warn!(%err, "grid refresh failed");
                }
                if let Err(err) = self.thumbs.enqueue_pending() {
                    tracing::warn!(%err, "could not queue pending thumbnails");
                }
                last_refresh = Instant::now();
                refreshed_total = total;
            }
            if last_progress.is_none_or(|t| t.elapsed() >= THROTTLE) {
                self.events
                    .scan_progress(ScanProgressEvent::new(watched.id, p, false, false));
                last_progress = Some(Instant::now());
            }
        });
        let cancelled = match &result {
            Ok(report) => report.cancelled,
            Err(err) => {
                tracing::warn!(watched_id = watched.id, %err, "scan failed");
                false
            }
        };
        if let Err(err) = self.refresh_grid() {
            tracing::warn!(%err, "grid refresh failed");
        }
        if let Err(err) = self.thumbs.enqueue_pending() {
            tracing::warn!(%err, "could not queue pending thumbnails");
        }
        if let Some(folder) = self
            .lib
            .watched_folders()
            .ok()
            .and_then(|all| all.into_iter().find(|w| w.id == watched.id))
        {
            self.events.folder_status(FolderStatus {
                watched_id: folder.id,
                online: folder.online,
            });
        }
        self.events
            .scan_progress(ScanProgressEvent::new(watched.id, &last, true, cancelled));
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `for i in 1 2 3; do cargo test -p photon-app engine || break; done && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS on every repeat.

- [ ] **Step 5: Commit**

```bash
git add crates/photon-app
git commit -m "feat(app): add the engine with background scans and change events"
```

---

### Task 7: Commands

**Files:**
- Create: `crates/photon-app/src/error.rs` and `crates/photon-app/src/commands.rs`
- Modify: `crates/photon-app/src/lib.rs` (add `pub mod commands; pub mod error;`)

**Interfaces:**
- Consumes: `Engine` (Task 6) and `grid::hex_key` (Task 4).
- Produces:
  - `error::AppError { kind: &'static str, message: String }`, which serialises as `{ "kind", "message" }` and implements `From<photon_core::Error>`. It also has `AppError::internal(impl ToString)`.
  - The kinds are `folderNotFound`, `folderOverlap`, `folderExcluded`, `notFound` and `internal`.
  - The DTOs `FolderList`, `GridInfo`, `GridRows` and `ViewerItem` (camelCase), plus the constants `MAX_ROWS = 1000` and `MAX_RADIUS = 10`.
  - `commands::{list_folders, add_folder, remove_folder, rescan_folder, grid_info, grid_rows, grid_offset_of_folder, set_visible, viewer_item, neighbours, item_path, folder_path}`.

- [ ] **Step 1: Write failing tests**

Create `crates/photon-app/src/commands.rs` containing only these tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, jpeg};

    #[test]
    fn folder_listing_and_grid_info() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("sub/b.jpg", &img)]);
        let watched = f.add_photos();
        let list = list_folders(&f.engine).unwrap();
        assert_eq!(list.watched, vec![photon_core::library::WatchedFolder { online: true, ..watched }]);
        assert_eq!(list.folders.len(), 2);
        let info = grid_info(&f.engine);
        assert_eq!((info.len, info.sections.len()), (2, 2));
        let sub = list.folders.iter().find(|x| x.name == "sub").unwrap();
        assert_eq!(grid_offset_of_folder(&f.engine, sub.id), Some(1));
    }

    #[test]
    fn grid_rows_are_capped_and_versioned() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img)]);
        f.add_photos();
        let rows = grid_rows(&f.engine, 0, 5_000);
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.version, grid_info(&f.engine).version);
        assert_eq!(clamp_count(5_000), MAX_ROWS);
        assert!(grid_rows(&f.engine, 10, 5).rows.is_empty());
    }

    #[test]
    fn viewer_item_and_neighbours() {
        let img = jpeg(32, 16);
        let f = fixture(&[("a.jpg", &img), ("b.jpg", &img), ("c.jpg", &img)]);
        f.add_photos();
        let ids = f.ids();
        let item = viewer_item(&f.engine, ids[1]).unwrap();
        assert_eq!((item.file_name.as_str(), item.width, item.height), ("b.jpg", 32, 16));
        assert_eq!(item.thumb_key.len(), 16);
        assert!(matches!(item.thumb_state, "pending" | "ready"));
        assert_eq!(neighbours(&f.engine, ids[1], 50), vec![ids[2], ids[0]]);
        assert_eq!(viewer_item(&f.engine, 9_999).unwrap_err().kind, "notFound");
    }

    #[test]
    fn errors_carry_a_kind_for_the_ui() {
        let f = fixture(&[]);
        f.add_photos();
        let nested = f.photos.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let err = add_folder(&f.engine, nested.to_str().unwrap()).unwrap_err();
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "folderOverlap");
        assert!(json["message"].as_str().unwrap().contains("photos"));
        assert_eq!(rescan_folder(&f.engine, 9_999).unwrap_err().kind, "notFound");
    }

    #[test]
    fn remove_and_paths() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a.jpg", &img)]);
        let watched = f.add_photos();
        let id = f.ids()[0];
        assert!(item_path(&f.engine, id).unwrap().ends_with("a.jpg"));
        let root = list_folders(&f.engine).unwrap().folders[0].id;
        assert_eq!(folder_path(&f.engine, root).unwrap(), std::path::PathBuf::from(&watched.path));
        remove_folder(&f.engine, watched.id).unwrap();
        assert_eq!(grid_info(&f.engine).len, 0);
    }
}
```

Run: `cargo test -p photon-app commands`
Expected: compile errors.

- [ ] **Step 2: Implement `error.rs`**

```rust
use serde::Serialize;

/// Error returned to the UI: a machine-readable `kind` plus a human-readable message.
#[derive(Debug, Serialize)]
pub struct AppError {
    pub kind: &'static str,
    pub message: String,
}

impl AppError {
    pub fn internal(message: impl ToString) -> Self {
        Self {
            kind: "internal",
            message: message.to_string(),
        }
    }
}

impl From<photon_core::Error> for AppError {
    fn from(err: photon_core::Error) -> Self {
        use photon_core::Error::*;
        let kind = match &err {
            FolderNotFound(_) => "folderNotFound",
            FolderOverlap { .. } => "folderOverlap",
            FolderExcluded { .. } => "folderExcluded",
            NotFound(_) => "notFound",
            _ => "internal",
        };
        Self {
            kind,
            message: err.to_string(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}
```

- [ ] **Step 3: Implement `commands.rs`**

Prepend:

```rust
//! Command implementations as plain functions over `Engine`. `ipc.rs` exposes them to
//! the UI as Tauri commands.

use crate::{engine::Engine, error::AppError};
use photon_core::{
    Error,
    grid::{GridEntry, Section, hex_key},
    library::{Folder, WatchedFolder},
    media::ThumbState,
    thumbs::Priority,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_ROWS: usize = 1000;
pub const MAX_RADIUS: usize = 10;

type CmdResult<T> = Result<T, AppError>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderList {
    pub watched: Vec<WatchedFolder>,
    pub folders: Vec<Folder>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridInfo {
    pub version: u64,
    pub len: usize,
    pub sections: Vec<Section>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridRows {
    pub version: u64,
    pub rows: Vec<GridEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerItem {
    pub id: i64,
    pub path: String,
    pub file_name: String,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub taken_at: i64,
    pub thumb_key: String,
    pub thumb_state: &'static str,
    pub thumb_error: Option<String>,
}

pub fn clamp_count(count: usize) -> usize {
    count.min(MAX_ROWS)
}

pub fn list_folders(engine: &Engine) -> CmdResult<FolderList> {
    Ok(FolderList {
        watched: engine.lib.watched_folders()?,
        folders: engine.lib.folders()?,
    })
}

pub fn add_folder(engine: &Arc<Engine>, path: &str) -> CmdResult<WatchedFolder> {
    Ok(engine.add_folder(Path::new(path))?)
}

pub fn remove_folder(engine: &Engine, watched_id: i64) -> CmdResult<()> {
    Ok(engine.remove_folder(watched_id)?)
}

pub fn rescan_folder(engine: &Arc<Engine>, watched_id: i64) -> CmdResult<()> {
    let watched = engine
        .lib
        .watched_folders()?
        .into_iter()
        .find(|w| w.id == watched_id)
        .ok_or(Error::NotFound(watched_id))?;
    engine.start_scan(watched);
    Ok(())
}

pub fn grid_info(engine: &Engine) -> GridInfo {
    let (version, grid) = engine.grid();
    GridInfo {
        version,
        len: grid.len(),
        sections: grid.sections().to_vec(),
    }
}

pub fn grid_rows(engine: &Engine, offset: usize, count: usize) -> GridRows {
    let (version, grid) = engine.grid();
    GridRows {
        version,
        rows: grid.rows(offset, clamp_count(count)).to_vec(),
    }
}

pub fn grid_offset_of_folder(engine: &Engine, folder_id: i64) -> Option<usize> {
    engine.grid().1.offset_of_folder(folder_id)
}

pub fn set_visible(engine: &Engine, ids: &[i64]) {
    engine.thumbs.set_visible(ids);
}

pub fn viewer_item(engine: &Engine, id: i64) -> CmdResult<ViewerItem> {
    let item = engine.lib.item(id)?.ok_or(Error::NotFound(id))?;
    let file_name = Path::new(&item.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(ViewerItem {
        id: item.id,
        thumb_key: hex_key(item.fingerprint()),
        thumb_state: match item.thumb_state {
            ThumbState::Pending => "pending",
            ThumbState::Ready => "ready",
            ThumbState::Failed => "failed",
        },
        file_name,
        width: item.width,
        height: item.height,
        orientation: item.orientation,
        taken_at: item.taken_at,
        thumb_error: item.thumb_error,
        path: item.path,
    })
}

/// Items around `id`, nearest first, queued at neighbour priority so the viewer's
/// next and previous previews are ready early.
pub fn neighbours(engine: &Engine, id: i64, radius: usize) -> Vec<i64> {
    let ids = engine.grid().1.neighbours(id, radius.min(MAX_RADIUS));
    engine.thumbs.prioritize(&ids, Priority::Neighbour);
    ids
}

pub fn item_path(engine: &Engine, id: i64) -> CmdResult<PathBuf> {
    let item = engine.lib.item(id)?.ok_or(Error::NotFound(id))?;
    Ok(PathBuf::from(item.path))
}

pub fn folder_path(engine: &Engine, folder_id: i64) -> CmdResult<PathBuf> {
    let folder = engine
        .lib
        .folders()?
        .into_iter()
        .find(|f| f.id == folder_id)
        .ok_or(Error::NotFound(folder_id))?;
    Ok(PathBuf::from(folder.path))
}
```

In `folder_listing_and_grid_info`, the `WatchedFolder` literal needs `Clone`/`PartialEq`, which it already derives. If the photo in `sub/` lands at a different offset because of sort order, check `grid_info(...).sections` and fix the test's expected offset. Don't change the ordering.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-app && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/photon-app
git commit -m "feat(app): add command functions and the UI error type"
```

---

### Task 8: The `photon://` protocol handler

**Files:**
- Create: `crates/photon-app/src/protocol.rs`
- Modify: `crates/photon-app/src/lib.rs` (add `pub mod protocol;`)

**Interfaces:**
- Consumes: `Engine`, and `ThumbService::request` (Task 3).
- Produces:
  - `protocol::THUMB_TIMEOUT: Duration` (30 s).
  - `protocol::handle(engine: &Engine, path: &str) -> tauri::http::Response<Vec<u8>>`. `path` is the URI path, for example `/thumb/12/grid/00ab…`.

- [ ] **Step 1: Write failing tests**

Create `protocol.rs` containing only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, jpeg};

    fn header<'a>(r: &'a Response<Vec<u8>>, name: &str) -> &'a str {
        r.headers().get(name).unwrap().to_str().unwrap()
    }

    #[test]
    fn serves_thumbnails_with_immutable_caching() {
        let img = jpeg(64, 32);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let id = f.ids()[0];
        let r = handle(&f.engine, &format!("/thumb/{id}/grid/0123456789abcdef"));
        assert_eq!(r.status(), 200);
        assert_eq!(header(&r, "content-type"), "image/webp");
        assert!(header(&r, "cache-control").contains("immutable"));
        assert!(!r.body().is_empty());
        assert_eq!(handle(&f.engine, &format!("/thumb/{id}/preview/x")).status(), 200);
    }

    #[test]
    fn serves_originals_with_their_mime_type() {
        let img = jpeg(64, 32);
        let f = fixture(&[("a.jpg", &img)]);
        f.add_photos();
        let r = handle(&f.engine, &format!("/image/{}", f.ids()[0]));
        assert_eq!(r.status(), 200);
        assert_eq!(header(&r, "content-type"), "image/jpeg");
        assert_eq!(r.body(), &img);
    }

    #[test]
    fn maps_errors_to_status_codes() {
        let f = fixture(&[("bad.jpg", b"garbage")]);
        f.add_photos();
        let bad = f.ids()[0];
        assert_eq!(handle(&f.engine, &format!("/thumb/{bad}/grid/k")).status(), 422);
        assert_eq!(handle(&f.engine, "/thumb/9999/grid/k").status(), 404);
        assert_eq!(handle(&f.engine, "/image/9999").status(), 404);
        assert_eq!(handle(&f.engine, "/thumb/abc/grid/k").status(), 400);
        assert_eq!(handle(&f.engine, "/thumb/1/huge/k").status(), 400);
        assert_eq!(handle(&f.engine, "/nope").status(), 404);
    }
}
```

Run: `cargo test -p photon-app protocol`
Expected: compile errors.

- [ ] **Step 2: Implement**

Prepend:

```rust
//! The `photon://` URI scheme: thumbnails and original images for the webview.
//!
//! - `/thumb/<id>/<grid|preview>/<thumbKey>`: WebP, built on demand, cached forever
//!   (`thumbKey` changes when the file does).
//! - `/image/<id>`: the original file.

use crate::engine::Engine;
use photon_core::{Error, thumbs::ThumbSize};
use std::{path::Path, time::Duration};
use tauri::http::{Response, StatusCode, header};

pub const THUMB_TIMEOUT: Duration = Duration::from_secs(30);

pub fn handle(engine: &Engine, path: &str) -> Response<Vec<u8>> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match parts.as_slice() {
        ["thumb", id, size, ..] => thumb(engine, id, size),
        ["image", id] => image(engine, id),
        _ => text(StatusCode::NOT_FOUND, "not found"),
    }
}

fn thumb(engine: &Engine, id: &str, size: &str) -> Response<Vec<u8>> {
    let Ok(id) = id.parse::<i64>() else {
        return text(StatusCode::BAD_REQUEST, "bad id");
    };
    let size = match size {
        "grid" => ThumbSize::Grid,
        "preview" => ThumbSize::Preview,
        _ => return text(StatusCode::BAD_REQUEST, "bad size"),
    };
    match engine.thumbs.request(id, size, THUMB_TIMEOUT) {
        Ok(file) => match std::fs::read(&file) {
            Ok(bytes) => ok(bytes, "image/webp", "public, max-age=31536000, immutable"),
            Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        },
        Err(Error::NotFound(_)) => text(StatusCode::NOT_FOUND, "not found"),
        Err(Error::ThumbFailed(message)) => text(StatusCode::UNPROCESSABLE_ENTITY, &message),
        Err(Error::ThumbTimeout(_) | Error::ThumbUnavailable(_)) => {
            text(StatusCode::SERVICE_UNAVAILABLE, "thumbnail not ready")
        }
        Err(err) => text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

fn image(engine: &Engine, id: &str) -> Response<Vec<u8>> {
    let Ok(id) = id.parse::<i64>() else {
        return text(StatusCode::BAD_REQUEST, "bad id");
    };
    let item = match engine.lib.item(id) {
        Ok(Some(item)) if item.missing_since.is_none() => item,
        Ok(_) => return text(StatusCode::NOT_FOUND, "not found"),
        Err(err) => return text(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    match std::fs::read(&item.path) {
        Ok(bytes) => ok(bytes, mime_for(Path::new(&item.path)), "no-cache"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => text(StatusCode::NOT_FOUND, "not found"),
        Err(err) => text(StatusCode::SERVICE_UNAVAILABLE, &err.to_string()),
    }
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg" | "jpe") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn ok(body: Vec<u8>, content_type: &str, cache: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .expect("static response parts are valid")
}

fn text(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(message.as_bytes().to_vec())
        .expect("static response parts are valid")
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p photon-app && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/photon-app
git commit -m "feat(app): serve thumbnails and originals over the photon:// protocol"
```

---

### Task 9: Tauri wiring

**Files:**
- Create: `crates/photon-app/src/ipc.rs` and `crates/photon-app/src/app.rs`
- Modify: `crates/photon-app/src/lib.rs`

**Interfaces:**
- Consumes: everything from Tasks 6–8.
- Produces:
  - Tauri commands named `list_folders`, `add_folder(path)`, `remove_folder(watchedId)`, `rescan_folder(watchedId)`, `grid_info`, `grid_rows(offset, count)`, `grid_offset_of_folder(folderId)`, `set_visible(ids)`, `viewer_item(id)`, `neighbours(id, radius)`, `reveal_in_file_manager(id)` and `reveal_folder(folderId)`.
  - The events `library-changed`, `scan-progress` and `folder-status`.
  - The `photon` URI scheme.

- [ ] **Step 1: Write `ipc.rs`**

```rust
//! Tauri command wrappers. Each runs on the async runtime (`async` attribute), so
//! blocking work such as waiting for a cancelled scan never stalls the UI thread.

use crate::{commands, engine::Engine, error::AppError};
use photon_core::library::WatchedFolder;
use std::sync::Arc;
use tauri::State;

type Eng<'a> = State<'a, Arc<Engine>>;

#[tauri::command(async)]
pub fn list_folders(engine: Eng<'_>) -> Result<commands::FolderList, AppError> {
    commands::list_folders(&engine)
}

#[tauri::command(async)]
pub fn add_folder(engine: Eng<'_>, path: String) -> Result<WatchedFolder, AppError> {
    commands::add_folder(engine.inner(), &path)
}

#[tauri::command(async)]
pub fn remove_folder(engine: Eng<'_>, watched_id: i64) -> Result<(), AppError> {
    commands::remove_folder(&engine, watched_id)
}

#[tauri::command(async)]
pub fn rescan_folder(engine: Eng<'_>, watched_id: i64) -> Result<(), AppError> {
    commands::rescan_folder(engine.inner(), watched_id)
}

#[tauri::command(async)]
pub fn grid_info(engine: Eng<'_>) -> commands::GridInfo {
    commands::grid_info(&engine)
}

#[tauri::command(async)]
pub fn grid_rows(engine: Eng<'_>, offset: usize, count: usize) -> commands::GridRows {
    commands::grid_rows(&engine, offset, count)
}

#[tauri::command(async)]
pub fn grid_offset_of_folder(engine: Eng<'_>, folder_id: i64) -> Option<usize> {
    commands::grid_offset_of_folder(&engine, folder_id)
}

#[tauri::command(async)]
pub fn set_visible(engine: Eng<'_>, ids: Vec<i64>) {
    commands::set_visible(&engine, &ids)
}

#[tauri::command(async)]
pub fn viewer_item(engine: Eng<'_>, id: i64) -> Result<commands::ViewerItem, AppError> {
    commands::viewer_item(&engine, id)
}

#[tauri::command(async)]
pub fn neighbours(engine: Eng<'_>, id: i64, radius: usize) -> Vec<i64> {
    commands::neighbours(&engine, id, radius)
}

#[tauri::command(async)]
pub fn reveal_in_file_manager(engine: Eng<'_>, id: i64) -> Result<(), AppError> {
    let path = commands::item_path(&engine, id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
}

#[tauri::command(async)]
pub fn reveal_folder(engine: Eng<'_>, folder_id: i64) -> Result<(), AppError> {
    let path = commands::folder_path(&engine, folder_id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(AppError::internal)
}
```

If the `async` attribute or `reveal_item_in_dir`'s signature differ in the installed crate versions, adapt minimally (for example, make the commands `async fn`) and record the change.

- [ ] **Step 2: Write `app.rs`**

```rust
//! Builds and runs the Tauri app: plugins, the photon:// protocol, commands, and the
//! engine's lifecycle.

use crate::{
    engine::{Engine, EngineConfig},
    events::{Events, FolderStatus, LibraryChanged, ScanProgressEvent},
    ipc, protocol,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

struct TauriEvents(AppHandle);

impl TauriEvents {
    fn emit<S: Serialize + Clone>(&self, name: &str, payload: S) {
        if let Err(err) = self.0.emit(name, payload) {
            tracing::warn!(%err, name, "failed to emit event");
        }
    }
}

impl Events for TauriEvents {
    fn library_changed(&self, e: LibraryChanged) {
        self.emit("library-changed", e);
    }
    fn scan_progress(&self, e: ScanProgressEvent) {
        self.emit("scan-progress", e);
    }
    fn folder_status(&self, e: FolderStatus) {
        self.emit("folder-status", e);
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("photon", |ctx, request, responder| {
            let engine = ctx.app_handle().try_state::<Arc<Engine>>().map(|s| s.inner().clone());
            let path = request.uri().path().to_string();
            tauri::async_runtime::spawn_blocking(move || {
                let response = match engine {
                    Some(engine) => protocol::handle(&engine, &path),
                    None => tauri::http::Response::builder()
                        .status(503)
                        .body(b"starting".to_vec())
                        .expect("static response"),
                };
                responder.respond(response);
            });
        })
        .setup(|app| {
            let paths = app.path();
            let config = EngineConfig {
                db_path: paths.app_data_dir()?.join("library.db"),
                cache_dir: paths.app_cache_dir()?.join("thumbs"),
                workers: photon_core::thumbs::default_workers(),
            };
            let events = Arc::new(TauriEvents(app.handle().clone()));
            match Engine::open(config, events) {
                Ok(engine) => {
                    let pictures = paths.picture_dir().ok();
                    app.manage(engine.clone());
                    engine.startup(pictures);
                }
                Err(err) => {
                    tracing::error!(%err, "could not open the photon library");
                    app.dialog()
                        .message(format!(
                            "photon could not open its library:\n\n{err}\n\nNothing was changed."
                        ))
                        .kind(MessageDialogKind::Error)
                        .title("photon")
                        .show(|_| std::process::exit(1));
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::list_folders,
            ipc::add_folder,
            ipc::remove_folder,
            ipc::rescan_folder,
            ipc::grid_info,
            ipc::grid_rows,
            ipc::grid_offset_of_folder,
            ipc::set_visible,
            ipc::viewer_item,
            ipc::neighbours,
            ipc::reveal_in_file_manager,
            ipc::reveal_folder,
        ])
        .build(tauri::generate_context!())
        .expect("error while building photon")
        .run(|app, event| {
            if let RunEvent::Exit = event
                && let Some(engine) = app.try_state::<Arc<Engine>>()
            {
                engine.shutdown();
            }
        });
}
```

- [ ] **Step 3: Update `lib.rs`**

```rust
//! photon-app: the Tauri shell around photon-core.

mod app;
pub mod commands;
pub mod engine;
pub mod error;
pub mod events;
mod ipc;
pub mod protocol;

#[cfg(test)]
mod testutil;

pub use app::run;
```

- [ ] **Step 4: Build and lint**

Run: `cargo build -p photon-app && cargo test -p photon-app && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: success. Adapt to any Tauri API differences (closure signatures, `try_state`, `picture_dir`, `show`) using the 2.11 docs, and record each adaptation. Do not launch the app.

- [ ] **Step 5: Commit**

```bash
git add crates/photon-app
git commit -m "feat(app): wire commands, events and the photon:// protocol into Tauri"
```

---

### Task 10: Grid layout maths

**Files:**
- Create: `ui/src/lib/layout.ts` and `ui/src/lib/layout.test.ts`

**Interfaces:**
- Produces:
  - `TILE = 160`, `GAP = 8`, `HEADER = 32`, `TILE_ROW = TILE + GAP`.
  - `interface SectionLike { folderId: number; offset: number; count: number }`.
  - `interface Row { kind: 'header' | 'tiles'; section: number; first: number; count: number; top: number; height: number }`. Header rows have `first` equal to the section offset and `count: 0`.
  - `columnsFor(width) -> number`, `buildRows(sections, columns) -> Row[]`, `totalHeight(rows)`, `rowIndexAt(rows, y)`, `visibleRange(rows, scrollTop, viewport, overscan) -> [start, end)`, `rowOfItem(rows, offset) -> number` (-1 if absent) and `itemSpan(rows) -> [start, end) | null`.

- [ ] **Step 1: Write failing tests**

`ui/src/lib/layout.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { buildRows, columnsFor, itemSpan, rowIndexAt, rowOfItem, totalHeight, visibleRange } from './layout';

const sections = [
  { folderId: 1, offset: 0, count: 5 },
  { folderId: 2, offset: 5, count: 3 },
];

describe('layout', () => {
  it('fits columns to the width', () => {
    expect(columnsFor(800)).toBe(4);
    expect(columnsFor(100)).toBe(1);
    expect(columnsFor(0)).toBe(1);
  });

  it('builds header and tile rows per section', () => {
    const rows = buildRows(sections, 2);
    expect(rows.map((r) => [r.kind, r.first, r.count, r.top])).toEqual([
      ['header', 0, 0, 0],
      ['tiles', 0, 2, 32],
      ['tiles', 2, 2, 200],
      ['tiles', 4, 1, 368],
      ['header', 5, 0, 536],
      ['tiles', 5, 2, 568],
      ['tiles', 7, 1, 736],
    ]);
    expect(totalHeight(rows)).toBe(904);
    expect(totalHeight([])).toBe(0);
  });

  it('hit-tests rows by y', () => {
    const rows = buildRows(sections, 2);
    expect(rowIndexAt(rows, -5)).toBe(0);
    expect(rowIndexAt(rows, 31)).toBe(0);
    expect(rowIndexAt(rows, 32)).toBe(1);
    expect(rowIndexAt(rows, 500)).toBe(3);
    expect(rowIndexAt(rows, 10_000)).toBe(6);
  });

  it('finds visible ranges and their items', () => {
    const rows = buildRows(sections, 2);
    expect(visibleRange(rows, 200, 100, 0)).toEqual([2, 3]);
    expect(visibleRange(rows, 0, 10_000, 0)).toEqual([0, 7]);
    expect(visibleRange([], 0, 100, 0)).toEqual([0, 0]);
    expect(itemSpan(rows.slice(0, 3))).toEqual([0, 4]);
    expect(itemSpan(rows.slice(4, 5))).toBeNull();
  });

  it('locates the row holding an item', () => {
    const rows = buildRows(sections, 2);
    expect(rowOfItem(rows, 0)).toBe(1);
    expect(rowOfItem(rows, 4)).toBe(3);
    expect(rowOfItem(rows, 5)).toBe(5);
    expect(rowOfItem(rows, 7)).toBe(6);
    expect(rowOfItem(rows, 8)).toBe(-1);
  });
});
```

Run: `npm test`. Expected: FAIL (the module is missing).

- [ ] **Step 2: Implement `layout.ts`**

```ts
/** Grid geometry. Square tiles in fixed-height rows, with one header row per folder
 *  section. Everything here is pure, so 100k items lay out in microseconds. */

export const TILE = 160;
export const GAP = 8;
export const HEADER = 32;
export const TILE_ROW = TILE + GAP;

export interface SectionLike {
  folderId: number;
  offset: number;
  count: number;
}

export interface Row {
  kind: 'header' | 'tiles';
  /** Index into the sections array. */
  section: number;
  /** Grid offset of the first item (the section offset for headers). */
  first: number;
  /** Items in this row (0 for headers). */
  count: number;
  top: number;
  height: number;
}

export function columnsFor(width: number): number {
  return Math.max(1, Math.floor((width + GAP) / TILE_ROW));
}

export function buildRows(sections: SectionLike[], columns: number): Row[] {
  const rows: Row[] = [];
  let top = 0;
  sections.forEach((s, section) => {
    rows.push({ kind: 'header', section, first: s.offset, count: 0, top, height: HEADER });
    top += HEADER;
    const end = s.offset + s.count;
    for (let first = s.offset; first < end; first += columns) {
      rows.push({ kind: 'tiles', section, first, count: Math.min(columns, end - first), top, height: TILE_ROW });
      top += TILE_ROW;
    }
  });
  return rows;
}

export function totalHeight(rows: Row[]): number {
  const last = rows[rows.length - 1];
  return last ? last.top + last.height : 0;
}

/** Index of the last row whose top is at or above `y`. */
export function rowIndexAt(rows: Row[], y: number): number {
  let lo = 0;
  let hi = rows.length - 1;
  if (hi < 0 || y <= 0) return 0;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (rows[mid].top <= y) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** Rows intersecting `[scrollTop - overscan, scrollTop + viewport + overscan)`, as `[start, end)`. */
export function visibleRange(rows: Row[], scrollTop: number, viewport: number, overscan: number): [number, number] {
  if (rows.length === 0) return [0, 0];
  const start = rowIndexAt(rows, scrollTop - overscan);
  const end = rowIndexAt(rows, scrollTop + viewport + overscan) + 1;
  return [start, Math.min(end, rows.length)];
}

/** Index of the tile row containing grid offset `offset`, or -1. */
export function rowOfItem(rows: Row[], offset: number): number {
  let lo = 0;
  let hi = rows.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid].first <= offset) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  if (found < 0) return -1;
  const row = rows[found];
  return row.kind === 'tiles' && offset < row.first + row.count ? found : -1;
}

/** Grid offsets covered by the tile rows in `rows`, as `[start, end)`, or null if none. */
export function itemSpan(rows: Row[]): [number, number] | null {
  let start = Infinity;
  let end = -Infinity;
  for (const r of rows) {
    if (r.kind !== 'tiles') continue;
    start = Math.min(start, r.first);
    end = Math.max(end, r.first + r.count);
  }
  return start === Infinity ? null : [start, end];
}
```

Run: `npm test && npm run check`. Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add ui/src/lib/layout.ts ui/src/lib/layout.test.ts
git commit -m "feat(ui): add grid row layout and hit testing"
```

---

### Task 11: Page cache and keyboard navigation

**Files:**
- Create: `ui/src/lib/pages.ts`, `ui/src/lib/pages.test.ts`, `ui/src/lib/nav.ts` and `ui/src/lib/nav.test.ts`

**Interfaces:**
- Consumes: `SectionLike` from `layout.ts`.
- Produces:
  - `PAGE_SIZE = 200`.
  - `class PageCache<T>` with:
    - `constructor(load: (offset, count) => Promise<{ version: number; rows: T[] }>, onStale?: (version) => void)`;
    - `version`;
    - `reset(version)`;
    - `get(offset): T | undefined`;
    - `ensure(start, end): Promise<boolean>`.
  - `type NavKey = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' | 'Home' | 'End'` and `move(offset, key, sections, columns): number`.

- [ ] **Step 1: Write failing tests**

`ui/src/lib/pages.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest';
import { PAGE_SIZE, PageCache } from './pages';

function loader(version: number) {
  return vi.fn(async (offset: number, count: number) => ({
    version,
    rows: Array.from({ length: count }, (_, i) => offset + i),
  }));
}

describe('PageCache', () => {
  it('loads the pages covering a range once', async () => {
    const load = loader(1);
    const cache = new PageCache<number>(load);
    cache.reset(1);
    expect(await cache.ensure(150, 250)).toBe(true);
    expect(load).toHaveBeenCalledTimes(2);
    expect(cache.get(0)).toBe(0);
    expect(cache.get(PAGE_SIZE + 5)).toBe(PAGE_SIZE + 5);
    expect(await cache.ensure(0, 10)).toBe(false);
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('does not request a page twice while it is loading', async () => {
    const load = loader(1);
    const cache = new PageCache<number>(load);
    cache.reset(1);
    await Promise.all([cache.ensure(0, 10), cache.ensure(5, 20)]);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('drops pages from other versions', async () => {
    const onStale = vi.fn();
    const cache = new PageCache<number>(loader(2), onStale);
    cache.reset(1);
    expect(await cache.ensure(0, 10)).toBe(false);
    expect(cache.get(0)).toBeUndefined();
    expect(onStale).toHaveBeenCalledWith(2);
  });

  it('clears on reset and can retry failed loads', async () => {
    let fail = true;
    const cache = new PageCache<number>(async (offset, count) => {
      if (fail) throw new Error('boom');
      return { version: 1, rows: Array.from({ length: count }, (_, i) => offset + i) };
    });
    cache.reset(1);
    expect(await cache.ensure(0, 1)).toBe(false);
    fail = false;
    expect(await cache.ensure(0, 1)).toBe(true);
    cache.reset(2);
    expect(cache.get(0)).toBeUndefined();
  });
});
```

`ui/src/lib/nav.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { move } from './nav';

const sections = [
  { folderId: 1, offset: 0, count: 5 },
  { folderId: 2, offset: 5, count: 3 },
];

describe('move', () => {
  it('steps left and right across the whole grid', () => {
    expect(move(0, 'ArrowLeft', sections, 2)).toBe(0);
    expect(move(4, 'ArrowRight', sections, 2)).toBe(5);
    expect(move(7, 'ArrowRight', sections, 2)).toBe(7);
    expect(move(3, 'Home', sections, 2)).toBe(0);
    expect(move(3, 'End', sections, 2)).toBe(7);
  });

  it('moves down by rows, into the next section at the same column', () => {
    expect(move(0, 'ArrowDown', sections, 2)).toBe(2);
    expect(move(3, 'ArrowDown', sections, 2)).toBe(4);
    expect(move(4, 'ArrowDown', sections, 2)).toBe(5);
    expect(move(6, 'ArrowDown', sections, 2)).toBe(7);
    expect(move(7, 'ArrowDown', sections, 2)).toBe(7);
  });

  it('moves up by rows, into the previous section’s last row', () => {
    expect(move(3, 'ArrowUp', sections, 2)).toBe(1);
    expect(move(5, 'ArrowUp', sections, 2)).toBe(4);
    expect(move(6, 'ArrowUp', sections, 2)).toBe(4);
    expect(move(1, 'ArrowUp', sections, 2)).toBe(1);
  });

  it('handles an empty grid', () => {
    expect(move(0, 'ArrowDown', [], 4)).toBe(0);
  });
});
```

Run: `npm test`. Expected: FAIL.

- [ ] **Step 2: Implement `pages.ts`**

```ts
/** Fixed-size pages of grid rows, fetched on demand and dropped when the grid version changes. */

export const PAGE_SIZE = 200;

export interface Page<T> {
  version: number;
  rows: T[];
}

type Loader<T> = (offset: number, count: number) => Promise<Page<T>>;

export class PageCache<T> {
  version = -1;
  private readonly load: Loader<T>;
  private readonly onStale: (version: number) => void;
  private pages = new Map<number, T[]>();
  private loading = new Set<number>();

  constructor(load: Loader<T>, onStale: (version: number) => void = () => {}) {
    this.load = load;
    this.onStale = onStale;
  }

  reset(version: number): void {
    if (version === this.version) return;
    this.version = version;
    this.pages.clear();
    this.loading.clear();
  }

  get(offset: number): T | undefined {
    return this.pages.get(Math.floor(offset / PAGE_SIZE))?.[offset % PAGE_SIZE];
  }

  /** Loads the missing pages covering `[start, end)`. Resolves true if anything new was stored. */
  async ensure(start: number, end: number): Promise<boolean> {
    const first = Math.floor(start / PAGE_SIZE);
    const last = Math.floor(Math.max(start, end - 1) / PAGE_SIZE);
    const wanted: number[] = [];
    for (let p = first; p <= last; p++) {
      if (!this.pages.has(p) && !this.loading.has(p)) wanted.push(p);
    }
    if (wanted.length === 0) return false;
    const version = this.version;
    for (const p of wanted) this.loading.add(p);
    const results = await Promise.all(
      wanted.map((p) =>
        this.load(p * PAGE_SIZE, PAGE_SIZE).then(
          (page) => [p, page] as const,
          () => [p, null] as const,
        ),
      ),
    );
    if (version !== this.version) return false;
    let stored = false;
    for (const [p, page] of results) {
      this.loading.delete(p);
      if (!page) continue;
      if (page.version !== version) {
        this.onStale(page.version);
        continue;
      }
      this.pages.set(p, page.rows);
      stored = true;
    }
    return stored;
  }
}
```

- [ ] **Step 3: Implement `nav.ts`**

```ts
import type { SectionLike } from './layout';

export type NavKey = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' | 'Home' | 'End';

function sectionIndexOf(sections: SectionLike[], offset: number): number {
  let lo = 0;
  let hi = sections.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (sections[mid].offset <= offset) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** The grid offset selected after pressing `key`. Vertical moves follow the on-screen
 *  rows, which restart at every section. */
export function move(offset: number, key: NavKey, sections: SectionLike[], columns: number): number {
  const lastSection = sections[sections.length - 1];
  const len = lastSection ? lastSection.offset + lastSection.count : 0;
  if (len === 0) return 0;
  switch (key) {
    case 'ArrowLeft':
      return Math.max(0, offset - 1);
    case 'ArrowRight':
      return Math.min(len - 1, offset + 1);
    case 'Home':
      return 0;
    case 'End':
      return len - 1;
  }
  const si = sectionIndexOf(sections, offset);
  const s = sections[si];
  const local = offset - s.offset;
  const column = local % columns;
  if (key === 'ArrowDown') {
    if (local + columns < s.count) return offset + columns;
    const lastRowStart = Math.floor((s.count - 1) / columns) * columns;
    if (local < lastRowStart) return s.offset + s.count - 1;
    const next = sections[si + 1];
    return next ? next.offset + Math.min(column, next.count - 1) : offset;
  }
  if (local - columns >= 0) return offset - columns;
  const prev = sections[si - 1];
  if (!prev) return offset;
  const prevLastRow = Math.floor((prev.count - 1) / columns) * columns;
  return prev.offset + Math.min(prevLastRow + column, prev.count - 1);
}
```

Run: `npm test && npm run check`. Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add ui/src/lib
git commit -m "feat(ui): add the grid page cache and keyboard navigation"
```

---

### Task 12: API module and library store

**Files:**
- Create: `ui/src/lib/api.ts` and `ui/src/lib/library.svelte.ts`

**Interfaces:**
- Consumes:
  - the Tauri commands and events from Task 9 (argument names in camelCase);
  - `mediaUrl` (Task 5);
  - `PageCache` (Task 11).
- Produces:
  - **`api.ts`:**
    - TS types `WatchedFolder`, `Folder`, `FolderList`, `Section`, `GridEntry`, `GridInfo`, `GridRows`, `ViewerItem`, `ScanProgressEvent`, `FolderStatus`, `LibraryChanged` and `AppError`;
    - the `api` object, which has one method per command;
    - `events.{onLibraryChanged, onScanProgress, onFolderStatus}(cb) => Promise<UnlistenFn>`;
    - a re-export of `mediaUrl`.
  - **`library.svelte.ts`:** `library`, a `LibraryStore` singleton, with:
    - the reactive fields `info`, `folders`, `scans`, `selected`, `pageTick` and `errors`;
    - the methods `init()`, `dispose()`, `refresh()`, `refreshFolders()`, `ensure(start, end)`, `entry(offset)`, `folderOf(folderId)`, `isOnline(folderId)`, `isScanning(watchedId)`, `reportError(e)` and `dismissError(id)`.

This task is all glue. It has no unit tests beyond `npm run check`, because the Tauri runtime is not available under Vitest.

- [ ] **Step 1: Write `api.ts`**

```ts
/** The only module that talks to the Rust side. Types mirror the serde structs in
 *  crates/photon-app/src/commands.rs and events.rs (camelCase). */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export { mediaUrl } from './url';

export interface WatchedFolder { id: number; path: string; online: boolean }
export interface Folder { id: number; watchedId: number; parentId: number | null; path: string; name: string }
export interface FolderList { watched: WatchedFolder[]; folders: Folder[] }
export interface Section { folderId: number; offset: number; count: number }
export interface GridEntry { id: number; folderId: number; takenAt: number; aspect: number; kind: 'image'; thumbKey: string }
export interface GridInfo { version: number; len: number; sections: Section[] }
export interface GridRows { version: number; rows: GridEntry[] }
export interface ViewerItem {
  id: number;
  path: string;
  fileName: string;
  width: number;
  height: number;
  orientation: number;
  takenAt: number;
  thumbKey: string;
  thumbState: 'pending' | 'ready' | 'failed';
  thumbError: string | null;
}
export interface ScanProgressEvent {
  watchedId: number;
  filesSeen: number;
  added: number;
  changed: number;
  done: boolean;
  cancelled: boolean;
}
export interface FolderStatus { watchedId: number; online: boolean }
export interface LibraryChanged { version: number; len: number }
export interface AppError { kind: string; message: string }

export const api = {
  listFolders: () => invoke<FolderList>('list_folders'),
  addFolder: (path: string) => invoke<WatchedFolder>('add_folder', { path }),
  removeFolder: (watchedId: number) => invoke<void>('remove_folder', { watchedId }),
  rescanFolder: (watchedId: number) => invoke<void>('rescan_folder', { watchedId }),
  gridInfo: () => invoke<GridInfo>('grid_info'),
  gridRows: (offset: number, count: number) => invoke<GridRows>('grid_rows', { offset, count }),
  gridOffsetOfFolder: (folderId: number) => invoke<number | null>('grid_offset_of_folder', { folderId }),
  setVisible: (ids: number[]) => invoke<void>('set_visible', { ids }),
  viewerItem: (id: number) => invoke<ViewerItem>('viewer_item', { id }),
  neighbours: (id: number, radius: number) => invoke<number[]>('neighbours', { id, radius }),
  revealInFileManager: (id: number) => invoke<void>('reveal_in_file_manager', { id }),
  revealFolder: (folderId: number) => invoke<void>('reveal_folder', { folderId }),
};

export const events = {
  onLibraryChanged: (cb: (e: LibraryChanged) => void): Promise<UnlistenFn> =>
    listen<LibraryChanged>('library-changed', (e) => cb(e.payload)),
  onScanProgress: (cb: (e: ScanProgressEvent) => void): Promise<UnlistenFn> =>
    listen<ScanProgressEvent>('scan-progress', (e) => cb(e.payload)),
  onFolderStatus: (cb: (e: FolderStatus) => void): Promise<UnlistenFn> =>
    listen<FolderStatus>('folder-status', (e) => cb(e.payload)),
};

export function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
  return String(e);
}
```

- [ ] **Step 2: Write `library.svelte.ts`**

```ts
import { api, errorMessage, events, type Folder, type FolderList, type GridEntry, type GridInfo, type ScanProgressEvent } from './api';
import { PageCache } from './pages';
import type { UnlistenFn } from '@tauri-apps/api/event';

export interface Toast { id: number; message: string }

/** App-wide reactive state: the grid snapshot, the folder tree, scan status and selection. */
export class LibraryStore {
  info = $state<GridInfo>({ version: -1, len: 0, sections: [] });
  folders = $state<FolderList>({ watched: [], folders: [] });
  scans = $state<Record<number, ScanProgressEvent>>({});
  /** Selected grid offset. */
  selected = $state<number | null>(null);
  /** Bumped whenever pages arrive, so `entry()` readers re-run. */
  pageTick = $state(0);
  errors = $state<Toast[]>([]);

  private folderById = $derived(new Map(this.folders.folders.map((f) => [f.id, f])));
  private onlineByWatched = $derived(new Map(this.folders.watched.map((w) => [w.id, w.online])));
  private pages = new PageCache<GridEntry>((o, c) => api.gridRows(o, c), () => void this.refresh());
  private unlisten: UnlistenFn[] = [];
  private nextToast = 0;

  async init(): Promise<void> {
    this.unlisten = await Promise.all([
      events.onLibraryChanged(() => void this.refresh()),
      events.onFolderStatus(() => void this.refreshFolders()),
      events.onScanProgress((e) => {
        this.scans[e.watchedId] = e;
        if (e.done) void this.refreshFolders();
      }),
    ]);
    await Promise.all([this.refresh(), this.refreshFolders()]);
  }

  dispose(): void {
    for (const u of this.unlisten) u();
    this.unlisten = [];
  }

  async refresh(): Promise<void> {
    const info = await api.gridInfo();
    if (info.version < this.info.version) return;
    this.pages.reset(info.version);
    this.info = info;
    if (this.selected !== null && this.selected >= info.len) this.selected = info.len ? info.len - 1 : null;
    this.pageTick++;
  }

  async refreshFolders(): Promise<void> {
    this.folders = await api.listFolders();
  }

  async ensure(start: number, end: number): Promise<void> {
    if (await this.pages.ensure(start, Math.min(end, this.info.len))) this.pageTick++;
  }

  entry(offset: number): GridEntry | undefined {
    void this.pageTick;
    return this.pages.get(offset);
  }

  folderOf(folderId: number): Folder | undefined {
    return this.folderById.get(folderId);
  }

  isOnline(folderId: number): boolean {
    const folder = this.folderById.get(folderId);
    return folder ? (this.onlineByWatched.get(folder.watchedId) ?? true) : true;
  }

  isScanning(watchedId: number): boolean {
    const scan = this.scans[watchedId];
    return !!scan && !scan.done;
  }

  reportError = (e: unknown): void => {
    const id = this.nextToast++;
    this.errors.push({ id, message: errorMessage(e) });
    setTimeout(() => this.dismissError(id), 6000);
  };

  dismissError(id: number): void {
    this.errors = this.errors.filter((t) => t.id !== id);
  }
}

export const library = new LibraryStore();
```

Run: `npm run check && npm test`. Expected: 0 errors and 0 warnings, and the tests pass.

- [ ] **Step 3: Commit**

```bash
git add ui/src/lib
git commit -m "feat(ui): add the typed API layer and the library store"
```

---

### Task 13: Grid, tiles, status bar and app shell

**Files:**
- Create:
  - `ui/src/components/Grid.svelte`
  - `ui/src/components/Tile.svelte`
  - `ui/src/components/StatusBar.svelte`
  - `ui/src/components/Toasts.svelte`
- Modify: `ui/src/App.svelte`

**Interfaces:**
- Consumes: `layout.ts`, `nav.ts`, `library`, `api` and `mediaUrl`.
- Produces:
  - `Grid` with the prop `onopen(offset)` and the exported methods `scrollToOffset(offset, align: 'start' | 'nearest')` and `focus()`.
  - `Tile` with the props `entry`, `selected`, `dimmed`, `onselect` and `onopen`.
  - An App layout with a `sidebar` slot for FolderTree (Task 14) and a viewer slot (Task 15).

- [ ] **Step 1: `Tile.svelte`**

```svelte
<script lang="ts">
  import { mediaUrl, type GridEntry } from '../lib/api';
  import { TILE } from '../lib/layout';

  let {
    entry,
    selected,
    dimmed,
    onselect,
    onopen,
  }: {
    entry: GridEntry | undefined;
    selected: boolean;
    dimmed: boolean;
    onselect: () => void;
    onopen: () => void;
  } = $props();

  const RETRY_MS = 2000;
  let status = $state<'loading' | 'loaded' | 'broken'>('loading');
  let attempt = $state(0);
  const key = $derived(entry ? `${entry.id}/${entry.thumbKey}` : '');
  const src = $derived(
    entry ? mediaUrl(`thumb/${entry.id}/grid/${entry.thumbKey}`) + (attempt ? `?retry=${attempt}` : '') : undefined,
  );

  // A different item or file version starts fresh.
  $effect(() => {
    void key;
    status = 'loading';
    attempt = 0;
  });

  function onerror() {
    if (attempt === 0) setTimeout(() => (attempt = 1), RETRY_MS);
    else status = 'broken';
  }
</script>

<button
  class="tile"
  class:selected
  class:dimmed
  style:width="{TILE}px"
  style:height="{TILE}px"
  tabindex="-1"
  onclick={onselect}
  ondblclick={onopen}
>
  {#if entry && status !== 'broken'}
    <img {src} alt="" draggable="false" decoding="async" class:loaded={status === 'loaded'} onload={() => (status = 'loaded')} {onerror} />
  {:else if status === 'broken'}
    <span class="broken" title="This photo can't be shown">⚠</span>
  {/if}
</button>

<style>
  .tile {
    position: relative;
    flex: none;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 4px;
    background: var(--panel-2);
    overflow: hidden;
    cursor: default;
  }
  .tile.selected { border-color: var(--accent); }
  .tile.dimmed { opacity: 0.4; }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 120ms ease-out;
  }
  img.loaded { opacity: 1; }
  .broken { display: grid; place-items: center; height: 100%; color: var(--muted); font-size: 28px; }
</style>
```

- [ ] **Step 2: `Grid.svelte`**

```svelte
<script lang="ts">
  import { api } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import { buildRows, columnsFor, GAP, itemSpan, rowOfItem, totalHeight, visibleRange } from '../lib/layout';
  import { move, type NavKey } from '../lib/nav';
  import Tile from './Tile.svelte';

  let { onopen }: { onopen: (offset: number) => void } = $props();

  const NAV_KEYS = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'];
  const VISIBLE_DEBOUNCE_MS = 150;

  let viewport: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);

  const columns = $derived(columnsFor(Math.max(0, width - 2 * GAP)));
  const rows = $derived(buildRows(library.info.sections, columns));
  const rendered = $derived.by(() => {
    const [start, end] = visibleRange(rows, scrollTop, height, height * 2);
    return rows.slice(start, end);
  });
  const onScreen = $derived.by(() => {
    const [start, end] = visibleRange(rows, scrollTop, height, 0);
    return itemSpan(rows.slice(start, end));
  });

  $effect(() => {
    const span = itemSpan(rendered);
    if (span) void library.ensure(span[0], span[1]);
  });

  // Tell the thumbnail queue what's on screen once scrolling settles.
  $effect(() => {
    const span = onScreen;
    void library.pageTick;
    const timer = setTimeout(() => {
      if (!span) return;
      const ids: number[] = [];
      for (let o = span[0]; o < span[1]; o++) {
        const e = library.entry(o);
        if (e) ids.push(e.id);
      }
      api.setVisible(ids).catch(() => {});
    }, VISIBLE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  export function scrollToOffset(offset: number, align: 'start' | 'nearest' = 'start') {
    const i = rowOfItem(rows, offset);
    if (i < 0 || !viewport) return;
    const row = rows[i];
    if (align === 'start') {
      const header = rows[i - 1];
      viewport.scrollTop = header?.kind === 'header' && header.first === row.first ? header.top : row.top;
    } else if (row.top < viewport.scrollTop) {
      viewport.scrollTop = row.top;
    } else if (row.top + row.height > viewport.scrollTop + height) {
      viewport.scrollTop = row.top + row.height - height;
    }
  }

  export function focus() {
    viewport?.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    const sel = library.selected;
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'r') {
      e.preventDefault();
      const entry = sel === null ? undefined : library.entry(sel);
      if (entry) api.revealInFileManager(entry.id).catch(library.reportError);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (sel !== null) onopen(sel);
      return;
    }
    if (!NAV_KEYS.includes(e.key) || library.info.len === 0) return;
    e.preventDefault();
    const next = sel === null ? 0 : move(sel, e.key as NavKey, library.info.sections, columns);
    library.selected = next;
    scrollToOffset(next, 'nearest');
  }
</script>

<div
  class="viewport"
  bind:this={viewport}
  bind:clientWidth={width}
  bind:clientHeight={height}
  onscroll={() => (scrollTop = viewport.scrollTop)}
  {onkeydown}
  tabindex="0"
  role="grid"
  aria-label="Photos"
>
  {#if library.info.len === 0}
    <p class="empty">No photos yet. Add a folder to get started.</p>
  {/if}
  <div class="canvas" style:height="{totalHeight(rows)}px">
    {#each rendered as row (row.top)}
      {@const folderId = library.info.sections[row.section].folderId}
      {#if row.kind === 'header'}
        {@const folder = library.folderOf(folderId)}
        <div class="header" style:top="{row.top}px">
          <span class="name">{folder?.name ?? ''}</span>
          <span class="path">{folder?.path ?? ''}</span>
        </div>
      {:else}
        <div class="row" style:top="{row.top}px" style:gap="{GAP}px" style:padding-left="{GAP}px">
          {#each { length: row.count } as _, i (row.first + i)}
            {@const offset = row.first + i}
            <Tile
              entry={library.entry(offset)}
              selected={library.selected === offset}
              dimmed={!library.isOnline(folderId)}
              onselect={() => (library.selected = offset)}
              onopen={() => onopen(offset)}
            />
          {/each}
        </div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .viewport { position: relative; height: 100%; overflow-y: auto; outline: none; }
  .canvas { position: relative; }
  .header, .row { position: absolute; left: 0; right: 0; }
  .header { display: flex; align-items: baseline; gap: 12px; height: 32px; padding: 8px 8px 0; }
  .header .name { font-weight: 600; }
  .header .path { color: var(--muted); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row { display: flex; }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; color: var(--muted); margin: 0; }
</style>
```

- [ ] **Step 3: `StatusBar.svelte` and `Toasts.svelte`**

`StatusBar.svelte`:

```svelte
<script lang="ts">
  import { library } from '../lib/library.svelte';

  const scanning = $derived(
    library.folders.watched
      .filter((w) => library.isScanning(w.id))
      .map((w) => `Scanning ${w.path.split(/[\\/]/).pop()}… ${library.scans[w.id].filesSeen.toLocaleString()} files`),
  );
</script>

<footer class="status">
  <span>{scanning.join(' · ')}</span>
  <span>{library.info.len.toLocaleString()} photos</span>
</footer>

<style>
  .status {
    display: flex;
    justify-content: space-between;
    padding: 4px 12px;
    background: var(--panel);
    color: var(--muted);
    font-size: 12px;
    border-top: 1px solid #0003;
  }
</style>
```

`Toasts.svelte`:

```svelte
<script lang="ts">
  import { library } from '../lib/library.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each library.errors as toast (toast.id)}
    <div class="toast" role="alert">
      <span>{toast.message}</span>
      <button onclick={() => library.dismissError(toast.id)} aria-label="Dismiss">✕</button>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: 16px; bottom: 40px; display: flex; flex-direction: column; gap: 8px; z-index: 30; }
  .toast {
    display: flex;
    gap: 12px;
    align-items: center;
    max-width: 420px;
    padding: 10px 12px;
    background: var(--panel-2);
    border-left: 3px solid var(--danger);
    border-radius: 4px;
    box-shadow: 0 4px 16px #0006;
  }
  .toast button { border: 0; background: none; color: var(--muted); cursor: pointer; }
</style>
```

- [ ] **Step 4: `App.svelte`**

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { library } from './lib/library.svelte';
  import Grid from './components/Grid.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Toasts from './components/Toasts.svelte';

  let grid: ReturnType<typeof Grid> | undefined = $state();

  onMount(() => {
    library.init().catch(library.reportError);
    return () => library.dispose();
  });

  function open(offset: number) {
    library.selected = offset;
  }
</script>

<div class="app">
  <aside class="sidebar"></aside>
  <main class="content">
    <Grid bind:this={grid} onopen={open} />
  </main>
  <div class="statusbar"><StatusBar /></div>
</div>
<Toasts />

<style>
  .app {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: 1fr auto;
    height: 100%;
  }
  .sidebar {
    width: 260px;
    min-width: 160px;
    max-width: 50vw;
    resize: horizontal;
    overflow: auto;
    background: var(--panel);
    border-right: 1px solid #0003;
  }
  .content { min-width: 0; min-height: 0; }
  .statusbar { grid-column: 1 / -1; }
</style>
```

Tasks 14 and 15 fill in the sidebar and the viewer.

- [ ] **Step 5: Check and build**

Run: `npm run check && npm test && npm run -w ui build`
Expected: 0 errors and 0 warnings, and the build succeeds. Fix every svelte-check warning (for example accessibility warnings) rather than suppressing it, unless a suppression is clearly justified. If you do suppress one, record it in the report.

- [ ] **Step 6: Commit**

```bash
git add ui
git commit -m "feat(ui): add the virtualized photo grid, status bar and app shell"
```

---

### Task 14: Folder tree and folder management

**Files:**
- Create: `ui/src/components/FolderTree.svelte`
- Modify: `ui/src/App.svelte`

**Interfaces:**
- Consumes:
  - `library` and `api` (addFolder, removeFolder, rescanFolder, revealFolder, gridOffsetOfFolder);
  - `@tauri-apps/plugin-dialog` `open` and `ask`;
  - `Grid.scrollToOffset`.
- Produces: `FolderTree` with the prop `onjump(folderId)`.

- [ ] **Step 1: `FolderTree.svelte`**

```svelte
<script lang="ts">
  import { ask, open } from '@tauri-apps/plugin-dialog';
  import { api, type Folder } from '../lib/api';
  import { library } from '../lib/library.svelte';

  let { onjump }: { onjump: (folderId: number) => void } = $props();

  const children = $derived.by(() => {
    const map = new Map<number | null, Folder[]>();
    for (const f of library.folders.folders) {
      const list = map.get(f.parentId) ?? [];
      list.push(f);
      map.set(f.parentId, list);
    }
    return map;
  });

  let menu = $state<{ x: number; y: number; folder: Folder } | null>(null);

  const watchedOf = (f: Folder) => library.folders.watched.find((w) => w.id === f.watchedId);

  async function addFolder() {
    const path = await open({ directory: true, multiple: false, title: 'Add a folder to photon' });
    if (typeof path !== 'string') return;
    try {
      await api.addFolder(path);
      await library.refreshFolders();
    } catch (e) {
      library.reportError(e);
    }
  }

  async function rescan(f: Folder) {
    menu = null;
    await api.rescanFolder(f.watchedId).catch(library.reportError);
  }

  async function reveal(f: Folder) {
    menu = null;
    await api.revealFolder(f.id).catch(library.reportError);
  }

  async function remove(f: Folder) {
    menu = null;
    const watched = watchedOf(f);
    if (!watched) return;
    const confirmed = await ask(`Remove “${watched.path}” from photon? Your files stay where they are.`, {
      title: 'Remove folder',
      kind: 'warning',
    });
    if (!confirmed) return;
    await api.removeFolder(watched.id).catch(library.reportError);
    await library.refreshFolders();
  }

  function openMenu(e: MouseEvent, folder: Folder) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, folder };
  }
</script>

<svelte:window onclick={() => (menu = null)} onkeydown={(e) => e.key === 'Escape' && (menu = null)} />

<nav class="tree" aria-label="Folders">
  <div class="toolbar">
    <button class="add" onclick={addFolder}>Add folder…</button>
  </div>

  {#snippet node(f: Folder, depth: number)}
    {@const watched = watchedOf(f)}
    <button
      class="node"
      class:offline={watched && !watched.online}
      style:padding-left="{8 + depth * 14}px"
      title={f.path}
      onclick={() => onjump(f.id)}
      oncontextmenu={(e) => openMenu(e, f)}
    >
      <span class="name">{f.name}</span>
      {#if depth === 0 && library.isScanning(f.watchedId)}
        <span class="spinner" aria-label="Scanning"></span>
      {/if}
    </button>
    {#each children.get(f.id) ?? [] as child (child.id)}
      {@render node(child, depth + 1)}
    {/each}
  {/snippet}

  {#each children.get(null) ?? [] as root (root.id)}
    {@render node(root, 0)}
  {/each}

  {#if library.folders.watched.length === 0}
    <p class="empty">No folders yet.</p>
  {/if}
</nav>

{#if menu}
  {@const f = menu.folder}
  <div class="menu" role="menu" style:left="{menu.x}px" style:top="{menu.y}px">
    <button role="menuitem" onclick={() => rescan(f)}>Rescan</button>
    <button role="menuitem" onclick={() => reveal(f)}>Reveal in file manager</button>
    {#if f.parentId === null}
      <button role="menuitem" class="danger" onclick={() => remove(f)}>Remove from photon</button>
    {/if}
  </div>
{/if}

<style>
  .tree { display: flex; flex-direction: column; padding-bottom: 12px; }
  .toolbar { padding: 8px; }
  .add { width: 100%; padding: 6px; border: 1px solid #fff2; border-radius: 4px; background: var(--panel-2); cursor: pointer; }
  .node {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .node:hover { background: #ffffff0d; }
  .node.offline { opacity: 0.45; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .spinner {
    width: 10px;
    height: 10px;
    border: 2px solid var(--muted);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .empty { padding: 8px 12px; color: var(--muted); }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 4px;
    background: var(--panel-2);
    border-radius: 6px;
    box-shadow: 0 6px 24px #0008;
  }
  .menu button { padding: 6px 10px; border: 0; background: none; text-align: left; cursor: pointer; border-radius: 4px; }
  .menu button:hover { background: #ffffff14; }
  .menu .danger { color: var(--danger); }
</style>
```

- [ ] **Step 2: Mount it in `App.svelte`**

- Import `api` and `FolderTree`.
- Replace `<aside class="sidebar"></aside>` with `<aside class="sidebar"><FolderTree onjump={jump} /></aside>`.
- Add:

```ts
  async function jump(folderId: number) {
    const offset = await api.gridOffsetOfFolder(folderId).catch(() => null);
    if (offset === null) return;
    library.selected = offset;
    grid?.scrollToOffset(offset, 'start');
  }
```

- [ ] **Step 3: Check and build**

Run: `npm run check && npm test && npm run -w ui build`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add ui
git commit -m "feat(ui): add the folder tree with add, rescan, reveal and remove"
```

---

### Task 15: Viewer

**Files:**
- Create: `ui/src/components/Viewer.svelte`
- Modify: `ui/src/App.svelte`

**Interfaces:**
- Consumes: `library` (ensure, entry, info.len), `api.viewerItem`, `api.neighbours` and `mediaUrl`.
- Produces: `Viewer` with the props `offset` (the starting grid offset) and `onclose(offset)`.

- [ ] **Step 1: `Viewer.svelte`**

```svelte
<script lang="ts">
  import { untrack } from 'svelte';
  import { api, errorMessage, mediaUrl, type ViewerItem } from '../lib/api';
  import { library } from '../lib/library.svelte';

  let { offset, onclose }: { offset: number; onclose: (offset: number) => void } = $props();

  const PRELOAD_RADIUS = 2;
  let current = $state(untrack(() => offset));
  let item = $state<ViewerItem | null>(null);
  let fullSrc = $state<string | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    const at = current;
    let cancelled = false;
    item = null;
    fullSrc = null;
    error = null;
    (async () => {
      await library.ensure(at, at + 1);
      const entry = library.entry(at);
      if (!entry || cancelled) return;
      const it = await api.viewerItem(entry.id);
      if (cancelled) return;
      item = it;
      if (it.thumbState === 'failed') {
        error = it.thumbError ?? "This photo can't be shown.";
        return;
      }
      const url = mediaUrl(`image/${it.id}`);
      const full = new Image();
      full.src = url;
      full.decode().then(
        () => {
          if (!cancelled) fullSrc = url;
        },
        () => {},
      );
      const near = await api.neighbours(it.id, PRELOAD_RADIUS);
      if (cancelled) return;
      for (const id of near) new Image().src = mediaUrl(`image/${id}`);
    })().catch((e) => {
      if (!cancelled) error = errorMessage(e);
    });
    return () => {
      cancelled = true;
    };
  });

  function onkeydown(e: KeyboardEvent) {
    const last = library.info.len - 1;
    const next =
      e.key === 'ArrowLeft' ? Math.max(0, current - 1)
      : e.key === 'ArrowRight' ? Math.min(last, current + 1)
      : e.key === 'Home' ? 0
      : e.key === 'End' ? last
      : null;
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose(current);
    } else if (next !== null) {
      e.preventDefault();
      current = next;
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="viewer" role="dialog" aria-modal="true" aria-label="Photo viewer">
  {#if error}
    <p class="error">{error}</p>
  {:else if item}
    <img class="preview" src={mediaUrl(`thumb/${item.id}/preview/${item.thumbKey}`)} alt="" class:hidden={!!fullSrc} />
    {#if fullSrc}
      <img class="full" src={fullSrc} alt={item.fileName} />
    {/if}
  {/if}
  <div class="caption">{item?.fileName ?? ''} · {current + 1} / {library.info.len}</div>
  <button class="close" onclick={() => onclose(current)} aria-label="Close viewer">✕</button>
</div>

<style>
  .viewer { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; background: #000; }
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; image-orientation: from-image; }
  .hidden { visibility: hidden; }
  .caption { position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); padding: 4px 10px; background: #0009; border-radius: 4px; color: var(--muted); font-size: 12px; }
  .close { position: absolute; top: 12px; right: 12px; width: 32px; height: 32px; border: 0; border-radius: 50%; background: #0009; cursor: pointer; }
  .error { color: var(--muted); }
</style>
```

- [ ] **Step 2: Wire it into `App.svelte`**

- Import `Viewer`, and add `let viewerAt = $state<number | null>(null);`.
- Change `open` to set `library.selected = offset; viewerAt = offset;`.
- Add:

```ts
  function closeViewer(at: number) {
    viewerAt = null;
    library.selected = at;
    grid?.scrollToOffset(at, 'nearest');
    grid?.focus();
  }
```

- Put `inert={viewerAt !== null}` on `<main class="content">` and on `<aside class="sidebar">`.
- Render `{#if viewerAt !== null}<Viewer offset={viewerAt} onclose={closeViewer} />{/if}` before `<Toasts />`.

- [ ] **Step 3: Check and build**

Run: `npm run check && npm test && npm run -w ui build`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add ui
git commit -m "feat(ui): add the full-window viewer with preview-first loading and preloading"
```

---

### Task 16: CI, developer docs and a full build

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `README.md`

**Interfaces:**
- Consumes: everything.
- Produces:
  - CI with a `rust` job on all three OSes (with the Linux webkit dependencies) and a `ui` job;
  - a README with setup, run and test instructions and the manual smoke checklist;
  - a verified release build.

- [ ] **Step 1: Update CI**

Replace `.github/workflows/ci.yml` with:

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

jobs:
  rust:
    name: rust (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v5
      - name: Install Linux webview dependencies
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev libxdo-dev libssl-dev
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - run: cargo bench -p photon-core --bench grid --no-run

  ui:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: actions/setup-node@v5
        with:
          node-version: 22
          cache: npm
      - run: npm ci
      - run: npm run check
      - run: npm test
      - run: npm run -w ui build
```

If Task 5 found that `cargo build -p photon-app` needs `ui/dist` to exist, add a step to the `rust` job before clippy that creates it, such as a Node setup followed by `npm ci && npm run -w ui build`.

- [ ] **Step 2: Write `README.md`**

````markdown
# photon

A fast, local photo manager for Linux, macOS and Windows. A spiritual successor to Picasa 3.

photon watches folders in place. It never moves or changes your files. It keeps a small
SQLite library and a thumbnail cache in your user data and cache directories.

## Development

Prerequisites:
- Rust (stable, 1.88 or newer). `mise use rust@stable` works.
- Node.js 22 or newer.
- The [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS. On Linux that means webkit2gtk-4.1, libsoup-3 and librsvg2.

```bash
npm install        # installs the UI workspace and the Tauri CLI
npm run dev        # runs the app with hot reload
npm test           # UI unit tests
npm run check      # svelte-check
cargo test --workspace
```

Build a release bundle for this OS with `npm run tauri build`.

## Manual smoke checklist

Run this before each release, on each OS:

- [ ] On first launch with a fresh profile, the Pictures folder is added and scanned without asking.
- [ ] Thumbnails appear within seconds, and scrolling stays smooth while indexing continues.
- [ ] Double-click or Enter opens the viewer with an image in about 100 ms. The full resolution follows. ←/→, Home/End and Esc work.
- [ ] "Add folder…" adds a folder. Adding a folder inside a watched one is refused with a clear message.
- [ ] Rescan works. "Remove from photon" asks first, works during a scan, and leaves the files on disk.
- [ ] Unplugging a drive with a watched folder dims its folder and tiles after a rescan. Nothing disappears.
- [ ] Ctrl/Cmd+Shift+R and "Reveal in file manager" open the system file manager at the file.
````

- [ ] **Step 3: Full verification**

Run:

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
npm ci && npm run check && npm test
npm run tauri build -- --debug --no-bundle
```

Expected: everything passes, and the debug build produces `target/debug/photon-app` (or `photon-app.exe`). Don't launch the binary; the user runs the smoke checklist. Record the build output tail in your report.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml README.md
git commit -m "chore: add the UI and app to CI and document development"
```

---

## Spec coverage (Plan 2)

| Spec section | Task |
|---|---|
| §2.1 folder validation (canonicalise, overlap, excluded, case rules) | 1 |
| §2.2 scanner exclusions and cancellation | 2 |
| §2.3 `request` with shared decodes and a timeout | 3 |
| §2.4 `thumb_key` in grid rows | 4 |
| §3.1 engine lifecycle, Pictures on first launch, garbage collection after the first scans | 6, 9 |
| §3.2 scan manager (one scan per folder, throttled rebuilds and events, cancel on remove) | 6 |
| §3.3 commands (+ `reveal_folder` for the tree's context menu) | 7, 9 |
| §3.4 events | 6, 9 |
| §3.5 `photon://` protocol, status codes and caching | 8, 9 |
| §4.1 UI units | 5, 10–15 |
| §4.2 grid geometry, dimming, selection and keys | 10, 11, 13 |
| §4.3 viewer | 15 |
| §5 error handling (toasts, broken tiles, retry, library open failure) | 9, 12, 13 |
| §6 testing and CI | every task, 16 |
| §7 success criteria | README smoke checklist (16) |

Deliberate refinements of the spec:
- `reveal_folder` was added. The tree's "Reveal in file manager" needs it, because folders have no item id.
- Tiles retry once for any error, because an `<img>` element can't read the status code. This matches the amended spec §5.
