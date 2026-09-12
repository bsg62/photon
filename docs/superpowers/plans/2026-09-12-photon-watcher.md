# photon Filesystem Watcher (Plan 3 of 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** photon notices changes on disk as they happen, instead of only at startup or on a manual rescan.

**Architecture:** A new `scan_subtree` restricts the existing scanner to one directory, so a filesystem event costs one small walk instead of a full rescan. A `watcher` module wraps `notify` with debouncing and keeps its policy in pure functions. A `WatcherService` in photon-app turns debounced events into subtree scans through the existing scan manager, so watcher work is serialised per folder and cancellable exactly like a manual rescan.

**Tech Stack:** Rust (edition 2024, rust-version 1.88), `notify` 8.2, `notify-debouncer-full` 0.7, plus the existing photon-core and photon-app crates and the Svelte 5 UI.

**Spec:** `docs/superpowers/specs/2026-09-12-photon-plan3-watcher-design.md`. Its parent, `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`, stays binding (as amended: HEIC/AVIF and video are deferred, so v1 is JPEG, PNG, GIF and WebP).

## Global Constraints

- **The subtree scan must never touch anything outside the directory it scanned.** This is the plan's central safety property: the walk, the known-items set it diffs against, marking and purging, and folder pruning are all restricted to that subtree.
- Subtree membership is resolved through the folder tree (`parent_id`), never by matching path text, so `%` and `_` in filenames need no escaping.
- A subtree scan never changes a watched folder's online flag, with one exception: if the watched root itself has gone, it marks the folder offline and returns, exactly as `scan_watched` does.
- A subtree scan has **no empty-directory guard**. `scan_watched` treats an empty root as an unmounted volume; an empty subdirectory legitimately means its files were deleted.
- Timings: debounce `2s`; pending follow-ups drained every `2s`; offline roots polled every `30s`; degraded roots rescanned every `5min`.
- photon never writes to, moves or deletes files inside watched folders.
- Scanner batching stays at `500` rows per transaction.
- Platforms: Linux, macOS and Windows. Tests must pass on all three; build paths with `Path::join`, and split test names on `/`.
- Every task ends with `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` passing, and — once the UI is touched — `npm run check` at 0 errors and 0 warnings plus `npm test`.
- Never launch the GUI app. Never push. Conventional Commits.

## File Structure

```
crates/photon-core/src/library/items.rs      + known_items_under
crates/photon-core/src/library/folders.rs    + prune_folders_under
crates/photon-core/src/scanner.rs            walk extracted into a shared helper; + scan_subtree
crates/photon-core/src/watcher/mod.rs        NEW: re-exports
crates/photon-core/src/watcher/policy.rs     NEW: pure event → scan-request policy
crates/photon-core/src/watcher/fs.rs         NEW: notify-backed watcher
crates/photon-app/src/engine.rs              + start_subtree_scan
crates/photon-app/src/watch.rs               NEW: WatcherService
crates/photon-app/src/events.rs              FolderStatus gains `degraded`
crates/photon-app/src/app.rs                 starts and stops the service
ui/src/lib/api.ts, library.svelte.ts         carry `degraded`
ui/src/components/StatusBar.svelte           shows the degraded notice
README.md                                    smoke checklist additions
```

---

### Task 1: Subtree-restricted library queries

**Files:**
- Modify: `crates/photon-core/src/library/items.rs`, `crates/photon-core/src/library/folders.rs`

**Interfaces:**
- Consumes: the existing `folders` table (`id`, `watched_id`, `parent_id`, `path`, `seen_scan`) and `items` table.
- Produces:
  - `Library::known_items_under(&self, watched_id: i64, dir: &str) -> Result<HashMap<String, KnownItem>>` — every item in the folder whose path is `dir` and in all its descendant folders, including soft-deleted ones. A `dir` with no folder row yields an empty map.
  - `Library::prune_folders_under(&self, watched_id: i64, scan_id: i64, dir: &str) -> Result<usize>` — like `prune_folders`, but only within that subtree.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/photon-core/src/library/items.rs`:

```rust
    #[test]
    fn known_items_under_covers_only_that_subtree() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let deep = lib.upsert_folder(watched, Some(a), "/p/a/deep", 1).unwrap();
        let b = lib.upsert_folder(watched, Some(root), "/p/b", 1).unwrap();
        lib.insert_items(&[
            new_item(root, "/p/top.jpg", 1),
            new_item(a, "/p/a/one.jpg", 2),
            new_item(deep, "/p/a/deep/two.jpg", 3),
            new_item(b, "/p/b/three.jpg", 4),
        ])
        .unwrap();

        let under_a = lib.known_items_under(watched, "/p/a").unwrap();
        let mut paths: Vec<&str> = under_a.keys().map(String::as_str).collect();
        paths.sort();
        assert_eq!(paths, ["/p/a/deep/two.jpg", "/p/a/one.jpg"]);

        assert_eq!(lib.known_items_under(watched, "/p").unwrap().len(), 4);
        assert!(lib.known_items_under(watched, "/p/missing").unwrap().is_empty());
    }

    #[test]
    fn known_items_under_includes_soft_deleted_items() {
        let (_dir, lib) = temp_library();
        let (watched, root) = seed_folder(&lib, Path::new("/p"));
        let a = lib.upsert_folder(watched, Some(root), "/p/a", 1).unwrap();
        let ids = lib.insert_items(&[new_item(a, "/p/a/one.jpg", 1)]).unwrap();
        lib.mark_missing(&ids, 99).unwrap();

        let under = lib.known_items_under(watched, "/p/a").unwrap();
        assert!(under["/p/a/one.jpg"].missing);
    }
