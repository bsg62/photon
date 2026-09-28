# INI-only Pass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the only thing that changed in a folder is its Picasa INI, photon rereads that INI instead of walking the folder, so photon's own star writes and Picasa's album renames never collapse into a full root rescan.

**Architecture:** The watcher classifies each changed directory as a walk or INI-only by the file names that changed in it. INI-only directories go to a new `scanner::refresh_picasa`, which runs the scan's own `apply_picasa` over those folders without walking, under the same per-root scan slot. The watcher keeps an uncapped per-root INI queue beside today's capped walk queue.

**Tech Stack:** Rust (photon-core watcher/scanner/picasa, photon-app engine/watch), notify-debouncer.

**Spec:** `docs/superpowers/specs/2026-09-28-photon-ini-only-pass-design.md`

## Global Constraints

- A path is INI-only when its file name is `.picasa.ini` or `Picasa.ini` (ASCII case-insensitive) or photon's own temporary `<ini name>.photon-*.tmp`; one predicate, `picasa::is_ini_write(name: &str) -> bool`, in `picasa.rs`.
- A folder is INI-only only when **every** changed path in the batch that reduces to it is INI-only; a directory event, any other file, or an unrecognised temporary makes it a walk. A folder in both sets is a walk.
- The INI pass runs under the **same per-root scan slot** as scans; emits no scan-progress events; refreshes the grid through `refresh_grid` only when `touched_rows()`; requests no hashing pass; records no full-scan time; scans `needs_walk` dirs as subtrees right after on the same thread and slot.
- `refresh_picasa` does no mark/purge, no pruning, no thumbnails, no metadata backfill; checks `cancel` between folders; reports `offline` for a missing root as `scan_subtree` does.
- Pending per root: `dirs` keeps `insert_pending` and `MAX_PENDING_DIRS` (8) unchanged; `ini` is uncapped; an INI dir covered by a queued walk is not added; queueing a walk drops the INI dirs it covers; a collapse to the root clears `ini`. Drain: a pending walk first (one per tick), else the whole INI set as one pass.
- Every new test is shown to fail with its rule reverted by an exact replacement (CLAUDE.md).
- Rust gate: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

1. **An INI in a subfolder of an INI folder** (`a/.picasa.ini` and `a/b/.picasa.ini` both change): the pass is not recursive, so both folders must be passed; collapsing the child into the parent as `plan_scans` does for walks would lose `a/b`'s change. Pinned in Task 4 (`plan_ini_keeps_a_nested_ini_folder`).
2. **An INI-only change in a folder photon has never scanned** (a new folder created with its INI): must end indexed by a walk, not ignored. Pinned in Task 3 (`an_unknown_folder_needs_a_walk`) and Task 5 (`an_ini_in_an_unscanned_folder_is_walked`).
3. **A photo and the INI changing in the same debounce batch** (Picasa writing a star while the user copies a photo in): must walk. Pinned in Task 2 (`an_ini_and_a_photo_in_one_folder_is_a_walk`).
4. **An INI pass queued while a walk of an ancestor is also queued**: the walk covers it; no double work, and no INI change lost when the walk later collapses to the root. Pinned in Task 4 (`queueing_a_walk_drops_the_ini_dirs_it_covers`, `a_collapse_to_the_root_clears_the_ini_set`).
5. **The pass racing a full scan of the same root**: the slot must refuse the pass and queue it. Pinned in Task 5 (`an_ini_batch_while_the_slot_is_held_is_queued_uncapped`).

---

## File Structure

- `crates/photon-core/src/picasa.rs`: `is_ini_write`, beside the temp-name writer.
- `crates/photon-core/src/watcher/fs.rs`: `Changed { dirs, ini_dirs }`; `changed_dirs` classifies; the channel carries `Changed`.
- `crates/photon-core/src/watcher/policy.rs`: `plan_ini`; `Pending` (the per-root queue) with `queue_walk` / `queue_ini`.
- `crates/photon-core/src/watcher/mod.rs`: exports.
- `crates/photon-core/src/scanner.rs`: `IniEvidence::none`, `IniPass`, `refresh_picasa`.
- `crates/photon-app/src/engine.rs`: `ScanKind`, `start_ini_pass`, `run_ini_pass`.
- `crates/photon-app/src/watch.rs`: `Changed` from the channel, `Pending` in the map, `handle_changes`, drain order.
- `CLAUDE.md`: the scanning section.

---

### Task 1: `picasa::is_ini_write`

**Files:**
- Modify: `crates/photon-core/src/picasa.rs` (near `NEW_INI`/`OLD_INI` ~line 27 and the temp-name code ~line 650)

**Interfaces:**
- Produces: `pub fn is_ini_write(name: &str) -> bool`.

