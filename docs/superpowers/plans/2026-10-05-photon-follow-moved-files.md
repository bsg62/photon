# Follow a moved or renamed photo - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A photo renamed or moved inside watched folders keeps its library row, and with it its albums, user keywords, edit, hidden flag, detected faces and hashes.

**Architecture:** When the scanner is about to insert a file as new (`walk_tree`'s `None` arm, flushed by `flush_new`), it asks the library for rows with the same size, mtime and kind, lets a pure matching function pick the one whose file is gone, and re-points that row with a new `Library::move_items` instead of inserting. `mark_missing`/`purge` become path-guarded so no scan can mark or purge a row that has just moved. Cached thumbnails are renamed to the new key through a new `ScanSink::moved` hook.

**Tech Stack:** Rust, rusqlite (SQLite), photon-core's scanner and library modules, photon-app's `Engine`.

**Spec:** `docs/superpowers/specs/2026-10-05-photon-follow-moved-files-design.md` - read it first; it has the reasons this plan does not repeat.

## Global Constraints

- Read `CLAUDE.md` before starting. Its rules bind every task.
- **The Rust gate before every commit:** `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **Every new test is shown to fail with its change reverted** (an exact replacement, not a compile error), and the commit message says so. A probe that passes is a finding: report it, and write the case that makes the reverted code differ.
- photon never writes to, moves or deletes photo files. This feature only reads (`stat`) and writes the library and photon's own thumbnail cache.
- No new dependency. No UI change, no IPC command, no setting.
- Schema goes 25 -> 26. `library/mod.rs` asserts the literal version twice; update the numbers, do not loosen them to `MIGRATIONS.len()`.
- Never launch the GUI.
- Comments carry the reasoning, in the surrounding code's density and voice. No em dashes in comments or docs; the codebase uses " - ".
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Do not push; the controller does.

## Review Focus

Inputs the spec implies and a person is likely to hit; each has its test in the task named.

1. **A folder renamed while photon was closed** (every file of it is "new" in one `scan_watched`): every row follows, none is marked missing by that same walk. Task 3, `a_renamed_folder_keeps_every_row`.
2. **The destination is scanned before the source** (the watcher's order is arbitrary): the row is live, not missing, when it is re-pointed. Task 3, `a_move_is_followed_when_only_the_destination_is_scanned`.
3. **An unplugged drive whose photos were copied into another watched folder**: the copies are new rows; the drive's rows keep everything. Task 2 (closure) and Task 3, `an_offline_or_empty_root_gives_no_candidates`.
4. **Two scans race for one row** (the source's scan purges it, or a second destination claims it, between the lookup and the write): the file is inserted as new, never dropped. Task 1, `move_items_reports_a_row_that_is_no_longer_at_its_old_path`, and Task 3, `a_file_whose_row_went_away_is_inserted`.
5. **A photo moved out of a folder the user hid**: it stays hidden. Task 1, `a_moved_row_keeps_its_own_hidden_flag_and_takes_a_hidden_folders`.

## File Structure

- `crates/photon-core/src/library/schema.rs` - migration 26.
- `crates/photon-core/src/library/items.rs` - `MoveCandidate`, `move_candidates`, `move_items`, `mark_missing_at`, `purge_at`.
- `crates/photon-core/src/library/folders.rs` - `inherit_folder_flags`.
- `crates/photon-core/src/moved.rs` (new) - the matching rule and the filesystem probe. One responsibility: "which row, if any, is this new file".
- `crates/photon-core/src/scanner.rs` - `flush_new` uses it; `ScanReport::moved`; `ScanSink::moved`; guarded mark and purge.
- `crates/photon-core/src/thumbs/cache.rs` - `ThumbCache::rename`.
- `crates/photon-app/src/engine.rs` - `ScanReporter::moved`.
- `README.md`, `CLAUDE.md`, `docs/smoke-checklist.md`.

---

### Task 1: Schema 26 and the library's side of a move

**Files:**
- Modify: `crates/photon-core/src/library/schema.rs` (append a migration; add its test)
- Modify: `crates/photon-core/src/library/mod.rs` (the two literal `25`s)
- Modify: `crates/photon-core/src/library/items.rs`
- Modify: `crates/photon-core/src/library/folders.rs`
- Modify: `crates/photon-core/src/library/settings.rs` (the GC tripwire test only)

**Interfaces:**
- Consumes: `NewItem`, `edit_from_db`, `settings::bump_thumb_gc_epoch`, `write_tags`, `Library::set_folder_hidden` (all existing).
- Produces:

```rust
// library/items.rs
/// A row that a new file may be the moved file of (`crate::moved`).
#[derive(Clone, Debug, PartialEq)]
pub struct MoveCandidate {
    pub id: i64,
    pub path: String,
    pub file_name: String,
    pub folder_id: i64,
    /// The folder's own path, for "is the old directory gone".
    pub folder_path: String,
    pub watched_id: i64,
    pub width: u32,
    pub height: u32,
    pub taken_at: i64,
    pub exif_version: i64,
    pub edit: Edit,
}

impl Library {
    /// Every row, live or missing, with this size, mtime and kind. Served by `items_moved`.
    pub fn move_candidates(&self, size: i64, mtime_ms: i64, kind: MediaKind) -> Result<Vec<MoveCandidate>>;

    /// Re-points each row from `old_path` to the file `NewItem` describes. Returns, per input
    /// in order, whether the row was moved; `false` means it is no longer at `old_path`
    /// (purged or claimed since the lookup) and the caller must insert the file instead.
    pub fn move_items(&self, moves: &[(i64, String, NewItem)]) -> Result<Vec<bool>>;

    /// `mark_missing`, for ids read before a walk: only a row still at `path` is marked.
    /// Returns how many were.
    pub fn mark_missing_at(&self, rows: &[(i64, String)], now_ms: i64) -> Result<u64>;

    /// `purge_items`, guarded the same way. Returns how many rows were deleted.
    pub fn purge_at(&self, rows: &[(i64, String)]) -> Result<u64>;
}

// library/folders.rs
impl Library {
    /// Gives folder `to` the alias and Hide-folder flag of `from`, when `to` has neither.
    /// Returns whether anything was written.
    pub fn inherit_folder_flags(&self, from: i64, to: i64) -> Result<bool>;
}
```

- [ ] **Step 1: The migration and its tripwires.** Append to `MIGRATIONS` in `schema.rs`:

```rust
    // 26: the lookup a scan makes for each new file, to find the row it used to be
    // (`Library::move_candidates`). Not partial, unlike `items_size`: a row a previous scan
    // marked missing is the likeliest candidate of all.
    r#"
CREATE INDEX items_moved ON items(size, mtime_ms);
"#,
```

Run `cargo test -p photon-core --lib library::` and fix exactly the failures that assert the old version: the two literals in `library/mod.rs` (`open_creates_schema_and_is_idempotent`, `refuses_newer_schema`) become 26, and each `migration_NN_...` test in `schema.rs` that asserts the final `user_version` follows the pattern its neighbours use. Add `migration_26_adds_the_move_index_to_a_populated_library`, modelled on `migration_22_adds_the_view_indexes_and_reshapes_the_starred_one`: seed from `MIGRATIONS[..25]`, insert a folder and an item, migrate, assert `user_version == 26`, the item is still there, and `SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'items_moved'` returns a row.

- [ ] **Step 2: Write the failing tests** in `items.rs`'s test module (use `temp_library`, `seed_folder`, `new_item` from `testutil`):

  - `move_candidates_finds_live_and_missing_rows_by_size_time_and_kind`: three rows in one folder - `a.jpg` (size 100, mtime 1000), `b.jpg` with `size: 101`, `c.mp4` with `kind: MediaKind::Video` and the same size and mtime as `a`; mark `a` missing with `mark_missing`. `move_candidates(100, 1_000, MediaKind::Image)` returns exactly `a`, with its `path`, `file_name`, `folder_id`, `folder_path == "/p"`, `watched_id`, `width`, `height`, `taken_at`, and `exif_version == EXIF_VERSION`.
  - `the_move_lookup_is_served_by_its_index`: `EXPLAIN QUERY PLAN` of the lookup's SQL (a `const MOVE_CANDIDATES_SQL` shared with the method) contains `items_moved`. Model it on `the_recent_view_is_served_by_its_index`.
  - `move_items_repoints_the_row_and_keeps_what_hangs_on_it`: one row with a tag in `tags`, then: add it to an album (`create_album`, `add_to_album`), `add_item_tag(id, "kept")`, `set_item_edit(id, Edit::new(1, None).unwrap())`, `set_items_hidden` (check its exact name and signature in `library/hidden.rs`), `set_similar_groups(&[(id, id)])`, mark it missing, and set `thumb_state` Ready via the existing setter the thumbnail tests use. Then `move_items(&[(id, "/p/a.jpg".into(), NewItem { path: "/p/sub/b.jpg".into(), file_name: "b.jpg".into(), folder_id: sub, ..same })])` where `sub` is a second folder from `upsert_folder`. Assert the returned vec is `[true]`; `lib.item(id)` has the new `path` and `folder_id`, `missing_since == None`, `thumb_state == ThumbState::Pending`, the same `edit`, `hidden == true`; the album still lists the id; `item_tags(id)` still holds both the file's tag and `"kept"`; `similar_groups()` still holds it; and `known_items(watched)` has the new path and not the old.
  - `move_items_reports_a_row_that_is_no_longer_at_its_old_path`: `move_items(&[(id, "/p/elsewhere.jpg".into(), item)])` returns `[false]` and leaves the row untouched; a purged id returns `[false]` too.
  - `a_moved_row_keeps_its_own_hidden_flag_and_takes_a_hidden_folders`: (a) a hidden row moved into a visible folder stays hidden; (b) a visible row moved into a folder hidden with `set_folder_hidden(folder, true)` becomes hidden.
  - `a_move_brings_a_stale_row_up_to_date`: `forget_metadata_for_test(id)`, then `move_items` with a `NewItem` carrying `camera.iso = Some(400)`: afterwards the row's `camera.iso` is `Some(400)` and `known_items[...].exif_version == EXIF_VERSION`.
  - `the_guarded_mark_and_purge_change_only_a_row_still_at_its_path`: two rows; `mark_missing_at(&[(a, "/p/a.jpg".into()), (b, "/p/WRONG.jpg".into())], 5)` returns 1, `a` is missing and `b` is not; `purge_at` with the same shape returns 1 and only `a` is gone.
  - In `settings.rs`, extend the tripwire test (the one that walks purge, replace, an edit, removing a watched folder): after the edit block, `settle(&lib)`, `lib.move_items(&[(ids[1], "/p/b.jpg".into(), new_item(folder, "/p/c.jpg", 2))])`, and assert `thumb_gc_due(...)` is `Some`, message `"a move"`.
  - In `folders.rs` tests, `a_folder_inherits_an_alias_and_the_hide_flag_only_when_it_has_neither`: `from` has alias "Holiday" and is hidden; `to` is plain and holds one visible item. `inherit_folder_flags(from, to)` returns `true`; `to` now has the alias, `hidden == true`, and its item is hidden. A second call returns `false`. A `to` that already has its own alias keeps it and is not hidden by the call.

- [ ] **Step 3: Run them and watch them fail.** `cargo test -p photon-core --lib move_ guarded_mark inherits_an_alias thumb_gc` - compile errors first; stub the five methods (`todo!()` is fine for a moment) until each test fails on an assertion or the `todo!()` panic, not on a missing symbol.

- [ ] **Step 4: Implement.**

```rust
// items.rs
const MOVE_CANDIDATES_SQL: &str =
    "SELECT i.id, i.path, i.file_name, i.folder_id, f.path, f.watched_id, i.width, i.height,
            i.taken_at, i.exif_version, i.edit_turns, i.edit_crop
     FROM items i JOIN folders f ON f.id = i.folder_id
     WHERE i.size = ?1 AND i.mtime_ms = ?2 AND i.kind = ?3";
```

`move_items`, one transaction, `bump_thumb_gc_epoch(&tx)?` first (the path is part of the thumbnail key), then per move:

```sql
UPDATE items SET folder_id = ?3, path = ?4, file_name = ?5,
       make = ?6, model = ?7, lens = ?8, focal_mm = ?9, aperture = ?10, exposure_s = ?11,
       iso = ?12, exif_version = ?13, taken_at = ?14, caption = ?15, gps_lat = ?16, gps_lon = ?17,
       thumb_state = 0, thumb_error = NULL, missing_since = NULL,
       hidden = MAX(hidden, coalesce((SELECT hidden FROM folders WHERE id = ?3), 0))
 WHERE id = ?1 AND path = ?2
```

When `execute` returns 1, call `write_tags(&tx, id, &item.tags)?` and push `true`; otherwise push `false`. It must not touch `content_hash`, `percep_hash`, `similar_group`, `face_version`, `edit_*`, `rating`, `picasa_hidden`, `size`, `mtime_ms`, `width`, `height`, `orientation`, `kind`, `duration_ms`, nor delete from `detected_faces`. Its doc comment says why it is not `update_items` (the content did not change, so nothing derived from the picture is cleared) and names the spec.

`mark_missing_at`: `UPDATE items SET missing_since = ?3 WHERE id = ?1 AND path = ?2 AND missing_since IS NULL`, summing `execute`'s counts. `purge_at`: `bump_thumb_gc_epoch`, then `DELETE FROM items WHERE id = ?1 AND path = ?2`, summing. Keep `mark_missing` and `purge_items` as they are (tests and benches in three crates call them by id); add to their docs that the scanner, whose ids come from a list read before its walk, uses the guarded forms.

`inherit_folder_flags`: read both folders' `(alias, hidden)`; if `to` has an alias or is hidden, or `from` has neither, return `Ok(false)`; write `to`'s alias if `from` has one (plain `UPDATE folders SET alias = ?2 WHERE id = ?1` - the value was validated when it was set on `from`); if `from` is hidden call `self.set_folder_hidden(to, true)?`; return `Ok(true)`.

- [ ] **Step 5: Run the tests; all pass.** Then the whole gate.

- [ ] **Step 6: Probes.** Revert in turn and confirm a test fails each time: the `AND path = ?2` in `move_items`; the `hidden = MAX(...)` (replace with nothing); `missing_since = NULL`; `thumb_state = 0`; the `bump_thumb_gc_epoch` call in `move_items`; the `AND path = ?2` in each of `mark_missing_at` and `purge_at`; the "`to` has neither" guard in `inherit_folder_flags`. After restoring a file from a copy, `touch` it (cargo otherwise keeps running the probed build).

- [ ] **Step 7: Commit** - `feat(library): schema 26 and the writes that follow a moved photo`.

---

### Task 2: The matching rule

**Files:**
- Create: `crates/photon-core/src/moved.rs`
- Modify: `crates/photon-core/src/lib.rs` (`pub(crate) mod moved;` - check how sibling modules are declared and match it)

**Interfaces:**
- Consumes: `MoveCandidate`, `NewItem`, `metadata::EXIF_VERSION`, `paths::same_path`, `Library::watched_folders`, `Library::move_candidates`.
- Produces:

```rust
/// Which of `candidates` the new file `item` is the moved file of, by the spec's rules 1-5,
/// or `None`. `gone` answers rules 2-4 for one candidate: its file is gone (or is this very
/// file) and its drive is there. `claimed` holds rows another file of this batch took.
pub(crate) fn pick<'c>(
    item: &NewItem,
    candidates: &'c [MoveCandidate],
    claimed: &HashSet<i64>,
    mut gone: impl FnMut(&MoveCandidate) -> bool,
) -> Option<&'c MoveCandidate>;