```

Add to the `tests` module in `crates/photon-core/src/library/folders.rs`:

```rust
    #[test]
    fn prune_folders_under_stays_inside_the_subtree() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        let a = lib.upsert_folder(w.id, Some(root), "/p/a", 1).unwrap();
        lib.upsert_folder(w.id, Some(a), "/p/a/gone", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/b", 1).unwrap();
        lib.upsert_folder(w.id, Some(root), "/p/c-stale", 1).unwrap();

        // A later scan of /p/a only saw /p/a itself.
        lib.upsert_folder(w.id, Some(root), "/p/a", 2).unwrap();
        assert_eq!(lib.prune_folders_under(w.id, 2, "/p/a").unwrap(), 1);

        let paths: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["/p", "/p/a", "/p/b", "/p/c-stale"]);
    }

    #[test]
    fn prune_folders_under_removes_an_empty_chain() {
        let (_dir, lib) = temp_library();
        let w = watch(&lib, "/p");
        let root = lib.upsert_folder(w.id, None, "/p", 1).unwrap();
        let a = lib.upsert_folder(w.id, Some(root), "/p/a", 1).unwrap();
        let mid = lib.upsert_folder(w.id, Some(a), "/p/a/mid", 1).unwrap();
        lib.upsert_folder(w.id, Some(mid), "/p/a/mid/leaf", 1).unwrap();

        lib.upsert_folder(w.id, Some(root), "/p/a", 2).unwrap();
        assert_eq!(lib.prune_folders_under(w.id, 2, "/p/a").unwrap(), 2);
        let paths: Vec<String> = lib.folders().unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["/p", "/p/a"]);
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core known_items_under prune_folders_under`
Expected: compile errors, because neither method exists.

- [ ] **Step 3: Implement**

In `crates/photon-core/src/library/items.rs`, add to the `impl Library` block:

```rust
    /// Every item in `dir`'s folder and all folders beneath it, keyed by path, including
    /// soft-deleted ones. Membership comes from the folder tree rather than a path-prefix
    /// match, so `%` and `_` in a filename need no escaping and a directory with no folder
    /// row simply yields nothing.
    pub fn known_items_under(
        &self,
        watched_id: i64,
        dir: &str,
    ) -> Result<HashMap<String, KnownItem>> {
        let conn = self.reader();
        let mut stmt = conn.prepare(
            "WITH RECURSIVE sub(id) AS (
                 SELECT id FROM folders WHERE watched_id = ?1 AND path = ?2
                 UNION ALL
                 SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id
             )
             SELECT i.path, i.id, i.size, i.mtime_ms, i.missing_since IS NOT NULL
             FROM items i WHERE i.folder_id IN (SELECT id FROM sub)",
        )?;
        let rows = stmt
            .query_map(params![watched_id, dir], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    KnownItem {
                        id: r.get(1)?,
                        size: r.get(2)?,
                        mtime_ms: r.get(3)?,
                        missing: r.get(4)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }
```

In `crates/photon-core/src/library/folders.rs`, add to the `impl Library` block:

```rust
    /// `prune_folders`, restricted to `dir` and everything beneath it. A subtree scan must
    /// never prune folders it did not walk.
    pub fn prune_folders_under(&self, watched_id: i64, scan_id: i64, dir: &str) -> Result<usize> {
        let conn = self.writer();
        let mut total = 0;
        loop {
            let removed = conn.execute(
                "WITH RECURSIVE sub(id) AS (
                     SELECT id FROM folders WHERE watched_id = ?1 AND path = ?3
                     UNION ALL
                     SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id
                 )
                 DELETE FROM folders
                 WHERE id IN (SELECT id FROM sub) AND seen_scan < ?2
                   AND NOT EXISTS (SELECT 1 FROM items WHERE items.folder_id = folders.id)
                   AND NOT EXISTS (SELECT 1 FROM folders c WHERE c.parent_id = folders.id)",
                params![watched_id, scan_id, dir],
            )?;
            if removed == 0 {
                return Ok(total);
            }
            total += removed;
        }
    }
```

If SQLite rejects a `WITH` clause before `DELETE` in the installed version, fall back to collecting the subtree ids with a `SELECT` and deleting with an `IN (...)` list built from them, keeping the same loop and the same conditions. Record which form you used.

- [ ] **Step 4: Run the tests and commit**

Run: `cargo test -p photon-core && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

```bash
git add crates/photon-core
git commit -m "feat(core): add subtree-restricted item and folder queries"
```