- [ ] **Step 1: Write the failing test** (in `picasa.rs`'s tests module)

```rust
    #[test]
    fn an_ini_write_is_either_ini_name_or_photon_s_own_temporary() {
        for name in [
            ".picasa.ini",
            ".Picasa.INI",
            "Picasa.ini",
            "picasa.ini",
            ".picasa.ini.photon-4242-7.tmp",
            "Picasa.ini.photon-1-0.tmp",
        ] {
            assert!(is_ini_write(name), "{name}");
        }
        for name in [
            "a.jpg",
            "picasa.ini.bak",
            ".picasa.ini.tmp",
            ".picasa.ini.photon-1-0",
            ".picasa.ini.other-1-0.tmp",
            "x.picasa.ini",
            "notes.txt.photon-1-0.tmp",
            ".picasa.ini.photon-x-0.tmp",
        ] {
            assert!(!is_ini_write(name), "{name}");
        }
    }
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p photon-core --lib an_ini_write_is`
Expected: FAIL to compile, `cannot find function is_ini_write`.

- [ ] **Step 3: Implement** (read the temp-name code first: `format!("{base}.photon-{}-{seq}.tmp", ...)`)

```rust
/// Whether a file named `name` is one an INI write touches: either INI name, in any ASCII
/// case as the reader matches it, or the temporary photon writes one through
/// (`<ini name>.photon-<pid>-<seq>.tmp`, see `write_atomically`). Beside the writer that
/// makes those names so the two cannot drift; the watcher reads a folder whose only changes
/// are these as an INI change, rereading the INI instead of walking the folder.
pub fn is_ini_write(name: &str) -> bool {
    let is_ini = |n: &str| n.eq_ignore_ascii_case(NEW_INI) || n.eq_ignore_ascii_case(OLD_INI);
    if is_ini(name) {
        return true;
    }
    let Some(rest) = name.strip_suffix(".tmp") else {
        return false;
    };
    let Some(at) = rest.find(".photon-") else {
        return false;
    };
    let (base, tail) = (&rest[..at], &rest[at + ".photon-".len()..]);
    is_ini(base)
        && tail
            .split_once('-')
            .is_some_and(|(pid, seq)| {
                !pid.is_empty()
                    && !seq.is_empty()
                    && pid.bytes().all(|b| b.is_ascii_digit())
                    && seq.bytes().all(|b| b.is_ascii_digit())
            })
}
```

Check that `write_atomically`'s `format!` really produces exactly `{base}.photon-{pid}-{seq}.tmp` with `base` the INI's file name; if it differs, match the writer, update the test's names, and ledger it.

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p photon-core --lib an_ini_write_is` — PASS.

- [ ] **Step 5: Revert probes**

- `return true;` (after `if is_ini(name) {`) → `return false;` — expect the test to fail on `.picasa.ini`.
- `is_ini(base)` → `true` — expect it to fail on `notes.txt.photon-1-0.tmp`.
- `pid.bytes().all(|b| b.is_ascii_digit())` → `true` — expect it to fail on `.picasa.ini.photon-x-0.tmp`.

- [ ] **Step 6: Gate, commit**

```bash
git add crates/photon-core/src/picasa.rs
git commit -m "feat(picasa): is_ini_write names the files an INI write touches

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: The watcher classifies INI-only folders

**Files:**
- Modify: `crates/photon-core/src/watcher/fs.rs` (`Watcher::start` ~39-60, `changed_dirs` ~145-200, tests)
- Modify: `crates/photon-core/src/watcher/mod.rs` (export `Changed`)
- Modify: `crates/photon-app/src/watch.rs` (the receiver type, ~lines 294, 305, 398) — temporarily treats `ini_dirs` as walks so nothing is lost until Task 5

**Interfaces:**
- Consumes: `picasa::is_ini_write`.
- Produces: `#[derive(Debug, Default, PartialEq, Eq)] pub struct Changed { pub dirs: Vec<PathBuf>, pub ini_dirs: Vec<PathBuf> }` (in `watcher::fs`, re-exported as `photon_core::watcher::Changed`); `changed_dirs(events) -> (Changed, Vec<WatchError>)`; `Watcher::start` returns `Receiver<Changed>` in place of `Receiver<Vec<PathBuf>>`.

- [ ] **Step 1: Write the failing tests** (fs.rs tests)

```rust
    fn event(kind: EventKind, paths: &[&str]) -> DebouncedEvent {
        let mut e = notify::event::Event::new(kind);
        for p in paths {
            e = e.add_path(PathBuf::from(p));
        }
        DebouncedEvent::new(e, std::time::Instant::now())
    }

    fn modify() -> EventKind {
        EventKind::Modify(notify::event::ModifyKind::Data(notify::event::DataChange::Any))
    }

    /// photon's star write: its temporary is created and written, then renamed over the INI.
    #[test]
    fn photon_s_star_write_is_ini_only() {
        use notify::event::{CreateKind, ModifyKind, RenameMode};
        let dir = "/nonexistent-photon-test/a";
        let tmp = format!("{dir}/.picasa.ini.photon-7-1.tmp");
        let ini = format!("{dir}/.picasa.ini");
        let (changed, _) = changed_dirs(&[
            event(EventKind::Create(CreateKind::File), &[&tmp]),
            event(modify(), &[&tmp]),
            event(EventKind::Modify(ModifyKind::Name(RenameMode::Both)), &[&tmp, &ini]),
        ]);
        assert!(changed.dirs.is_empty(), "{changed:?}");
        assert_eq!(changed.ini_dirs, [PathBuf::from(dir)]);
    }

    /// Review Focus 3.
    #[test]
    fn an_ini_and_a_photo_in_one_folder_is_a_walk() {
        let (changed, _) = changed_dirs(&[
            event(modify(), &["/nonexistent-photon-test/a/.picasa.ini"]),
            event(modify(), &["/nonexistent-photon-test/a/b.jpg"]),
        ]);
        assert_eq!(changed.dirs, [PathBuf::from("/nonexistent-photon-test/a")]);
        assert!(changed.ini_dirs.is_empty());
    }

    #[test]
    fn a_deleted_ini_is_ini_only() {
        let (changed, _) = changed_dirs(&[event(
            EventKind::Remove(notify::event::RemoveKind::File),
            &["/nonexistent-photon-test/a/Picasa.ini"],
        )]);
        assert_eq!(changed.ini_dirs, [PathBuf::from("/nonexistent-photon-test/a")]);
        assert!(changed.dirs.is_empty());
    }

    #[test]
    fn an_unrecognised_temporary_beside_the_ini_is_a_walk() {
        let (changed, _) = changed_dirs(&[
            event(modify(), &["/nonexistent-photon-test/a/.picasa.ini"]),
            event(modify(), &["/nonexistent-photon-test/a/.picasa.ini~"]),
        ]);
        assert_eq!(changed.dirs, [PathBuf::from("/nonexistent-photon-test/a")]);
        assert!(changed.ini_dirs.is_empty());
    }

    /// A path that is a directory is always a walk, even if it were named like an INI.
    #[test]
    fn a_directory_event_is_a_walk() {
        let dir = tempfile::tempdir().unwrap();
        let named = dir.path().join(".picasa.ini");
        std::fs::create_dir(&named).unwrap();
        let (changed, _) = changed_dirs(&[event(modify(), &[named.to_str().unwrap()])]);
        assert_eq!(changed.dirs, [named]);
        assert!(changed.ini_dirs.is_empty());
    }
```

Also change the existing `a_lost_events_notice_is_reported_as_a_watch_failure` to read `changed.dirs` / `changed.ini_dirs` (both empty), and the event-driven tests that receive from `rx` to read `.dirs` from the received `Changed` (check each: they assert on a directory list).

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-core --lib watcher::fs`
Expected: FAIL to compile (`changed.dirs`: no field on tuple/Vec).

- [ ] **Step 3: Implement**

The type, above `Watcher`:

```rust
/// One debounced batch of changes, by directory. `dirs` are walked (a subtree scan);
/// `ini_dirs` changed only in their Picasa INI (`picasa::is_ini_write`) and are reread
/// without a walk. A directory is in one or the other, never both.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changed {
    pub dirs: Vec<PathBuf>,
    pub ini_dirs: Vec<PathBuf>,
}

impl Changed {
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty() && self.ini_dirs.is_empty()
    }
}
```

`Watcher::start`: `channel::<Changed>()`, return type `Receiver<Changed>`, and in the handler
`let (changed, lost) = changed_dirs(&events); if !changed.is_empty() { let _ = tx.send(changed); }`.
Update the doc comment's first bullet ("Each message on the first returned channel is a batch
of directories that changed…") to say it is a `Changed`.

`changed_dirs` becomes (keep the existing comments on `seen`, `is_dir`, and the rescan branch):

```rust
fn changed_dirs(events: &[DebouncedEvent]) -> (Changed, Vec<WatchError>) {
    let mut seen: HashSet<&Path> = HashSet::new();
    // Per directory: whether every path seen into it so far was an INI write.
    let mut kinds: HashMap<PathBuf, bool> = HashMap::new();
    let mut order: Vec<PathBuf> = Vec::new();
    let mut lost: Vec<WatchError> = Vec::new();
    for event in events {
        // (the need_rescan branch, unchanged)
        if !may_have_changed(&event.kind) {
            continue;
        }
        for path in &event.paths {
            if !seen.insert(path.as_path()) {
                continue;
            }
            let (dir, ini) = if path.is_dir() {
                (path.clone(), false)
            } else {
                match path.parent() {
                    Some(parent) => (
                        parent.to_path_buf(),
                        path.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(crate::picasa::is_ini_write),
                    ),
                    None => continue,
                }
            };
            match kinds.entry(dir) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    *e.get_mut() &= ini;
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    order.push(e.key().clone());
                    e.insert(ini);
                }
            }
        }
    }
    let mut changed = Changed::default();
    for dir in order {
        if kinds[&dir] {
            changed.ini_dirs.push(dir);
        } else {
            changed.dirs.push(dir);
        }
    }
    (changed, lost)
}
```

Add a paragraph to its doc comment: a directory whose every changed path is an INI write is reported in `ini_dirs`, reread rather than walked; anything else into it - another file, a directory, a temporary photon does not recognise - makes it a walk, so the fast path only takes what it recognises.

`watcher/mod.rs`: `pub use fs::{Changed, WatchError, Watcher};`

`photon-app/src/watch.rs`: every `Receiver<Vec<PathBuf>>` becomes `Receiver<Changed>` (import `photon_core::watcher::Changed`), and the event loop's `Ok(dirs) => plan_and_apply(&engine, &pending, dirs)` becomes, until Task 5:

```rust
                    // Walked for now; Task 5 of the INI-only plan routes `ini_dirs`.
                    Ok(changed) => plan_and_apply(
                        &engine,
                        &pending,
                        changed.dirs.into_iter().chain(changed.ini_dirs).collect(),
                    ),