/// The filesystem's answers to rules 2-4, with each watched folder's "is its drive there"
/// worked out once per scan.
pub(crate) struct Probe { /* watched id -> root path and online flag, loaded lazily; a cache of liveness */ }

impl Probe {
    pub(crate) fn new() -> Self;
    /// Rules 2-4 for `candidate` against the new file at `new_path`.
    pub(crate) fn gone(&mut self, lib: &Library, candidate: &MoveCandidate, new_path: &Path) -> bool;
}

/// Rule 2's reading of a stat: only "no such file" is gone.
pub(crate) fn is_gone(stat: &std::io::Result<std::fs::Metadata>) -> bool;

/// Rule 4 for one root: a directory with at least one entry.
pub(crate) fn root_is_there(root: &Path) -> bool;
```

- [ ] **Step 1: Write the failing tests** in `moved.rs` (`#[cfg(test)] mod tests`). Build candidates with a small `fn candidate(id: i64, path: &str) -> MoveCandidate` whose defaults match `testutil::new_item` (400 by 300, `taken_at` as given, `exif_version: EXIF_VERSION`, `edit: Edit::default()`, `file_name` from the path).

  - `one_gone_candidate_is_the_file`: one candidate, `gone = |_| true` -> `Some(id)`.
  - `a_candidate_whose_file_is_still_there_is_not`: `gone = |_| false` -> `None` (this is "a copy is not a move").
  - `a_different_picture_is_not_the_file`: candidate with `width: 401` -> `None`; with another `taken_at` -> `None`; and with another `taken_at` but `exif_version: EXIF_VERSION - 1` -> `Some` (an older reader may have dated it differently).
  - `a_claimed_row_is_not_offered_twice`: `claimed` holds the id -> `None`.
  - `two_candidates_are_not_guessed_between`: two gone candidates with file names that both differ from the new file's -> `None`.
  - `the_file_name_decides_between_two`: two gone candidates, one named like the new file -> that one; two both named like it -> `None`.
  - `gone_is_asked_only_about_rows_that_could_be_the_file`: a `gone` closure that records the ids it was asked about is never asked about a candidate with different dimensions (a stat is a syscall, possibly on a network share).
  - `only_not_found_is_gone`: `is_gone(&Err(io::Error::from(io::ErrorKind::NotFound)))` is true; `PermissionDenied`, `TimedOut` and `Ok(metadata of a real temp file)` are false.
  - `a_root_is_there_only_as_a_directory_with_something_in_it`: a temp dir with a file -> true; an empty temp dir -> false; a path that does not exist -> false; a path to a file -> false.
  - `the_probe_follows_a_file_that_is_gone_from_a_live_root`, on a real temp library and directory: add a watched folder with one real file in it, seed a row whose path is a file that does not exist under that root -> `gone` is true; delete everything in the root (now empty) and use a fresh `Probe` -> false; `set_watched_online(id, false)` with a non-empty root and a fresh `Probe` -> false; a candidate whose file exists -> false; a candidate whose `path` equals `new_path` exactly, file present -> true (rule 3, the only form of it Linux can show).
  - `the_probe_asks_about_a_root_once`: after one `gone` call, emptying the root does not change the answer of the same `Probe` (it is per scan by design; say so in the test's comment).

- [ ] **Step 2: Run and watch them fail** (`cargo test -p photon-core --lib moved::`), stubbing as in Task 1.

- [ ] **Step 3: Implement.**

```rust
pub(crate) fn pick<'c>(
    item: &NewItem,
    candidates: &'c [MoveCandidate],
    claimed: &HashSet<i64>,
    mut gone: impl FnMut(&MoveCandidate) -> bool,
) -> Option<&'c MoveCandidate> {
    let fits: Vec<&MoveCandidate> = candidates
        .iter()
        .filter(|c| !claimed.contains(&c.id))
        .filter(|c| c.width == item.width && c.height == item.height)
        .filter(|c| c.exif_version < EXIF_VERSION || c.taken_at == item.taken_at)
        .filter(|c| gone(c))
        .collect();
    match fits.as_slice() {
        [only] => Some(*only),
        [] => None,
        several => {
            let mut named = several.iter().filter(|c| c.file_name == item.file_name);
            match (named.next(), named.next()) {
                (Some(one), None) => Some(*one),
                _ => None,
            }
        }
    }
}
```

`Probe::gone`: if `paths::same_path(Path::new(&candidate.path), new_path)` return true (the old spelling is this file; nothing about a drive to ask). Otherwise the candidate's watched folder must be live - looked up in a `HashMap<i64, bool>` filled on first use from `lib.watched_folders()` as `w.online && root_is_there(Path::new(&w.path))`; an id not found, or a failed read, is not live - and then `is_gone(&fs::symlink_metadata(&candidate.path))`. The order matters and gets a comment: the root is asked first because an unplugged drive answers `NotFound` for every file.

The module doc comment states the rule in five lines and points at the spec.

- [ ] **Step 4: Tests pass; the gate passes.**

- [ ] **Step 5: Probes.** Each `filter` in `pick` removed in turn; the name tie-break replaced by `several.first().copied()`; `is_gone` widened to `stat.is_err()`; the emptiness test in `root_is_there`; the `w.online &&`; the root check moved after or dropped from `Probe::gone`.

- [ ] **Step 6: Commit** - `feat(scanner): the rule for which row a new file used to be`.

---

### Task 3: The scanner follows a move

**Files:**
- Modify: `crates/photon-core/src/scanner.rs`

**Interfaces:**
- Consumes: everything Tasks 1 and 2 produce.
- Produces:
  - `ScanReport::moved: u64`, counted in `touched_rows`.
  - `ScanSink::moved(&mut self, _keys: &[(u64, u64)]) {}` - a default no-op: `(old thumbnail key, new thumbnail key)` of each re-pointed row. Task 4 implements it in the engine.
  - `ScanProgress` is unchanged.

- [ ] **Step 1: Write the failing tests** in `scanner.rs`'s test module. Start with two helpers there:

```rust
/// Everything photon keeps on a row, put on `id`, so a test can ask what survived a move.
fn decorate(lib: &Library, id: i64) -> i64 {
    let album = lib.create_album("Kept", 1).unwrap();
    lib.add_to_album(album.id, &[id], 1).unwrap();
    lib.add_item_tag(id, "kept").unwrap();
    lib.set_item_edit(id, crate::edit::Edit::new(1, None).unwrap()).unwrap();
    album.id
}

/// The row `id` is at `path` now and still carries what `decorate` put on it.
fn assert_followed(lib: &Library, id: i64, album: i64, path: &Path) {
    let item = lib.item(id).unwrap().expect("the row is still there");
    assert_eq!(item.path, key(path));
    assert_eq!(item.missing_since, None);
    assert_eq!(item.edit, crate::edit::Edit::new(1, None).unwrap());
    assert!(lib.item_tags(id).unwrap().contains(&"kept".to_string()));
    // Use whichever reader of an album's item ids the album tests in library/albums.rs use.
    assert!(album_ids(lib, album).contains(&id));
}
```

  Also put a detected face on the row and assert it survives, using the seeding the tests in `library/detected_faces.rs` use (`set_face_detection(true)`, `write_face_batch` with a `FaceCandidate` built from the item and one `Detection`, `DETECTOR_VERSION`); read that module's `seeded`/`face` helpers and reuse their shape. If the row's thumbnail must be Ready for that writer, set it the way those tests do.

  Files are real: `write_file(&root, "a.jpg", &jpeg_bytes(8, 6))`, moved with `fs::rename` (which keeps the mtime). Scans alternate ids 1, 2, 3.

  - `a_renamed_file_keeps_its_row`: scan; decorate; `fs::rename(a, root/"b.jpg")`; scan. `assert_followed`; `report.moved == 1`, `report.added == 0`, `report.marked_missing == 0`, `report.purged == 0`; exactly one item in `known_items`.
  - `a_file_moved_to_another_folder_keeps_its_row`: into `root/2024/a.jpg`; the item's `folder_id` is the `2024` folder's.
  - `a_renamed_folder_keeps_every_row`: three files in `root/trip`, one decorated; `fs::rename(root/"trip", root/"holiday")`; scan. Three items, the same three ids, `moved == 3`, nothing marked missing, and after one more scan nothing purged and the `trip` folder row gone.
  - `a_move_is_followed_when_only_the_destination_is_scanned`: move `root/x/a.jpg` to `root/y/a.jpg`; `scan_subtree(&lib, &watched, &root.join("y"), ...)` only. Followed, and the row was never missing. Then `scan_subtree` of `x`: nothing marked missing (`report.marked_missing == 0`) - this is the guarded mark at work, since `x`'s own `known` no longer lists the row, so also assert with a stale list directly: `lib.mark_missing_at(&[(id, key(&old_path))], 5)` returns 0.
  - `a_move_is_followed_after_the_source_was_scanned_first`: move, `scan_subtree` of `x` first (the row is now missing), then of `y`: followed, `missing_since == None`, and the detected face is still there (`update_items`, the old revive path, would have deleted it).
  - `a_move_between_two_watched_folders_keeps_the_row`: two roots, `fs::rename` across them, scan the destination root only.
  - `a_copy_beside_the_original_is_a_new_row`: `fs::copy` then `set_mtime` the copy to the original's mtime (use the module's existing `set_mtime` helper; read the original's mtime from `fs::metadata`). Two rows; the original's id is unchanged and still decorated; `moved == 0`, `added == 1`.
  - `two_rows_that_fit_are_not_guessed_between`: two byte-identical files `a.jpg` and `b.jpg` with the same mtime, both indexed; delete both and write the same bytes with the same mtime as `c.jpg`: a new row (`added == 1`, `moved == 0`). The same but the new file is named `a.jpg` in another folder: `a`'s row follows.
  - `an_offline_or_empty_root_gives_no_candidates`: two roots; root 1 holds `a.jpg`, indexed and decorated. Rename root 1's directory away (so its files answer NotFound), write the same bytes with the same mtime into root 2, scan root 2: `added == 1`, `moved == 0`, and the decorated row is untouched. Repeat with root 1 present but emptied.
  - `a_different_picture_with_the_same_size_and_time_is_not_followed`: index `a.jpg` (8 by 6), delete it, write a file of the same byte length and mtime but other dimensions (`jpeg_bytes(6, 8)`, padded after the end-of-image marker to equal length as CLAUDE.md's fixture note describes): a new row.
  - `a_renamed_folder_keeps_its_name_and_its_hide_flag`: `set_folder_alias(trip, Some("Holiday"))`, `set_folder_hidden(trip, true)`; rename the directory and add one new file to it before the scan. After the scan the new folder row has the alias and `hidden`, the moved photos are hidden, and the added file is hidden too.
  - `a_folder_that_still_exists_gives_nothing_to_the_one_its_photo_moved_to`: aliased folder `x` keeps other photos; one photo moves to new folder `y`: `y` has no alias.
  - `a_photo_moved_without_its_ini_loses_its_picasa_star`: `.picasa.ini` in `x` stars `a.jpg`; move `a.jpg` alone to `y`; scan: the row follows, its `rating` is no longer a star, `restarred == 1`. And `a_folder_moved_with_its_ini_keeps_its_stars`: rename the folder, `restarred == 0`, still starred. Use the INI fixtures the existing star tests in this module use.
  - `a_file_whose_row_went_away_is_inserted`: cannot be raced from a test, so it is pinned one level down: call the new helper that `flush_new` uses to apply decided moves (name it `apply_moves`) with a move whose `old_path` no longer matches the row; it returns that `NewItem` among the ones to insert, and the scan then has a row at the new path.
  - `a_scan_that_only_followed_a_move_touches_rows`: `ScanReport { moved: 1, ..ScanReport::default() }.touched_rows()` is true.
  - `a_move_reports_the_old_and_new_thumbnail_keys`: a `ScanSink` that records `moved` and `indexed`: after a rename it got one pair, `(Item::thumb_key()` before the move, `Item::thumb_key()` after`)` - compute "before" from the item read before the rename, with its edit - and the row's id in `indexed`, and `moved` was called before `indexed`.

- [ ] **Step 2: Run and watch them fail** (`cargo test -p photon-core --lib scanner::`). Most fail on `added == 1` where `moved == 1` is expected; that is the right failure.

- [ ] **Step 3: Implement.**

  1. `ScanReport` gains `moved` (doc: rows re-pointed to a file's new path; like `restarred` it can be the only non-zero field) and `touched_rows` adds it, with its sentence in that doc comment's last paragraph.
  2. `ScanSink::moved`, a default no-op, documented: the pairs are `Item::thumb_key()` before and after, so a consumer can carry cached thumbnails over; called before `indexed` for the same rows.
  3. `walk_tree` creates `let mut probe = moved::Probe::new();` beside its batches and passes it to both `flush_new` calls.
  4. `flush_new`:

```rust
fn flush_new(
    lib: &Library,
    batch: &mut Vec<NewItem>,
    probe: &mut moved::Probe,
    report: &mut ScanReport,
    seen: &mut ScanProgress,
    progress: &mut dyn ScanSink,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let (moves, mut fresh) = find_moves(lib, std::mem::take(batch), probe)?;
    fresh.extend(apply_moves(lib, moves, report, progress)?);
    let ids = lib.insert_items(&fresh)?;
    report.added += fresh.len() as u64;
    seen.added = report.added;
    progress.indexed(&ids);
    progress.progress(seen);
    Ok(())
}
```

  `find_moves` walks the batch in order with a `claimed: HashSet<i64>`; for each item calls `lib.move_candidates(item.size, item.mtime_ms, item.kind)?` and, only when that is non-empty, `moved::pick(&item, &candidates, &claimed, |c| probe.gone(lib, c, Path::new(&item.path)))`; a hit goes to `moves` as `(MoveCandidate, NewItem)` and its id into `claimed`, a miss to `fresh`.

  `apply_moves`: `lib.move_items(...)`; for each `true`, collect the id, the key pair `(candidate.edit.thumb_key(fingerprint(&candidate.path, item.size, item.mtime_ms)), candidate.edit.thumb_key(fingerprint(&item.path, item.size, item.mtime_ms)))`, and the folder pair `(candidate.folder_id, candidate.folder_path, item.folder_id)`; each `false` returns its `NewItem` to be inserted. Then, for each distinct folder pair whose two ids differ and whose old directory `moved::is_gone(&fs::symlink_metadata(folder_path))`, `lib.inherit_folder_flags(from, to)?` - before the caller inserts `fresh`, so those rows take a hidden folder's flag the usual way. Then `report.moved += n`, `progress.moved(&keys)`, `progress.indexed(&ids)`. Return the un-moved items.

  5. `finish_mark_purge` builds `(id, path)` lists from `known`'s entries and calls `mark_missing_at` / `purge_at`, returning their summed counts as `(marked, purged)`. Update its doc comment: the counts are the rows actually changed, because a row this walk or another scan re-pointed is no longer at the path this walk's list has for it.

- [ ] **Step 4: Tests pass; the gate passes** (the app crate's tests run too: a `ScanReport` literal there may need `..ScanReport::default()`).

- [ ] **Step 5: Probes.** Revert in turn: `find_moves` returning everything as fresh (most tests fail - confirm which do not, and say why each is still worth having); the `claimed` set; the folder inheritance call; its "old directory is gone" condition; its position before the insert (move it after: the hide-flag test's added file must fail); `finish_mark_purge` back to the unguarded `mark_missing` (the same-walk tests must fail with `marked_missing == 1` or a missing row); `moved` out of `touched_rows`; the order of `progress.moved` and `progress.indexed`; the `false` branch of `apply_moves` dropping the item.

- [ ] **Step 6: Commit** - `feat(scanner): re-point the row of a moved or renamed photo`.

---

### Task 4: Thumbnails follow the photo

**Files:**
- Modify: `crates/photon-core/src/thumbs/cache.rs`
- Modify: `crates/photon-app/src/engine.rs`

**Interfaces:**
- Consumes: `ScanSink::moved(&[(u64, u64)])` from Task 3.
- Produces: `ThumbCache::rename(&self, from: u64, to: u64)` - moves each size's cached file from one key to the other; a size with no file under `from` is skipped; a failure is logged with `tracing::warn!` and otherwise ignored.

- [ ] **Step 1: Failing tests.**
  - `cache.rs`, `rename_carries_both_sizes_to_the_new_key`: `generate` (or `store`) under key 1; `rename(1, 2)`; `is_complete(2)` and neither size's `path_for(1, ..)` exists. `rename_of_a_key_with_nothing_cached_does_nothing`: `rename(7, 8)` creates no file and no directory under key 8's shard... assert `!is_complete(8)` and that it does not panic. `rename_carries_a_lone_size`: remove the preview file under key 1 first; after `rename(1, 2)` the grid file is under 2.
  - `engine.rs`, `a_moved_photos_thumbnails_are_carried_to_its_new_key`: with the engine test `Fixture` (read how the scan and thumbnail tests there add a folder, scan, and wait - `wait_for_scans`, `settle`, and how they reach the cache and the library): index one JPEG and wait until its thumbnail is Ready; note `key_before = item.thumb_key()`; rename the file on disk; run a scan and wait for it; the item (same id) has a different `thumb_key()`, the cache `is_complete(new_key)` and has nothing under `key_before`. If the fixture can count renders or decodes, also assert the count did not grow; if it cannot, say so in the commit message rather than adding an assertion that passes either way.

- [ ] **Step 2: Watch them fail.**

- [ ] **Step 3: Implement.** `ThumbCache::rename`: for each `ThumbSize::ALL`, `let (src, dst) = (self.path_for(from, size), self.path_for(to, size));` skip unless `src.is_file()`; `fs::create_dir_all(dst.parent())`, `fs::rename(&src, &dst)`; warn on error. Doc comment: a key names one picture and the picture has not changed, only what it is called; a rename that fails costs one render.

  `ScanReporter::moved` in `engine.rs`: for each pair, the cache's `rename(old, new)` - find how the engine reaches its `ThumbCache` (the protocol handler and `collect_thumb_garbage_if_due` both do) and use the same route. Comment: before `indexed` queues the rows, so the worker finds the files under the new key and marks the row Ready without decoding.

- [ ] **Step 4: Tests pass; the gate passes.**

- [ ] **Step 5: Probes.** `ScanReporter::moved` emptied (the engine test must fail); the `is_file` skip; the `create_dir_all`.

- [ ] **Step 6: Commit** - `feat: carry a moved photo's cached thumbnails to its new key`.

---

### Task 5: Documentation

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `docs/smoke-checklist.md`

No code. One commit: `docs: a moved or renamed photo keeps its albums, edit and names`.

- [ ] **README.** Find each of these and rewrite it to say the photo keeps the thing, in the README's own voice:
  - "an album remembers photos by their library row, so a photo renamed or moved on disk leaves its albums once the old row is purged" (under "Saving a search").
  - "An edit belongs to the file's entry in the library: a photo renamed or moved outside photon comes back unedited." (under "Rotating and cropping").
  - "so a photo renamed or moved outside photon comes back visible, as it loses its albums and edits." (under "Hiding photos").
  - "so a folder renamed or moved outside photon comes back under its directory's name." (the folder alias paragraph).

  Add one short section, `### Renaming and moving photos`, placed before "Linux with an NVIDIA GPU": what follows a photo renamed or moved inside the folders photon watches (albums, keywords added in photon, turns and crops, hidden, the faces photon found and their names; a renamed folder keeps its photon name and its Hide folder flag); how photon recognises it (same size, same modification time, the old file gone - nothing is read); and what does not follow, each in a clause: a photo moved out of the watched folders and back later, a copy whose original is deleted afterwards, a file edited and saved under a new name, and Picasa's stars, faces and albums unless the folder's `.picasa.ini` moved too.

- [ ] **CLAUDE.md.**
  - Conventions, the first bullet: "(both are by item id, so a renamed file leaves its albums and loses its edit when its old row is purged - a recorded limitation, not a bug)" becomes a statement that both are by item id and a renamed or moved file keeps them because the scanner re-points its row (`photon_core::moved`), with the limits (out of the library and back; copy then delete).
  - "What clears what": "A renamed or moved file or folder is a purge and a new row, so its faces lose their confirmations as it loses its albums" becomes the new truth, and why `move_items` is not `update_items`.
  - Schema section: the thumbnail-GC list gains `move_items` (five writers; the tripwire enumerates five). Hidden photos: "Any other writer that creates item rows must inherit the flag the same way" - add that `move_items` does, with `MAX`, so it never un-hides.
  - "The grid index is the spine": the `ScanReport` counters list gains `moved`.
  - A new paragraph under "Scanning, and why "unchanged" matters", in the voice of its neighbours, covering: the rule in one sentence and where it lives (`moved::pick`, `moved::Probe`); that it runs in `flush_new`, so both `walk_tree` callers have it; the guarded `mark_missing_at`/`purge_at` and the two hazards they close (the walk's own stale `known`, a concurrent scan of another root); that `move_items` reports a row no longer at its old path and the scanner then inserts; rule 4 and the unplugged drive; `ScanSink::moved` and the thumbnail rename; `inherit_folder_flags` and its "old directory is gone" condition. State each guard's test by name.

- [ ] **Smoke checklist.** A new section `## Renaming and moving photos` at the end:
  - Put a photo in an album, turn it, add a keyword. Rename the file in the file manager with photon running: within a few seconds the grid shows the new name, and the photo is still in the album, still turned, still tagged. Its thumbnail does not flash to a placeholder.
  - Quit photon, rename a folder that has a photon name, a hidden photo and a named face in it, start photon: the folder keeps its name, the photo is still hidden, the person's view still lists their photo.
  - Move a photo from one watched folder to another: it keeps its album.
  - Copy a photo (keep the original): the copy is a new photo with no album; the original keeps its own.
  - Unplug a drive photon watches, copy some of its photos from a backup into another watched folder: the copies appear as new photos; plug the drive back in and its photos still have their albums.

- [ ] **Self-check:** `grep -n "comes back unedited\|comes back visible\|leaves its albums\|purge and a new row" README.md CLAUDE.md` returns nothing that still states the old behaviour.