---

### Task 2: Extract the scanner's shared walk

**Files:**
- Modify: `crates/photon-core/src/scanner.rs`

This task changes **no behaviour**. It carves a seam so `scan_subtree` can reuse the walk instead of duplicating 100 lines of it, which a reviewer would rightly call out.

**Interfaces:**
- Produces (all private to the module):
  - `struct WalkOutcome { report: ScanReport, seen: ScanProgress, incomplete_prefixes: Vec<PathBuf>, skip_mark_purge: bool, cancelled: bool }`
  - `fn walk_tree(lib, watched_id: i64, root: &Path, root_parent_id: Option<i64>, known: &mut HashMap<String, KnownItem>, folder_ids: &mut HashMap<PathBuf, i64>, scan_id: i64, options: &ScanOptions, progress: &mut dyn FnMut(&ScanProgress)) -> Result<WalkOutcome>`
  - `fn finish_mark_purge(lib: &Library, known: HashMap<String, KnownItem>) -> Result<(u64, u64)>` — returns `(marked, purged)`.
- `scan_watched` keeps its exact signature and behaviour.

- [ ] **Step 1: Extract, without changing behaviour**

Move the body of `scan_watched` between `let walker = WalkDir::new(root)` and the two final `flush_*` calls into `walk_tree`, with these changes only:

- it takes `watched_id` instead of a `&WatchedFolder`;
- it walks `root` (its parameter) instead of the watched path;
- the depth-0 folder's parent becomes `root_parent_id` instead of always `None`:

```rust
        if entry.file_type().is_dir() {
            let parent = if entry.depth() == 0 {
                root_parent_id
            } else {
                path.parent().and_then(|p| folder_ids.get(p)).copied()
            };
            let id = lib.upsert_folder(watched_id, parent, path_str, scan_id)?;
            folder_ids.insert(path.to_path_buf(), id);
            continue;
        }
```

- it ends with the two `flush_*` calls and returns `WalkOutcome`.

Extract the mark-and-purge tail into:

```rust
/// Soft-deletes what this walk didn't find, and purges what was already missing.
/// Both are chunked at `BATCH` rows per transaction.
fn finish_mark_purge(lib: &Library, known: HashMap<String, KnownItem>) -> Result<(u64, u64)> {
    let (mut to_mark, mut to_purge) = (Vec::new(), Vec::new());
    for k in known.into_values() {
        if k.missing {
            to_purge.push(k.id)
        } else {
            to_mark.push(k.id)
        }
    }
    let now = crate::now_ms();
    for chunk in to_mark.chunks(BATCH) {
        lib.mark_missing(chunk, now)?;
    }
    for chunk in to_purge.chunks(BATCH) {
        lib.purge_items(chunk)?;
    }
    Ok((to_mark.len() as u64, to_purge.len() as u64))
}
```

`scan_watched` becomes the same logic expressed through them, keeping every comment that explains *why* (the cancelled case, the `skip_mark_purge` case, the incomplete-prefix filter, and the empty-root guard), and passing `None` as `root_parent_id`.

- [ ] **Step 2: Prove nothing changed**