```

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p photon-core --lib watcher` and `cargo test -p photon-app` — PASS.

- [ ] **Step 5: Revert probes**

- `*e.get_mut() &= ini;` → `` (empty) — expect `an_ini_and_a_photo_in_one_folder_is_a_walk` and `an_unrecognised_temporary…` to fail (the first path's kind wins).
- `(path.clone(), false)` → `(path.clone(), true)` — expect `a_directory_event_is_a_walk` to fail.
- `.is_some_and(crate::picasa::is_ini_write)` → `.is_some_and(|n| n.eq_ignore_ascii_case(".picasa.ini"))` — expect `photon_s_star_write_is_ini_only` and `a_deleted_ini_is_ini_only` to fail.

- [ ] **Step 6: Gate, commit**

```bash
git add crates/photon-core/src/watcher crates/photon-app/src/watch.rs
git commit -m "feat(watcher): a folder whose only change is its INI is reported apart

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `scanner::refresh_picasa`

**Files:**
- Modify: `crates/photon-core/src/scanner.rs` (`IniEvidence` impl ~535, a new function beside `scan_subtree` ~286, tests)

**Interfaces:**
- Produces: `#[derive(Debug, Default)] pub struct IniPass { pub report: ScanReport, pub needs_walk: Vec<PathBuf> }` and `pub fn refresh_picasa(lib: &Library, watched: &WatchedFolder, dirs: &[PathBuf], cancel: &AtomicBool) -> Result<IniPass>`.

- [ ] **Step 1: Write the failing tests** (scanner tests; `scan`, `photos_root`, `write_file`, `jpeg_bytes`, `temp_library` exist there)

```rust
    fn ini_pass(lib: &Library, watched: &WatchedFolder, dirs: &[PathBuf]) -> IniPass {
        refresh_picasa(lib, watched, dirs, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn an_ini_pass_applies_a_star_without_walking() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 0);

        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        // A photo that appears at the same time is a walk's business, not this pass's.
        write_file(&root, "b.jpg", &jpeg_bytes(6, 2));
        let pass = ini_pass(&lib, &watched, std::slice::from_ref(&root));

        assert_eq!(lib.starred_count().unwrap(), 1);
        assert_eq!(pass.report.restarred, 1);
        assert!(pass.report.touched_rows());
        assert_eq!(lib.grid_entries().unwrap().len(), 1, "nothing was walked");
        assert!(pass.needs_walk.is_empty());
    }

    #[test]
    fn an_ini_pass_over_a_deleted_ini_clears_the_stars() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        assert_eq!(lib.starred_count().unwrap(), 1);

        fs::remove_file(root.join(".picasa.ini")).unwrap();
        ini_pass(&lib, &watched, std::slice::from_ref(&root));

        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    /// Review Focus 2.
    #[test]
    fn an_unknown_folder_needs_a_walk() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);

        let new = root.join("new");
        write_file(&new, "c.jpg", &jpeg_bytes(4, 2));
        write_file(&new, ".picasa.ini", b"[c.jpg]\nstar=yes\n");
        let pass = ini_pass(&lib, &watched, std::slice::from_ref(&new));

        assert_eq!(pass.needs_walk, [new]);
        assert_eq!(lib.starred_count().unwrap(), 0);
    }

    #[test]
    fn an_ini_pass_over_an_offline_root_reports_offline() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        fs::remove_dir_all(&root).unwrap();

        let pass = ini_pass(&lib, &watched, std::slice::from_ref(&root));

        assert!(pass.report.offline);
    }

    #[test]
    fn an_ini_pass_stops_when_cancelled() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();
        scan(&lib, &watched, 1);
        write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");

        let pass = refresh_picasa(&lib, &watched, std::slice::from_ref(&root), &AtomicBool::new(true)).unwrap();

        assert!(pass.report.cancelled);
        assert_eq!(lib.starred_count().unwrap(), 0);
    }
```

(If `AtomicBool` is not already imported in the tests module, add `use std::sync::atomic::AtomicBool;`.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-core --lib an_ini_pass an_unknown_folder`
Expected: FAIL to compile, `cannot find function refresh_picasa`.

- [ ] **Step 3: Implement**

In `impl IniEvidence`:

```rust
    /// No listing at all: every folder is found by `read_folder`, as a folder whose listing
    /// may have been cut short is. For the INI pass, which lists nothing itself.
    fn none() -> IniEvidence<'static> {
        static EMPTY: std::sync::LazyLock<HashMap<PathBuf, IniListing>> =
            std::sync::LazyLock::new(HashMap::new);
        IniEvidence {
            listed: &EMPTY,
            incomplete: &[],
            whole: false,
        }
    }
```

Beside `scan_subtree`:

```rust
/// What an INI pass did, and the folders it could not do it for.
#[derive(Debug, Default)]
pub struct IniPass {
    pub report: ScanReport,
    /// Folders photon has no row for: never scanned, so a new folder whose INI appeared with
    /// it. A walk's to index, not this pass's.
    pub needs_walk: Vec<PathBuf>,
}

/// Rereads the Picasa INI of each folder in `dirs` and applies it - stars, faces, the hidden
/// flag and albums - without walking anything: for a folder whose only change is its INI
/// (the watcher's `Changed::ini_dirs`), which is what photon's own star writes and a Picasa
/// album rename produce.
///
/// Through the scan's own `apply_picasa`, so it applies exactly the scan's rules. What a scan
/// does besides is left out on purpose - no mark or purge, no pruning, no thumbnails, no
/// metadata - since nothing but the INI changed; that is what makes photon's own echo cost
/// one INI read per folder.
pub fn refresh_picasa(
    lib: &Library,
    watched: &WatchedFolder,
    dirs: &[PathBuf],
    cancel: &AtomicBool,
) -> Result<IniPass> {
    let root = Path::new(&watched.path);
    if !root.is_dir() {
        lib.set_watched_online(watched.id, false)?;
        return Ok(IniPass {
            report: ScanReport {
                offline: true,
                ..ScanReport::default()
            },
            ..IniPass::default()
        });
    }
    let rows = FolderRows::load(lib, watched.id)?;
    let mut pass = IniPass::default();
    let mut walked: Vec<(PathBuf, i64)> = Vec::new();
    for dir in dirs {
        if !crate::paths::is_within(dir, root) {
            continue;
        }
        match dir.to_str().and_then(|d| rows.stored.get(d)) {
            Some(&(id, _)) => walked.push((dir.clone(), id)),
            None => pass.needs_walk.push(dir.clone()),
        }
    }
    // In chunks so a cancel (a folder's removal, a quit) is honoured between folders
    // without paying `apply_picasa`'s per-call setup once per folder.
    for chunk in walked.chunks(64) {
        if cancel.load(Ordering::Relaxed) {
            pass.report.cancelled = true;
            break;
        }
        let applied = apply_picasa(lib, chunk, &IniEvidence::none());
        pass.report.restarred += applied.restarred;
        pass.report.refaced += applied.refaced;
        pass.report.rehidden += applied.rehidden;
        pass.report.realbumed += applied.realbumed;
    }
    Ok(pass)
}
```

Check `FolderRows.stored`'s key (the stored path string) and value shape against its definition, and `ScanReport`'s field names (`cancelled`, `offline`); adjust to what is there.

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p photon-core --lib an_ini_pass an_unknown_folder` — PASS (5).

- [ ] **Step 5: Revert probes**

- `None => pass.needs_walk.push(dir.clone()),` → `None => {}` — expect `an_unknown_folder_needs_a_walk` to fail.
- `if !root.is_dir() {` → `if false {` — expect the offline test to fail.
- `pass.report.restarred += applied.restarred;` → `` — expect `an_ini_pass_applies_a_star_without_walking` to fail (touched_rows false).
- `if cancel.load(Ordering::Relaxed) {` (in `refresh_picasa`) → `if false {` — expect the cancel test to fail.

- [ ] **Step 6: Gate, commit**

```bash
git add crates/photon-core/src/scanner.rs
git commit -m "feat(scanner): refresh_picasa rereads INIs without walking

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The pending queue and INI planning (pure policy)

**Files:**
- Modify: `crates/photon-core/src/watcher/policy.rs`, `crates/photon-core/src/watcher/mod.rs`

**Interfaces:**
- Consumes: `insert_pending` (unchanged).
- Produces:
  - `pub fn plan_ini(dirs: &[PathBuf], roots: &[WatchedRoot], excluded: &[PathBuf]) -> Vec<(i64, PathBuf)>` — drops excluded and outside-every-root dirs, dedups, sorts; does **not** collapse descendants (the pass is not recursive).
  - `#[derive(Clone, Debug, Default, PartialEq, Eq)] pub struct Pending { pub dirs: Vec<PathBuf>, pub ini: BTreeSet<PathBuf> }` with `pub fn queue_walk(&mut self, dir: &Path, root: &Path)`, `pub fn queue_ini(&mut self, dir: &Path)`, `pub fn is_empty(&self) -> bool`, `pub fn len(&self) -> usize`.

- [ ] **Step 1: Write the failing tests** (policy.rs tests; `roots()`-style helpers exist there — reuse them, or build `WatchedRoot { watched_id: 1, path: PathBuf::from("/p") }`)

```rust
    /// Review Focus 1.
    #[test]
    fn plan_ini_keeps_a_nested_ini_folder() {
        let roots = [WatchedRoot { watched_id: 1, path: PathBuf::from("/p") }];
        let planned = plan_ini(
            &[PathBuf::from("/p/a/b"), PathBuf::from("/p/a"), PathBuf::from("/p/a")],
            &roots,
            &[],
        );
        assert_eq!(planned, [(1, PathBuf::from("/p/a")), (1, PathBuf::from("/p/a/b"))]);
    }

    #[test]
    fn plan_ini_drops_excluded_and_unwatched_folders() {
        let roots = [WatchedRoot { watched_id: 1, path: PathBuf::from("/p") }];
        let planned = plan_ini(
            &[PathBuf::from("/p/x/a"), PathBuf::from("/elsewhere"), PathBuf::from("/p/b")],
            &roots,
            &[PathBuf::from("/p/x")],
        );
        assert_eq!(planned, [(1, PathBuf::from("/p/b"))]);
    }

    #[test]
    fn the_ini_queue_has_no_cap() {
        let mut pending = Pending::default();
        for i in 0..50 {
            pending.queue_ini(&PathBuf::from(format!("/p/{i}")));
        }
        assert_eq!(pending.ini.len(), 50);
        assert!(pending.dirs.is_empty());
    }

    #[test]
    fn an_ini_dir_under_a_queued_walk_is_not_added() {
        let mut pending = Pending::default();
        pending.queue_walk(Path::new("/p/a"), Path::new("/p"));
        pending.queue_ini(Path::new("/p/a/b"));
        assert!(pending.ini.is_empty());
    }

    /// Review Focus 4.
    #[test]
    fn queueing_a_walk_drops_the_ini_dirs_it_covers() {
        let mut pending = Pending::default();
        pending.queue_ini(Path::new("/p/a/b"));
        pending.queue_ini(Path::new("/p/c"));
        pending.queue_walk(Path::new("/p/a"), Path::new("/p"));
        assert_eq!(pending.ini.iter().collect::<Vec<_>>(), [Path::new("/p/c")]);
    }

    /// Review Focus 4.
    #[test]
    fn a_collapse_to_the_root_clears_the_ini_set() {
        let mut pending = Pending::default();
        pending.queue_ini(Path::new("/p/z"));
        for i in 0..=MAX_PENDING_DIRS {
            pending.queue_walk(&PathBuf::from(format!("/p/{i}")), Path::new("/p"));
        }
        assert_eq!(pending.dirs, [PathBuf::from("/p")]);
        assert!(pending.ini.is_empty());
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p photon-core --lib watcher::policy`
Expected: FAIL to compile (`plan_ini`, `Pending` not found).

- [ ] **Step 3: Implement** (policy.rs; `use std::collections::BTreeSet;`)

```rust
/// Turns folders whose only change is their Picasa INI into INI-pass requests: drops anything
/// excluded or outside every watched root, removes duplicates, and returns a deterministic
/// order. Unlike `plan_scans` it keeps a folder beneath another: the pass rereads one INI per
/// folder and does not descend, so a parent's pass says nothing about its child's INI.
pub fn plan_ini(
    dirs: &[PathBuf],
    roots: &[WatchedRoot],
    excluded: &[PathBuf],
) -> Vec<(i64, PathBuf)> {
    let mut requests: Vec<(i64, PathBuf)> = dirs
        .iter()
        .filter(|dir| !excluded.iter().any(|x| is_within(dir, x)))
        .filter_map(|dir| {
            roots
                .iter()
                .find(|r| is_within(dir, &r.path))
                .map(|r| (r.watched_id, dir.clone()))
        })
        .collect();
    requests.sort();
    requests.dedup();
    requests
}

/// What is still waiting for one watched folder while its scan slot is busy: directories to
/// walk (`insert_pending`'s rules and cap) and folders whose INI to reread. The INI set has
/// no cap - a pass over it costs one INI read per folder, and it is bounded by the folder
/// count - so an INI-only burst (photon starring across many folders, Picasa renaming an
/// album) never collapses into a rescan of the root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    pub dirs: Vec<PathBuf>,
    pub ini: BTreeSet<PathBuf>,
}

impl Pending {
    /// Queues a walk of `dir`. A walk rereads the INI of every folder it covers, so those
    /// leave the INI set; a collapse to `root` covers them all.
    pub fn queue_walk(&mut self, dir: &Path, root: &Path) {
        insert_pending(&mut self.dirs, dir, root);
        let dirs = &self.dirs;
        self.ini.retain(|ini| !dirs.iter().any(|walk| is_within(ini, walk)));
    }

    /// Queues an INI pass over `dir`, unless a queued walk already covers it.
    pub fn queue_ini(&mut self, dir: &Path) {
        if self.dirs.iter().any(|walk| is_within(dir, walk)) {
            return;
        }
        self.ini.insert(dir.to_path_buf());
    }

    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty() && self.ini.is_empty()
    }

    pub fn len(&self) -> usize {
        self.dirs.len() + self.ini.len()
    }
}
```

`mod.rs`: `pub use policy::{MAX_PENDING_DIRS, Pending, WatchedRoot, insert_pending, plan_ini, plan_scans, roots_affected_by};`

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p photon-core --lib watcher::policy` — PASS.

- [ ] **Step 5: Revert probes**

- in `queue_walk`, the `self.ini.retain(...)` line → `` — expect `queueing_a_walk_drops…` and `a_collapse_to_the_root…` to fail.
- in `queue_ini`, `if self.dirs.iter().any(|walk| is_within(dir, walk)) {` → `if false {` — expect `an_ini_dir_under_a_queued_walk…` to fail.
- in `plan_ini`, add after `requests.dedup();` the `plan_scans` ancestor collapse (copy its `kept` loop) — expect `plan_ini_keeps_a_nested_ini_folder` to fail.

- [ ] **Step 6: Gate, commit**

```bash
git add crates/photon-core/src/watcher
git commit -m "feat(watcher): plan_ini and a per-root pending queue with an uncapped INI set

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The engine runs INI passes; the watcher routes them

**Files:**
- Modify: `crates/photon-app/src/engine.rs` (`start_scan`/`start_subtree_scan`/`start_scan_inner` ~1487-1545; a new `run_ini_pass`; tests)
- Modify: `crates/photon-app/src/watch.rs` (the `pending` map type everywhere, `plan_and_apply`, `queue_pending`, `try_drain`, `start_queued_scan`, `pending_len`, `pending_dirs`, the event loop; tests)

**Interfaces:**
- Consumes: `refresh_picasa`, `IniPass` (Task 3); `Changed` (Task 2); `Pending`, `plan_ini` (Task 4).
- Produces: `Engine::start_ini_pass(self: &Arc<Self>, watched: WatchedFolder, dirs: Vec<PathBuf>) -> bool`; `WatcherService::handle_changes(&self, changed: Changed)` (`handle_batch(dirs)` stays, as `handle_changes(Changed { dirs, ini_dirs: vec![] })`); test helper `pending_ini(&self, id) -> Vec<PathBuf>`.

- [ ] **Step 1: Engine — write the failing test** (engine.rs tests)

```rust
    #[test]
    fn an_ini_pass_applies_the_ini_under_the_scan_slot() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        let dir = PathBuf::from(&watched.path).join("a");
        photon_core::picasa::set_star(&dir, "one.jpg", true).unwrap();

        let slot = f.engine.occupy_scan_slot_for_test(watched.id);
        assert!(!f.engine.start_ini_pass(watched.clone(), vec![dir.clone()]), "the slot is taken");
        drop(slot);
        assert!(f.engine.start_ini_pass(watched, vec![dir]));
        f.settle();

        assert_eq!(f.engine.counts().starred, 1);
    }
```

Run: `cargo test -p photon-app --lib an_ini_pass_applies` — FAIL to compile (`start_ini_pass`).

- [ ] **Step 2: Engine — implement**

```rust
/// What a scan thread does with its slot.
enum ScanKind {
    Full,
    Subtree(PathBuf),
    /// Reread these folders' INIs (`photon_core::scanner::refresh_picasa`).
    Ini(Vec<PathBuf>),
}
```

`start_scan` → `self.start_scan_inner(watched, ScanKind::Full)`; `start_subtree_scan` → `ScanKind::Subtree(dir)`; `start_scan_inner(…, kind: ScanKind)`; in the thread:

```rust
                match kind {
                    ScanKind::Full => engine.run_scan(&watched, None, thread_cancel),
                    ScanKind::Subtree(dir) => engine.run_scan(&watched, Some(dir), thread_cancel),
                    ScanKind::Ini(dirs) => engine.run_ini_pass(&watched, dirs, thread_cancel),
                }
```

New public method beside `start_subtree_scan`:

```rust
    /// Rereads the Picasa INI of each of `dirs` beneath `watched` without walking them: for
    /// folders whose only change is their INI. Under the same per-folder slot as a scan, so
    /// it never interleaves with a scan's own read of the same INIs - where the older read
    /// could land last.
    pub fn start_ini_pass(self: &Arc<Self>, watched: WatchedFolder, dirs: Vec<PathBuf>) -> bool {
        self.start_scan_inner(watched, ScanKind::Ini(dirs))
    }
```

And:

```rust
    /// An INI pass. It emits no scan progress - nothing is being scanned from the user's point
    /// of view - requests no hashing pass (no file was read), and records no full-scan time.
    /// Folders photon has never scanned are walked afterwards, in this same slot.
    fn run_ini_pass(self: &Arc<Self>, watched: &WatchedFolder, dirs: Vec<PathBuf>, cancel: Arc<AtomicBool>) {
        let pass = match photon_core::scanner::refresh_picasa(&self.lib, watched, &dirs, &cancel) {
            Ok(pass) => pass,
            Err(err) => {
                tracing::warn!(watched_id = watched.id, %err, "INI pass failed");
                return;
            }
        };
        let went_offline = pass.report.offline && watched.online;
        if (pass.report.touched_rows() || went_offline)
            && let Err(err) = self.refresh_grid()
        {
            tracing::warn!(%err, "grid refresh after an INI pass failed");
        }
        if went_offline
            && let Some(folder) = self
                .lib
                .watched_folders()
                .ok()
                .and_then(|all| all.into_iter().find(|w| w.id == watched.id))
        {
            let degraded = self
                .watcher_service()
                .is_some_and(|service| service.is_degraded(folder.id));
            self.emit_status(&folder, degraded);
        }
        for dir in pass.needs_walk {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            self.run_scan(watched, Some(dir), cancel.clone());
        }
    }
```

(Check `emit_status`'s signature against its use in `run_scan`.) Run the test — PASS.

- [ ] **Step 3: Watcher — write the failing tests** (watch.rs tests; `fixture`, `jpeg`, `occupy_scan_slot_for_test`, `pending_dirs` exist)

```rust
    /// Review Focus 5, and the point of the plan: an INI-only burst never collapses.
    #[test]
    fn an_ini_batch_while_the_slot_is_held_is_queued_uncapped() {
        let img = jpeg(16, 16);
        let files: Vec<(String, &[u8])> =
            (0..20).map(|i| (format!("d{i}/one.jpg"), img.as_slice())).collect();
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), *b)).collect();
        let f = fixture(&refs);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);
        let root = PathBuf::from(&watched.path);
        let dirs: Vec<PathBuf> = (0..20).map(|i| root.join(format!("d{i}"))).collect();
        for dir in &dirs {
            photon_core::picasa::set_star(dir, "one.jpg", true).unwrap();
        }

        let slot = f.engine.occupy_scan_slot_for_test(watched.id);
        service.handle_changes(Changed { dirs: vec![], ini_dirs: dirs.clone() });
        assert_eq!(service.pending_ini(watched.id), dirs.iter().cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>());
        assert_eq!(service.pending_dirs(watched.id).unwrap_or_default(), Vec::<PathBuf>::new(), "nothing collapsed to the root");
        drop(slot);

        service.drain_pending();
        f.settle();
        assert_eq!(f.engine.counts().starred, 20, "one pass applied every INI");
        assert_eq!(service.pending_len(), 0);
        service.stop();
    }

    #[test]
    fn an_ini_folder_inside_a_walk_of_the_same_batch_is_left_to_the_walk() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/b/one.jpg", &img)]);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);
        let root = PathBuf::from(&watched.path);

        let slot = f.engine.occupy_scan_slot_for_test(watched.id);
        service.handle_changes(Changed { dirs: vec![root.join("a")], ini_dirs: vec![root.join("a/b")] });
        assert!(service.pending_ini(watched.id).is_empty());
        assert_eq!(service.pending_dirs(watched.id), Some(vec![root.join("a")]));
        drop(slot);
        service.stop();
    }

    /// Review Focus 2.
    #[test]
    fn an_ini_in_an_unscanned_folder_is_walked() {
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img)]);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);
        let new = PathBuf::from(&watched.path).join("new");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("two.jpg"), &img).unwrap();
        std::fs::write(new.join(".picasa.ini"), b"[two.jpg]\nstar=yes\n").unwrap();

        service.handle_changes(Changed { dirs: vec![], ini_dirs: vec![new] });
        f.settle();

        assert_eq!(f.ids().len(), 2, "the new folder was walked");
        assert_eq!(f.engine.counts().starred, 1);
        service.stop();
    }

    /// The whole point from photon's side: starring across more folders than the walk queue
    /// holds, fed back as the batch those writes' events make, starts no walk.
    #[test]
    fn photon_s_own_stars_across_many_folders_start_no_walk() {
        let img = jpeg(16, 16);
        let files: Vec<(String, &[u8])> =
            (0..12).map(|i| (format!("d{i}/one.jpg"), img.as_slice())).collect();
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), *b)).collect();
        let f = fixture(&refs);
        let watched = f.add_photos();
        let service = WatcherService::start(&f.engine);
        let root = PathBuf::from(&watched.path);

        let slot = f.engine.occupy_scan_slot_for_test(watched.id);
        f.engine.set_stars(&f.ids(), true).unwrap();
        // The directories an INI write reports: each folder, INI-only.
        let ini_dirs: Vec<PathBuf> = (0..12).map(|i| root.join(format!("d{i}"))).collect();
        service.handle_changes(Changed { dirs: vec![], ini_dirs });

        assert_eq!(service.pending_dirs(watched.id).unwrap_or_default(), Vec::<PathBuf>::new());
        assert_eq!(service.pending_ini(watched.id).len(), 12);
        drop(slot);
        service.drain_pending();
        f.settle();
        assert_eq!(f.engine.counts().starred, 12, "the pass found the stars photon wrote");
        service.stop();
    }
```

(Adapt `fixture`'s argument type if it is not `&[(&str, &[u8])]`; `set_stars` must not need the slot — it writes INIs and the database directly; if it waits on the slot, take the slot after it.)

Run: `cargo test -p photon-app --lib watch::tests` — FAIL to compile (`handle_changes`, `pending_ini`).

- [ ] **Step 4: Watcher — implement**

- Import `photon_core::watcher::{Changed, Pending, plan_ini}`.
- Every `Mutex<HashMap<i64, Vec<PathBuf>>>` / `Arc<…>` becomes `…HashMap<i64, Pending>…` (the service field, `ticker_loop`, the event thread, `plan_and_apply`, `queue_pending`, `try_drain`, `start_queued_scan`, `retry_watcher_startup` and anything else the compiler names).
- `pending_len`: `self.pending.lock().values().map(Pending::len).sum()`.
- `pending_dirs` (test helper): `self.pending.lock().get(&id).map(|p| p.dirs.clone()).filter(|d| !d.is_empty())`. New `#[cfg(test)] fn pending_ini(&self, id: i64) -> Vec<PathBuf>` returning the INI set in order (empty if none).
- `queue_pending`: `pending.entry(id).or_default().queue_walk(&dir, root);`
- `start_queued_scan`: remove `dir` from `p.dirs` (and the entry when `p.is_empty()`), as today.
- `handle_batch(dirs)` → `self.handle_changes(Changed { dirs, ini_dirs: Vec::new() })`; new `pub fn handle_changes(&self, changed: Changed) { plan_and_apply(&self.engine, &self.pending, changed) }`; the event loop's `Ok(changed) => plan_and_apply(&engine, &pending, changed)` (remove Task 2's chaining).
- `plan_and_apply(engine, pending, changed: Changed)`: canonicalize both lists as today; walks as today from `plan_scans(&dirs, …)`; then

```rust
    // A walk in this batch rereads the INI of every folder it covers.
    let ini = plan_ini(&ini_dirs, &roots, engine.excluded());
    let mut by_root: HashMap<i64, Vec<PathBuf>> = HashMap::new();
    for (id, dir) in ini {
        if requests.iter().any(|(walk_id, walk)| *walk_id == id && is_within(&dir, walk)) {
            continue;
        }
        by_root.entry(id).or_default().push(dir);
    }
    for (id, dirs) in by_root {
        let Some(folder) = watched.iter().find(|w| w.id == id) else {
            continue;
        };
        if !engine.start_ini_pass(folder.clone(), dirs.clone()) {
            let mut pending = pending.lock();
            let queued = pending.entry(id).or_default();
            for dir in &dirs {
                queued.queue_ini(dir);
            }
        }
    }
```

  (keep the walk requests in a variable `requests` visible to this block; import `photon_core::paths::is_within`.)
- `try_drain`: for each root with a pending entry, if `p.dirs` is non-empty do today's one walk; otherwise take the whole INI set out *before* starting (same reasoning as `start_queued_scan`'s comment) and, if `start_ini_pass` refuses, put each back with `queue_ini`:

```rust
        let ini: Vec<PathBuf> = {
            let mut pending = pending.lock();
            let Some(p) = pending.get_mut(&id) else { continue };
            let ini = std::mem::take(&mut p.ini).into_iter().collect::<Vec<_>>();
            if p.is_empty() { pending.remove(&id); }
            ini
        };
        if !ini.is_empty() && !engine.start_ini_pass(folder.clone(), ini.clone()) {
            let mut pending = pending.lock();
            let queued = pending.entry(id).or_default();
            for dir in &ini { queued.queue_ini(dir); }
        }
```

  Update `try_drain`'s doc comment: a pending walk first (it rereads the INIs it covers), otherwise the whole INI set as one pass.

Run: `cargo test -p photon-app` — PASS (all watch tests, old and new).

- [ ] **Step 5: Revert probes**

- `plan_and_apply`: the `if requests.iter().any(...) { continue; }` → `` — expect `an_ini_folder_inside_a_walk…` to fail.
- `plan_and_apply`: `queued.queue_ini(dir);` → `queued.queue_walk(dir, Path::new(&folder.path));` — expect `an_ini_batch_while_the_slot_is_held…` and `photon_s_own_stars…` to fail (collapse to the root).
- `run_ini_pass`: the `for dir in pass.needs_walk` loop → `` — expect `an_ini_in_an_unscanned_folder_is_walked` to fail.
- `ScanKind::Ini(dirs) => engine.run_ini_pass(…)` → `ScanKind::Ini(_) => {}` — expect the engine test and `an_ini_batch…` to fail.

- [ ] **Step 6: Gate, commit**

```bash
git add crates/photon-app/src/engine.rs crates/photon-app/src/watch.rs
git commit -m "perf(watch): an INI-only change rereads the INI instead of walking

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Docs

**Files:**
- Modify: `CLAUDE.md` (the "Scanning, and why "unchanged" matters" section, after the paragraph about `walk_tree` having two callers)
- Modify: `docs/superpowers/plans/2026-09-28-performance-audit-open-items.md` (section 7's INI-echo item and its "Ideas for that" sub-list)

- [ ] **Step 1: CLAUDE.md** — add after the `walk_tree` two-callers paragraph:

```markdown
`apply_picasa` has a third caller: `refresh_picasa`, the INI pass. The watcher reports a
folder whose only changed files are its INI (`picasa::is_ini_write`, which also knows photon's
own temporary) in `Changed::ini_dirs`, and those are reread without a walk under the same scan
slot, queued in `Pending::ini`, which has no cap - photon starring across many folders and a
Picasa album rename used to overflow the walk queue into a rescan of the whole root. A
rename of the writer's temporary name must change `is_ini_write` with it, or every star
becomes a walk again.
```

- [ ] **Step 2: The audit plan** — replace the "photon's own INI writes echo back as subtree scans" bullet and its "Ideas for that" sub-bullets with: `- ~~**photon's own INI writes echo back as subtree scans.**~~ Done: an INI-only change is reread, not walked (docs/superpowers/specs/2026-09-28-photon-ini-only-pass-design.md); the INI queue has no cap, so neither photon's stars nor a Picasa album rename collapses into a root rescan.`

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md docs/superpowers/plans/2026-09-28-performance-audit-open-items.md
git commit -m "docs: the INI-only pass

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```