Run: `cargo test -p photon-core scanner && cargo test -p photon-core && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: every existing scanner test passes untouched. Do not add, change or delete a single test in this task — an unchanged suite is the evidence that this refactor is behaviour-preserving.

- [ ] **Step 3: Commit**

```bash
git add crates/photon-core/src/scanner.rs
git commit -m "refactor(core): extract the scanner's shared walk and mark/purge tail"
```

---

### Task 3: `scan_subtree`

**Files:**
- Modify: `crates/photon-core/src/scanner.rs`

**Interfaces:**
- Consumes: Task 1's `known_items_under` and `prune_folders_under`; Task 2's `walk_tree` and `finish_mark_purge`; `crate::paths::{is_within, same_path}`.
- Produces: `pub fn scan_subtree(lib: &Library, watched: &WatchedFolder, dir: &Path, scan_id: i64, options: &ScanOptions, progress: &mut dyn FnMut(&ScanProgress)) -> Result<ScanReport>`.

- [ ] **Step 1: Write the failing tests**

Add to the scanner's `tests` module:

```rust
    fn scan_sub(lib: &Library, watched: &WatchedFolder, dir: &Path, scan_id: i64) -> ScanReport {
        scan_subtree(lib, watched, dir, scan_id, &ScanOptions::default(), &mut |_| {}).unwrap()
    }

    #[test]
    fn subtree_scan_leaves_everything_outside_alone() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let keep = write_file(&root, "b/keep.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        // Delete a file in the *other* folder; scanning /a must not notice or touch it.
        fs::remove_file(&keep).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a"), 2);

        assert_eq!((report.marked_missing, report.purged), (0, 0));
        let id = lib.known_items(watched.id).unwrap()[&key(&keep)].id;
        assert_eq!(lib.item(id).unwrap().unwrap().missing_since, None);
    }

    #[test]
    fn subtree_scan_finds_additions_and_removals_inside_it() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let gone = write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        let id = lib.known_items(watched.id).unwrap()[&key(&gone)].id;

        write_file(&root, "a/two.jpg", &jpeg_bytes(8, 8));
        fs::remove_file(&gone).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a"), 2);
        assert_eq!((report.added, report.marked_missing), (1, 1));
        assert!(lib.item(id).unwrap().unwrap().missing_since.is_some());

        // A second subtree scan purges it, as a full scan would.
        let report = scan_sub(&lib, &watched, &root.join("a"), 3);
        assert_eq!(report.purged, 1);
        assert!(lib.item(id).unwrap().is_none());
    }

    #[test]
    fn subtree_scan_of_a_deleted_directory_uses_its_nearest_living_ancestor() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/sub/one.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "a/keep.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        fs::remove_dir_all(root.join("a").join("sub")).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a").join("sub"), 2);

        assert_eq!(report.marked_missing, 1);
        assert_eq!(report.unchanged, 1, "the surviving sibling was walked");
        scan_sub(&lib, &watched, &root.join("a").join("sub"), 3);
        assert!(lib.folders().unwrap().iter().all(|f| f.name != "sub"));
    }

    #[test]
    fn subtree_scan_of_an_empty_directory_does_not_report_offline() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let only = write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        // Emptying a subdirectory is a real deletion, not an unmounted volume.
        fs::remove_file(&only).unwrap();
        let report = scan_sub(&lib, &watched, &root.join("a"), 2);
        assert!(!report.offline);
        assert_eq!(report.marked_missing, 1);
        assert!(lib.watched_folders().unwrap()[0].online);
    }

    #[test]
    fn subtree_scan_delegates_to_a_full_scan_at_the_root() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        let report = scan_sub(&lib, &watched, &root, 1);
        assert_eq!(report.added, 1);
    }

    #[test]
    fn subtree_scan_refuses_a_directory_outside_the_watched_folder() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        let report = scan_sub(&lib, &watched, &outside, 2);
        assert_eq!(report, ScanReport::default());
        assert_eq!(lib.known_items(watched.id).unwrap().len(), 1);
    }

    #[test]
    fn subtree_scan_skips_excluded_directories_and_honours_cancel() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a/one.jpg", &jpeg_bytes(8, 8));
        write_file(&root, "a/cache/two.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let excluded = ScanOptions {
            excluded: vec![root.join("a").join("cache")],
            ..ScanOptions::default()
        };
        let report =
            scan_subtree(&lib, &watched, &root.join("a"), 1, &excluded, &mut |_| {}).unwrap();
        assert_eq!(report.added, 1);

        let cancelled = ScanOptions::default();
        cancelled.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let report =
            scan_subtree(&lib, &watched, &root.join("a"), 2, &cancelled, &mut |_| {}).unwrap();
        assert!(report.cancelled);
        assert_eq!(report.marked_missing, 0);
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p photon-core subtree`
Expected: compile error, because `scan_subtree` does not exist.

- [ ] **Step 3: Implement**

```rust
/// Brings one directory and everything beneath it in line with what is on disk.
///
/// Everything this touches is restricted to that subtree: the walk, the known-items set it
/// diffs against, what it marks or purges, and which folders it prunes. `scan_watched`'s
/// diff compares against every item under the watched root, so aiming that logic at one
/// directory would make the rest of the library look deleted.
///
/// Differences from [`scan_watched`], both deliberate:
/// - it never changes the watched folder's online flag, except when the watched root itself
///   has gone, which it reports exactly as `scan_watched` does;
/// - it has no empty-directory guard. An empty root means an unmounted volume; an empty
///   subdirectory means its files really were deleted.
///
/// A `dir` that no longer exists is not an error: the nearest ancestor that still exists is
/// scanned instead, which is what makes a deleted folder disappear from the library.
pub fn scan_subtree(
    lib: &Library,
    watched: &WatchedFolder,
    dir: &Path,
    scan_id: i64,
    options: &ScanOptions,
    progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport> {
    let root = Path::new(&watched.path);
    if !root.is_dir() {
        lib.set_watched_online(watched.id, false)?;
        return Ok(ScanReport {
            offline: true,
            ..ScanReport::default()
        });
    }
    if !crate::paths::is_within(dir, root) {
        tracing::warn!(?dir, watched = %watched.path, "ignoring a subtree outside its watched folder");
        return Ok(ScanReport::default());
    }

    // A deleted directory is scanned through its nearest living ancestor, so the parent's
    // walk sees it gone, marks its items missing and eventually prunes it.
    let mut target = dir.to_path_buf();
    while !target.is_dir() {
        match target.parent() {
            Some(parent) if crate::paths::is_within(parent, root) => {
                target = parent.to_path_buf()
            }
            _ => {
                target = root.to_path_buf();
                break;
            }
        }
    }
    if crate::paths::same_path(&target, root) {
        return scan_watched(lib, watched, scan_id, options, progress);
    }

    let target_str = target
        .to_str()
        .ok_or_else(|| crate::Error::NonUtf8Path(target.clone()))?;
    let mut known = lib.known_items_under(watched.id, target_str)?;
    let (mut folder_ids, parent_id) = seed_ancestors(lib, watched, &target, scan_id)?;

    let outcome = walk_tree(
        lib,
        watched.id,
        &target,
        parent_id,
        &mut known,
        &mut folder_ids,
        scan_id,
        options,
        progress,
    )?;

    if outcome.cancelled {
        progress(&outcome.seen);
        return Ok(ScanReport {
            cancelled: true,
            ..outcome.report
        });
    }
    if outcome.skip_mark_purge {
        progress(&outcome.seen);
        return Ok(outcome.report);
    }

    let mut known = known;
    known.retain(|path_str, _| {
        !outcome
            .incomplete_prefixes
            .iter()
            .any(|prefix| Path::new(path_str).starts_with(prefix))
    });

    let mut report = outcome.report;
    let (marked, purged) = finish_mark_purge(lib, known)?;
    lib.prune_folders_under(watched.id, scan_id, target_str)?;
    report.marked_missing = marked;
    report.purged = purged;

    progress(&outcome.seen);
    Ok(report)
}

/// Upserts the folder rows from the watched root down to `target`'s parent, so the walk can
/// attach `target` to its real parent rather than treating it as a root. Returns the ids it
/// created, and the id of `target`'s parent.
fn seed_ancestors(
    lib: &Library,
    watched: &WatchedFolder,
    target: &Path,
    scan_id: i64,
) -> Result<(HashMap<PathBuf, i64>, Option<i64>)> {
    let root = Path::new(&watched.path);
    let root_str = root
        .to_str()
        .ok_or_else(|| crate::Error::NonUtf8Path(root.to_path_buf()))?;
    let mut ids = HashMap::new();
    let mut parent = Some(lib.upsert_folder(watched.id, None, root_str, scan_id)?);
    ids.insert(root.to_path_buf(), parent.expect("just inserted"));

    let relative = target.strip_prefix(root).unwrap_or(Path::new(""));
    let mut components: Vec<_> = relative.components().collect();
    components.pop(); // `target` itself is upserted by the walk.
    let mut current = root.to_path_buf();
    for component in components {
        current = current.join(component);
        let current_str = current
            .to_str()
            .ok_or_else(|| crate::Error::NonUtf8Path(current.clone()))?;
        let id = lib.upsert_folder(watched.id, parent, current_str, scan_id)?;
        ids.insert(current.clone(), id);
        parent = Some(id);
    }
    Ok((ids, parent))
}
```

- [ ] **Step 4: Run the tests and commit**

Run: `cargo test -p photon-core && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, including every pre-existing scanner test.

```bash
git add crates/photon-core/src/scanner.rs
git commit -m "feat(core): scan a single subtree without touching the rest of the library"
```

---

### Task 4: Watcher policy (pure)

**Files:**
- Create: `crates/photon-core/src/watcher/mod.rs`, `crates/photon-core/src/watcher/policy.rs`
- Modify: `crates/photon-core/src/lib.rs` (add `pub mod watcher;`)

**Interfaces:**
- Consumes: `crate::paths::is_within`.
- Produces:
  - `watcher::WatchedRoot { pub watched_id: i64, pub path: PathBuf }`
  - `watcher::plan_scans(dirs: &[PathBuf], roots: &[WatchedRoot], excluded: &[PathBuf]) -> Vec<(i64, PathBuf)>` — maps changed directories to subtree scan requests. It drops anything excluded or outside every root, collapses a directory into an ancestor that is also present for the same root, removes duplicates, and returns a deterministic order (by watched id, then path).

- [ ] **Step 1: Write the failing tests**

`crates/photon-core/src/watcher/policy.rs`, tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<WatchedRoot> {
        vec![
            WatchedRoot { watched_id: 1, path: PathBuf::from("/photos") },
            WatchedRoot { watched_id: 2, path: PathBuf::from("/other") },
        ]
    }

    #[test]
    fn maps_directories_to_their_watched_folder() {
        let dirs = [PathBuf::from("/photos/a"), PathBuf::from("/other/b")];
        assert_eq!(
            plan_scans(&dirs, &roots(), &[]),
            vec![(1, PathBuf::from("/photos/a")), (2, PathBuf::from("/other/b"))]
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
            vec![(1, PathBuf::from("/photos/a")), (1, PathBuf::from("/photos/b"))]
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
            vec![(1, PathBuf::from("/photos/a")), (2, PathBuf::from("/other/b"))]
        );
        assert!(plan_scans(&[], &roots(), &[]).is_empty());
    }

    #[test]
    fn a_changed_root_itself_is_a_valid_request() {
        let dirs = [PathBuf::from("/photos")];
        assert_eq!(plan_scans(&dirs, &roots(), &[]), vec![(1, PathBuf::from("/photos"))]);
    }
}
```

- [ ] **Step 2: Run and confirm failure, then implement**

Run: `cargo test -p photon-core policy` — expected: the module doesn't exist.

`crates/photon-core/src/watcher/mod.rs`:

```rust
//! Watching watched folders for changes.
//!
//! The policy — which directories become scan requests — is pure and lives in [`policy`].
//! The `notify`-backed part lives in [`fs`] and is deliberately thin.

mod fs;
mod policy;

pub use fs::{WatchError, Watcher};
pub use policy::{WatchedRoot, plan_scans};
```

(`fs` arrives in Task 5; until then, declare only `policy` and its re-export.)

`crates/photon-core/src/watcher/policy.rs`, above the tests:

```rust
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
    // descendants, and sorting put every ancestor before the directories beneath it.
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
```

- [ ] **Step 3: Run the tests and commit**

Run: `cargo test -p photon-core && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/photon-core
git commit -m "feat(core): add the watcher's pure event-to-scan policy"
```

---

### Task 5: The notify-backed watcher

**Files:**
- Create: `crates/photon-core/src/watcher/fs.rs`
- Modify: `crates/photon-core/src/watcher/mod.rs`, `crates/photon-core/Cargo.toml`

**Interfaces:**
- Consumes: `notify` 8.2 and `notify-debouncer-full` 0.7.
- Produces:
  - `watcher::WatchError { pub path: PathBuf, pub message: String }`
  - `watcher::Watcher`, with:
    - `Watcher::start(debounce: Duration) -> Result<(Watcher, Receiver<Vec<PathBuf>>)>`
    - `watch_root(&mut self, path: &Path) -> std::result::Result<(), WatchError>` — recursive
    - `unwatch_root(&mut self, path: &Path)`
  - Each message on the channel is a batch of **directories** that changed: a file event is reported as its parent directory, so the policy in Task 4 stays pure.

- [ ] **Step 1: Add the dependencies and write the watcher**

In `crates/photon-core/Cargo.toml`:

```toml
notify = "8.2.0"
notify-debouncer-full = "0.7.0"
```

`crates/photon-core/src/watcher/fs.rs`:

```rust
//! The `notify`-backed half of the watcher: register roots, debounce, and report the
//! directories that changed. Everything policy-shaped lives in `policy.rs`, which is pure.

use notify::{RecursiveMode, Watcher as _};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
    time::Duration,
};

/// A root that could not be watched, for example because the OS ran out of watch
/// descriptors. The caller falls back to periodic rescans for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchError {
    pub path: PathBuf,
    pub message: String,
}

pub struct Watcher {
    debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

impl Watcher {
    /// Starts debouncing. Each message on the returned channel is a batch of directories
    /// that changed; a file event is reported as its parent directory.
    pub fn start(debounce: Duration) -> crate::Result<(Self, Receiver<Vec<PathBuf>>)> {
        let (tx, rx) = channel::<Vec<PathBuf>>();
        let debouncer = new_debouncer(debounce, None, move |result: DebounceEventResult| {
            match result {
                Ok(events) => {
                    let mut dirs: Vec<PathBuf> = Vec::new();
                    for event in events {
                        for path in &event.paths {
                            let dir = if path.is_dir() {
                                path.clone()
                            } else {
                                match path.parent() {
                                    Some(parent) => parent.to_path_buf(),
                                    None => continue,
                                }
                            };
                            if !dirs.contains(&dir) {
                                dirs.push(dir);
                            }
                        }
                    }
                    if !dirs.is_empty() {
                        let _ = tx.send(dirs);
                    }
                }
                Err(errors) => {
                    for error in errors {
                        tracing::warn!(%error, "filesystem watch error");
                    }
                }
            }
        })
        .map_err(|err| std::io::Error::other(err.to_string()))?;
        Ok((Self { debouncer }, rx))
    }

    /// Watches `path` and everything beneath it. A failure is returned rather than
    /// propagated, so one unwatchable root never stops the others.
    pub fn watch_root(&mut self, path: &Path) -> std::result::Result<(), WatchError> {
        self.debouncer
            .watch(path, RecursiveMode::Recursive)
            .map_err(|err| WatchError {
                path: path.to_path_buf(),
                message: err.to_string(),
            })
    }

    pub fn unwatch_root(&mut self, path: &Path) {
        if let Err(err) = self.debouncer.unwatch(path) {
            tracing::debug!(%err, ?path, "could not unwatch");
        }
    }
}
```

The exact constructor and method names in `notify-debouncer-full` 0.7 may differ (for example whether `watch` lives on the debouncer or on `debouncer.watcher()`). Adapt minimally to the installed API, keep this module's own interface exactly as the Interfaces block states, and record what you changed.

- [ ] **Step 2: One real-filesystem test, excluded from CI**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Real filesystem events are timing-dependent, so this is excluded from CI.
    /// Run it locally with: cargo test -p photon-core -- --ignored watcher_reports
    #[test]
    #[ignore]
    fn watcher_reports_the_directory_a_new_file_landed_in() {
        let dir = tempfile::tempdir().unwrap();
        let (mut watcher, rx) = Watcher::start(Duration::from_millis(200)).unwrap();
        watcher.watch_root(dir.path()).unwrap();

        std::fs::write(dir.path().join("new.jpg"), b"x").unwrap();

        let batch = rx.recv_timeout(Duration::from_secs(5)).expect("no event arrived");
        let canonical = dunce::canonicalize(dir.path()).unwrap();
        assert!(batch.iter().any(|d| dunce::canonicalize(d).unwrap() == canonical));
    }
}
```

- [ ] **Step 3: Verify and commit**

Run: `cargo test -p photon-core && cargo test -p photon-core -- --ignored watcher_reports && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: the normal suite passes, and the ignored test passes when run explicitly. If the ignored test is flaky on your machine, say so in the report rather than loosening it.

```bash
git add crates/photon-core Cargo.lock
git commit -m "feat(core): add the notify-backed filesystem watcher"
```

---

### Task 6: Subtree scans in the engine, and the watcher service

**Files:**
- Modify: `crates/photon-app/src/engine.rs`
- Create: `crates/photon-app/src/watch.rs`
- Modify: `crates/photon-app/src/lib.rs` (add `pub mod watch;`)

**Interfaces:**
- Consumes: `Engine`'s existing scan manager, `scan_subtree`, `plan_scans`, `Watcher`.
- Produces:
  - `Engine::start_subtree_scan(self: &Arc<Self>, watched: WatchedFolder, dir: PathBuf) -> bool` — occupies the same per-folder slot as `start_scan`, so it returns false when that folder is already scanning or the engine is shutting down.
  - `watch::WatcherService`, with `start(engine: &Arc<Engine>) -> WatcherService` and `stop(&self)`, plus three methods the ticker thread and the tests both use: `handle_batch(&self, dirs: Vec<PathBuf>)`, `drain_pending(&self)` and `pending_len(&self) -> usize`.
  - A degraded root is rescanned every 5 minutes; offline roots are polled every 30 seconds; pending follow-ups are drained every 2 seconds.

- [ ] **Step 1: Generalise the engine's scan spawn**

In `engine.rs`, rename the body of `start_scan` into a private `start_scan_inner(self: &Arc<Self>, watched: WatchedFolder, subtree: Option<PathBuf>) -> bool`, keeping the `scans` lock, the token, `RemoveOnDrop` and `shutting_down` exactly as they are. The thread calls `run_scan(&watched, subtree.clone(), cancel)`. Then:

```rust
    /// Starts a full background scan unless one is already running for this folder.
    pub fn start_scan(self: &Arc<Self>, watched: WatchedFolder) -> bool {
        self.start_scan_inner(watched, None)
    }

    /// Starts a scan of one directory beneath `watched`. It takes the same per-folder slot
    /// as a full scan, so a folder never has two scans running, and the watcher's work is
    /// cancelled by `remove_folder` and `shutdown` exactly like a manual rescan.
    pub fn start_subtree_scan(self: &Arc<Self>, watched: WatchedFolder, dir: PathBuf) -> bool {
        self.start_scan_inner(watched, Some(dir))
    }
```

`run_scan` gains the `subtree: Option<PathBuf>` parameter and chooses the scanner, leaving its throttling, refresh and event code untouched:

```rust
        let result = match &subtree {
            Some(dir) => scan_subtree(&self.lib, watched, dir, now_ms(), &options, &mut on_progress),
            None => scan_watched(&self.lib, watched, now_ms(), &options, &mut on_progress),
        };
```

(Extract the existing progress closure into `on_progress` so both arms share it.)

- [ ] **Step 2: Write the service's failing tests**

`crates/photon-app/src/watch.rs`, tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, jpeg};

    #[test]
    fn an_event_in_a_watched_folder_scans_that_subtree() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);

        std::fs::write(f.photos.join("a").join("two.jpg"), &img).unwrap();
        service.handle_batch(vec![f.photos.join("a")]);
        f.engine.wait_for_scans();

        assert_eq!(f.engine.grid().1.len(), 2);
        assert_eq!(watched.id, f.engine.lib.watched_folders().unwrap()[0].id);
        service.stop();
    }

    #[test]
    fn events_inside_photons_own_directories_are_ignored() {
        let f = fixture(&[]);
        f.add_photos();
        let service = WatcherService::start(&f.engine);
        let before = f.engine.grid().0;

        service.handle_batch(vec![f.engine.excluded()[0].clone()]);
        f.engine.wait_for_scans();

        assert_eq!(f.engine.grid().0, before, "no scan, so no new grid version");
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    #[test]
    fn an_event_during_a_running_scan_becomes_a_pending_follow_up() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);

        // Occupy the folder's scan slot, then deliver an event for it.
        let blocker = f.engine.clone();
        assert!(blocker.start_scan(watched.clone()));
        service.handle_batch(vec![f.photos.join("a")]);
        assert_eq!(service.pending_len(), 1);

        f.engine.wait_for_scans();
        service.drain_pending();
        f.engine.wait_for_scans();
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    #[test]
    fn an_unknown_directory_is_dropped() {
        let f = fixture(&[]);
        f.add_photos();
        let service = WatcherService::start(&f.engine);
        service.handle_batch(vec![f.dir.path().join("elsewhere")]);
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }
}
```

- [ ] **Step 3: Implement the service**

```rust
//! Turns filesystem events into subtree scans.
//!
//! The watcher itself and the mapping policy live in photon-core; this owns the lifecycle:
//! which roots are watched, what happens when the OS won't watch them, and how events
//! become scans without ever running two scans of one folder at once.

use crate::engine::Engine;
use parking_lot::Mutex;
use photon_core::{
    library::WatchedFolder,
    watcher::{WatchedRoot, Watcher, plan_scans},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

const DEBOUNCE: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_secs(2);
const OFFLINE_POLL: Duration = Duration::from_secs(30);
const DEGRADED_RESCAN: Duration = Duration::from_secs(300);

pub struct WatcherService {
    engine: Arc<Engine>,
    /// At most one queued follow-up per watched folder: the shallowest directory wins,
    /// because scanning it covers the others.
    pending: Mutex<HashMap<i64, PathBuf>>,
    /// Roots the OS would not let us watch; rescanned periodically instead.
    degraded: Mutex<Vec<i64>>,
    stopping: Arc<AtomicBool>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}
```

Then:

- `start` registers every online watched root, records failures in `degraded` (logging once per root), spawns the event thread (receive a batch → `handle_batch`) and the ticker thread (every `TICK`: drain pending; every `OFFLINE_POLL`: rescan offline roots that are back; every `DEGRADED_RESCAN`: full-rescan degraded roots).
- `handle_batch(dirs)` calls `plan_scans(&dirs, &roots, self.engine.excluded())`, then for each request either starts a subtree scan or records a pending follow-up when `start_subtree_scan` returns false, keeping the shallower of the two directories.
- `drain_pending` retries each pending request, removing the ones that start.
- `stop` sets `stopping`, drops the watcher and joins the threads.
- Registration failures also surface through `FolderStatus { degraded: true }` (Task 7).

- [ ] **Step 4: Verify and commit**

Run: `cargo test -p photon-app && cargo test --workspace && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Run the watch tests 5 times; they start real scan threads.

```bash
git add crates/photon-app
git commit -m "feat(app): turn filesystem events into subtree scans"
```

---

### Task 7: Wiring, the degraded notice, and docs

**Files:**
- Modify: `crates/photon-app/src/app.rs`, `crates/photon-app/src/events.rs`, `crates/photon-app/src/engine.rs`
- Modify: `ui/src/lib/api.ts`, `ui/src/lib/library.svelte.ts`, `ui/src/components/StatusBar.svelte`
- Modify: `README.md`

**Interfaces:**
- Produces:
  - `FolderStatus` gains `pub degraded: bool` (serialised as `degraded`), so the existing `folder-status` event carries watch health; no new event or command.
  - The status bar shows "Live updates limited" while any watched folder is degraded.

- [ ] **Step 1: Carry `degraded` through**

- `events.rs`: add the field; update every construction (the scan path sets `degraded: false` unless the service says otherwise) and the recorder tests.
- `engine.rs`: `WatcherService` is started after `startup`'s first scans and stopped in `shutdown`, before the thumbnail queue closes. Adding or removing a folder re-registers its watch.
- `api.ts`: `FolderStatus` gains `degraded: boolean`.
- `library.svelte.ts`: track degraded watched ids from `folder-status`, exposing `anyDegraded`.
- `StatusBar.svelte`: when `library.anyDegraded` is true, show `Live updates limited — photon will re-check these folders periodically.`

- [ ] **Step 2: README**

Add to the smoke checklist:

```markdown
- [ ] Copying a photo into a watched folder makes it appear in the grid within a few seconds, with no manual rescan.
- [ ] Deleting a photo on disk removes it from the grid.
- [ ] Renaming a folder on disk moves its photos in the tree within a few seconds.
- [ ] Unplugging a watched drive dims it; plugging it back in restores it within about a minute, unattended.
- [ ] On a library large enough to exhaust the system's watch limit, the status bar says live updates are limited rather than silently missing changes.
```

Add a short "How watching works" section: photon watches each folder recursively, waits 2 seconds for changes to settle, then rescans just the directories that changed; when the OS won't allow a watch it falls back to a rescan every 5 minutes.

- [ ] **Step 3: Full verification**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
npm ci && npm run check && npm test && npm run -w ui build
```

Expected: all clean. Do not launch the app.

```bash
git add crates ui README.md
git commit -m "feat: start the watcher with the app and surface degraded watching"
```

---

## Spec coverage

| Spec section | Task |
|---|---|
| §2.1 subtree scan, restricted four ways; folder-tree membership; vanished directory | 1, 2, 3 |
| §2.2 watcher module, 2s debounce, thin notify seam, per-root registration failures | 4, 5 |
| §3 WatcherService, collisions, degraded roots, offline polling, no new IPC | 6, 7 |
| §4 error handling (limits, excluded paths, files mid-write, renames, dead watcher) | 3, 5, 6 |
| §5 testing (subtree safety, pure policy, fake event source, real-FS test out of CI) | 1, 3, 4, 5, 6 |
| §6 success criteria | README checklist (7) |
