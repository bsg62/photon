# photon Picasa Stars Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stars come from Picasa's per-directory `.picasa.ini` / `Picasa.ini` instead of each photo's embedded XMP.

**Architecture:** A new `photon-core::picasa` module parses one directory's INI into a set of starred file names. A per-folder pass runs **after** the directory walk — not inside `describe()` — and sets `rating` for every photo in each walked folder, because `describe()` only runs for photos whose size or mtime changed and editing a star in Picasa touches only the INI. The XMP reader stays but loses its scan-path caller.

**Tech Stack:** Rust (rusqlite, walkdir), SQLite.

**Spec:** `docs/superpowers/specs/2026-09-13-photon-picasa-stars-design.md`

## Global Constraints

- **photon never writes to, moves or deletes files inside watched folders.** This feature only reads. It never writes, creates or deletes a `Picasa.ini`.
- **No new dependencies.** No INI-parsing crate: the format needed here is a section header and one key, and real Picasa files carry quirks a strict parser would reject. Hand-rolled, in `photon-core`.
- **No schema change.** No column, index or migration. `user_version` stays at 2. `star=yes` becomes `rating = 1`, and `starred` remains `rating >= 1`.
- **Never launch the GUI.** Verification is the test suites; anything needing eyes goes on the README's manual checklist.
- **The Rust gate is four commands, all of which must pass before any commit:**
  `cargo fmt --all --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace` · `cargo bench -p photon-core --bench grid --no-run`
  Run `cargo fmt --all` (not just `--check`) first — rustfmt rejecting a line split has already cost this project a CI rejection.
- **Every task must leave the whole workspace compiling.** `--workspace` is in the gate.
- **Every new test must be demonstrated to fail with its change reverted.** Revert, run, paste the real failure, restore. **A compile error is not a revert-proof** — it shows a symbol was missing, not that an assertion discriminates behaviour. A test that passes both ways proves nothing: say so and replace it.
- **Disclose every deviation from this plan**, including ones you are confident are right. An undisclosed deviation is indistinguishable from an oversight and costs the reviewer the find.

**Three deviations this plan makes deliberately, declared so nobody has to rediscover them:**
`read_stars` returns `Option<HashSet<String>>` where spec §3 says a plain `HashSet` that
"never fails" — the `Option` is what carries §7's distinction between "no stars" and "no
evidence", and §3's wording is the looser of the two. `folder_item_names` uses
`prepare_cached` on a reader where every other read in this codebase uses plain `prepare`;
it runs once per folder per scan, so caching is worth the inconsistency. And Task 2's subtree
test calls `scan_subtree` directly rather than the existing `scan_sub` helper, because it
needs a distinct `scan_id`.

---

### Task 1: The INI parser

**Files:**
- Create: `crates/photon-core/src/picasa.rs`
- Modify: `crates/photon-core/src/lib.rs` (the module list)
- Test: `crates/photon-core/src/picasa.rs` (a `#[cfg(test)] mod tests` in the same file, as `xmp.rs` does)

**Interfaces:**
- Produces: `photon_core::picasa::read_stars(dir: &Path) -> Option<HashSet<String>>`, where the strings are **lowercased file names**. `None` means the evidence could not be read; `Some(empty)` means read successfully with nothing starred.
- Consumes: nothing from other tasks.

**Context you need.** `crates/photon-core/src/lib.rs` is, in full:

```rust
//! photon-core: headless library, scanning and thumbnail engine for photon.

pub mod decode;
pub mod error;
pub mod grid;
pub mod library;
pub mod media;
pub mod metadata;
pub(crate) mod paths;
pub mod scanner;
pub mod thumbs;
pub mod watcher;
pub mod xmp;

#[cfg(test)]
mod testutil;

pub use error::{Error, Result};
```

Add `pub mod picasa;` between `metadata` and `paths`, matching the existing alphabetical convention and `xmp`'s public visibility.

The file format, from the spec:

```ini
[filename.jpg]
star=yes
backuphash=12223
[nextfilename.jpg]
star=yes
backuphash=1332313
```

A section header is a bare filename. Lines under it are that file's properties until the next header. `star` is optional, default `no`. **Every other property is ignored.**

- [ ] **Step 1: Register the module**

Add `pub mod picasa;` to `crates/photon-core/src/lib.rs`, between `metadata` and `paths`.
**Do this first.** Without it `picasa.rs` is never compiled, and Step 3's "expected failure"
would be a vacuous `0 tests run` that reads like success.

- [ ] **Step 2: Write the failing tests**

Create `crates/photon-core/src/picasa.rs` containing the tests below **and nothing else** — no
stub. The first run then fails to compile on the missing `read_stars`, which is an honest
"not built yet" signal rather than a stub quietly returning something. House style: long sentence-like test names, and a comment on any case whose reason is not obvious. `write_file(dir, rel, bytes) -> PathBuf` already exists in `crate::testutil` and creates parent directories.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::write_file;

    /// The starred names in `dir`, sorted, for assertions that do not care about set order.
    fn stars(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = read_stars(dir).unwrap().into_iter().collect();
        v.sort();
        v
    }

    #[test]
    fn reads_a_star_from_a_dotted_ini() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nstar=yes\nbackuphash=12223\n[b.jpg]\nbackuphash=1332313\n",
        );
        assert_eq!(stars(dir.path()), vec!["a.jpg"], "b.jpg has no star key, so it is not starred");
    }

    #[test]
    fn reads_a_star_from_an_undotted_ini() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "Picasa.ini", b"[a.jpg]\nstar=yes\n");
        assert_eq!(stars(dir.path()), vec!["a.jpg"]);
    }

    #[test]
    fn accepts_the_truthy_spellings_and_rejects_everything_else() {
        // Lenient in one direction on purpose: being strict costs a silently missing star,
        // which is the bug this whole feature exists to fix.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"[a.jpg]\nstar=YES\n[b.jpg]\nstar=true\n[c.jpg]\nstar=1\n\
              [d.jpg]\nstar=no\n[e.jpg]\nstar=0\n[f.jpg]\nstar=\n",
        );
        assert_eq!(stars(dir.path()), vec!["a.jpg", "b.jpg", "c.jpg"]);
    }

    #[test]
    fn matches_file_names_case_insensitively() {
        // Picasa came from Windows, where the filesystem is case-insensitive, so an INI
        // written there can disagree in case with the same files read on Linux.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[DSC_0001.JPG]\nstar=yes\n");
        assert_eq!(stars(dir.path()), vec!["dsc_0001.jpg"]);
    }

    #[test]
    fn the_dotted_file_wins_and_the_two_are_never_merged() {
        // This is the test that pins the precedence rule. Merging two disagreeing files
        // would produce a set matching neither, and a star removed in one but left in the
        // other would linger with nothing able to explain it.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[a.jpg]\nstar=yes\n");
        write_file(dir.path(), "Picasa.ini", b"[b.jpg]\nstar=yes\n");
        assert_eq!(stars(dir.path()), vec!["a.jpg"], "b.jpg is starred only in the file that lost");
    }

    #[test]
    fn ignores_properties_that_are_not_star() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[a.jpg]\nbackuphash=12223\nfaces=rect64(abc)\n");
        assert!(stars(dir.path()).is_empty());
    }

    #[test]
    fn a_directory_with_no_ini_reads_as_nothing_starred() {
        // Some(empty), not None: the directory was read and the answer is "no stars", which
        // is what lets the caller clear stars the INI no longer confirms.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_stars(dir.path()), Some(std::collections::HashSet::new()));
    }

    #[test]
    fn an_unreadable_directory_reads_as_no_evidence() {
        // None, not Some(empty): failing to read is not evidence that the stars are gone,
        // so the caller must leave existing stars alone.
        let missing = std::path::Path::new("/definitely/not/a/directory/here");
        assert_eq!(read_stars(missing), None);
    }

    #[test]
    fn survives_a_malformed_file() {
        // Real Picasa files carry stray lines, comments and unterminated headers. A strict
        // parser would reject the file outright and lose every star in the folder.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            ".picasa.ini",
            b"; a comment\n[unterminated\nstar=yes\nstray line with no equals\n\
              [a.jpg]\nstar=yes\n",
        );
        assert_eq!(stars(dir.path()), vec!["a.jpg"], "the unterminated header claims no file");
    }

    #[test]
    fn survives_a_non_utf8_file() {
        // Picasa files from old Windows locales are not necessarily UTF-8. A section header
        // that survives lossy decoding still matches.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[caf\xe9.jpg]\nstar=yes\n[a.jpg]\nstar=yes\n");
        let found = stars(dir.path());
        assert!(found.contains(&"a.jpg".to_string()), "a valid section after invalid bytes is still read");
    }

    #[test]
    fn a_duplicate_section_starring_the_file_wins() {
        // Spec §8 names this case. Picasa has been known to write a file twice; whichever
        // way it resolves must be pinned rather than left to whatever the loop happens to
        // do, since a silent change here would move stars.
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[a.jpg]\nstar=no\n[a.jpg]\nstar=yes\n");
        assert_eq!(stars(dir.path()), vec!["a.jpg"], "a star anywhere in the file counts");

        let dir2 = tempfile::tempdir().unwrap();
        write_file(dir2.path(), ".picasa.ini", b"[a.jpg]\nstar=yes\n[a.jpg]\nstar=no\n");
        assert_eq!(
            stars(dir2.path()),
            vec!["a.jpg"],
            "inserting into a set never un-stars: a later star=no does not remove an earlier star"
        );
    }

    #[test]
    fn the_read_is_bounded() {
        // Spec §3's cap has to actually bound the read rather than being advisory, the same
        // way `xmp::a_packet_beyond_the_read_cap_is_not_found` pins the XMP one.
        let dir = tempfile::tempdir().unwrap();
        let mut ini = vec![b' '; MAX_INI as usize];
        ini.extend_from_slice(b"\n[a.jpg]\nstar=yes\n");
        write_file(dir.path(), ".picasa.ini", &ini);
        assert!(
            stars(dir.path()).is_empty(),
            "a section past the cap is not read"
        );
    }

    #[test]
    fn a_later_section_does_not_inherit_the_previous_one_s_star() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), ".picasa.ini", b"[a.jpg]\nstar=yes\n[b.jpg]\nbackuphash=1\n");
        assert_eq!(stars(dir.path()), vec!["a.jpg"]);
    }
}
```

- [ ] **Step 3: Run them and watch them fail**

Run: `cargo test -p photon-core picasa`
Expected: compile failure — `read_stars` does not exist. That is the checkpoint for this step; it is **not** the revert-proof, which comes in Step 6.

- [ ] **Step 4: Implement**

```rust
//! Reads Picasa's per-directory star flags.
//!
//! Picasa writes one INI per directory — `.picasa.ini` on newer versions, `Picasa.ini` on
//! older ones — with a section per file. photon only ever reads them: nothing here writes,
//! creates or deletes an INI, and photon cannot set a star.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// How much of an INI is read. Picasa writes a few lines per photo, so this is far beyond
/// any real file; it exists so a stray enormous one cannot pull unbounded memory through a
/// scan of a hundred thousand photos.
pub const MAX_INI: u64 = 8 * 1024 * 1024;

/// The starred file names in one directory, lowercased.
///
/// `None` means the evidence could not be read — the directory or its INI is unreadable —
/// and the caller must leave existing stars alone, because failing to read is not evidence
/// that the stars are gone. `Some(empty)` is a real answer: the directory was read and
/// nothing is starred, which is what lets a caller clear stars the INI no longer confirms.
pub fn read_stars(dir: &Path) -> Option<HashSet<String>> {
    let Some(path) = ini_path(dir)? else {
        return Some(HashSet::new());
    };
    let bytes = read_capped(&path)?;
    Some(parse_stars(&String::from_utf8_lossy(&bytes)))
}

/// The INI to read, if any. `Some(None)` when the directory has none; `None` when the
/// directory itself could not be listed.
///
/// Listing the directory rather than probing two fixed names is what makes the match
/// case-insensitive: a library written on Windows and read on Linux may carry any casing,
/// and a missed file means a whole folder silently loses its stars. `.picasa.ini` wins when
/// both exist, and the two are never merged.
fn ini_path(dir: &Path) -> Option<Option<PathBuf>> {
    let mut dotted = None;
    let mut plain = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        match name.to_lowercase().as_str() {
            ".picasa.ini" => dotted = Some(entry.path()),
            "picasa.ini" => plain = Some(entry.path()),
            _ => {}
        }
    }
    Some(dotted.or(plain))
}

fn read_capped(path: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.by_ref().take(MAX_INI).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Walks the INI line by line. Deliberately lenient: an unterminated header, a stray line
/// or a comment is skipped rather than failing the file, because rejecting a file outright
/// would lose every star in that folder.
fn parse_stars(text: &str) -> HashSet<String> {
    let mut stars = HashSet::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            // An unterminated header names no file, so it clears the section rather than
            // letting the keys below it attach to the previous one.
            section = rest.strip_suffix(']').map(|name| name.trim().to_lowercase());
            continue;
        }
        let Some(name) = section.as_ref() else { continue };
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim().eq_ignore_ascii_case("star") && is_star(value.trim()) {
            stars.insert(name.clone());
        }
    }
    stars
}

fn is_star(value: &str) -> bool {
    value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true") || value == "1"
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p photon-core picasa` — all green.

- [ ] **Step 6: Prove the tests discriminate**

Two reverts, each with its real failure pasted into your report, restoring after each:

1. Make `ini_path` prefer `plain.or(dotted)` instead of `dotted.or(plain)`. Expect `the_dotted_file_wins_and_the_two_are_never_merged` to fail.
2. Make `is_star` accept only the exact lowercase `"yes"`. Expect `accepts_the_truthy_spellings_and_rejects_everything_else` to fail.

If either passes anyway, it does not pin the behaviour — say so and fix the test.

- [ ] **Step 7: Run the full Rust gate and commit**

All four commands, then:

```bash
git add crates/photon-core/src/picasa.rs crates/photon-core/src/lib.rs
git commit -m "feat(stars): parse Picasa's per-directory INI star flags"
```

---

### Task 2: The per-folder pass, and its wiring

**Files:**
- Modify: `crates/photon-core/src/library/items.rs` (add two methods near `update_items` ~line 143)
- Modify: `crates/photon-core/src/scanner.rs` (add the pass; wire it into **both** `walk_tree` call sites)
- Test: `crates/photon-core/src/scanner.rs` and `crates/photon-core/src/library/items.rs` test modules

**Interfaces:**
- Consumes: `crate::picasa::read_stars(dir) -> Option<HashSet<String>>` from Task 1.
- Produces: `Library::folder_item_names(folder_id: i64) -> Result<Vec<(i64, String)>>` (lowercased names); `Library::set_ratings(&[(i64, u8)]) -> Result<()>`; `WalkOutcome.walked: Vec<(PathBuf, i64)>`.

**Verify, do not modify:** `crates/photon-app`. Its
`switching_to_the_starred_view_rebuilds_the_grid_with_only_starred_photos` writes a rating
directly and holds a live watcher, so a watcher-triggered subtree scan over a folder with no
INI could reset it. It should stay green because it runs no further scan — but if it goes
flaky, report it rather than editing it.

**Context you need — read this before writing anything.**

**There are TWO callers of `walk_tree`.** `scan_watched` (~line 60) walks a whole watched root; `scan_subtree` (~line 165) walks one directory and is what the file watcher uses when a folder changes. **A pass wired into only the first would silently never run for watcher-triggered scans**, which is the common case for "I just starred something in Picasa". Find both with `rg -n 'walk_tree\(' crates/photon-core/src/scanner.rs` and wire both.

`folder_ids` is owned by the caller, not by `walk_tree`, so it is still in scope after the walk returns. In `scan_watched`:

```rust
let mut known = lib.known_items(watched.id)?;
let mut folder_ids: HashMap<PathBuf, i64> = HashMap::new();

let WalkOutcome { report, seen, incomplete_prefixes, skip_mark_purge, cancelled } = walk_tree(
    lib, watched.id, root, None, &mut known, &mut folder_ids, scan_id, options, progress,
)?;

if cancelled {
    progress(&seen);
    return Ok(ScanReport { cancelled: true, ..report });
}
```

It maps an absolute directory path to that folder's row id — exactly what the pass needs.

**Why the pass cannot live in `describe()`.** `describe()` is called only here:

```rust
match known.remove(path_str) {
    Some(k) if k.size == size && k.mtime_ms == mtime_ms && !k.missing => {
        report.unchanged += 1
    }
    Some(k) => changed_batch.push((k.id, describe(&entry, path_str, folder_id, kind, size, mtime_ms))),
    None => new_batch.push(describe(&entry, path_str, folder_id, kind, size, mtime_ms)),
}
```

Starring a photo in Picasa rewrites the INI and **does not touch the photo**, so on a rescan it takes the `unchanged` branch, `describe()` never runs, and a star read there would never be written.

**There is no `COLLATE NOCASE` anywhere in the schema and `lower()` is not used in any query.** Case-insensitive matching is done in Rust throughout this codebase (see `search_entries`), so the pass reads each folder's item names, lowercases them in Rust, and updates by row id.

`BATCH` is already defined in `scanner.rs` as `500`, and `finish_mark_purge` shows the chunking convention: one call per chunk, each its own transaction.

- [ ] **Step 1: Write the failing tests**

Add to the scanner's test module. Its helpers already exist: `temp_library()`, `photos_root(&dir)`, `write_file(&root, rel, bytes)`, `jpeg_bytes(w, h)`, and `scan(&lib, &watched, scan_id)`.

```rust
#[test]
fn a_star_added_after_indexing_is_picked_up_without_the_photo_changing() {
    // THE test for this feature. Starring in Picasa rewrites the INI and leaves the photo
    // untouched, so the photo takes the scanner's `unchanged` branch and describe() never
    // runs for it. An implementation that reads stars in describe() passes every other test
    // here and fails this one.
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);
    assert_eq!(lib.starred_count().unwrap(), 0);

    write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
    let report = scan(&lib, &watched, 2);

    assert_eq!(report.unchanged, 1, "the photo itself did not change");
    assert_eq!(lib.starred_count().unwrap(), 1);
}

#[test]
fn a_star_removed_from_the_ini_is_cleared_on_the_next_scan() {
    // The INI is the only authority (spec §5): only-ever-adding would leave an un-starred
    // photo stuck with no way to clear it short of deleting the library.
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
    write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);
    assert_eq!(lib.starred_count().unwrap(), 1);

    write_file(&root, ".picasa.ini", b"[a.jpg]\nbackuphash=1\n");
    scan(&lib, &watched, 2);
    assert_eq!(lib.starred_count().unwrap(), 0);
}

#[test]
fn deleting_the_ini_clears_the_folder_s_stars() {
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(&root, "a.jpg", &jpeg_bytes(4, 2));
    write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);
    assert_eq!(lib.starred_count().unwrap(), 1);

    std::fs::remove_file(root.join(".picasa.ini")).unwrap();
    scan(&lib, &watched, 2);
    assert_eq!(lib.starred_count().unwrap(), 0, "a folder with no INI has no stars");
}

#[test]
fn a_parent_folder_s_ini_does_not_star_photos_in_a_subfolder() {
    // Picasa writes one INI per directory and its section names are bare filenames, so
    // nothing is inherited downward.
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
    write_file(&root, ".picasa.ini", b"[a.jpg]\nstar=yes\n");
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);

    assert_eq!(lib.starred_count().unwrap(), 0);
}

#[test]
fn stars_are_matched_case_insensitively_against_the_files_on_disk() {
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(&root, "DSC_0001.JPG", &jpeg_bytes(4, 2));
    write_file(&root, ".picasa.ini", b"[dsc_0001.jpg]\nstar=yes\n");
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);

    assert_eq!(lib.starred_count().unwrap(), 1);
}

#[test]
fn a_subtree_scan_applies_stars_too() {
    // scan_subtree is the path the file watcher uses when a folder changes, which is
    // exactly what happens when someone stars a photo in Picasa. Wiring the pass into
    // scan_watched alone would leave this broken while every other test passed.
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);
    assert_eq!(lib.starred_count().unwrap(), 0);

    write_file(&root, "sub/.picasa.ini", b"[a.jpg]\nstar=yes\n");
    scan_subtree(&lib, &watched, &root.join("sub"), 2, &ScanOptions::default(), &mut |_| {}).unwrap();

    assert_eq!(lib.starred_count().unwrap(), 1);
}
```

**Check `scan_subtree`'s real signature before writing that last test** and adapt the call — the argument list above is the shape, not a quotation. Find it with `rg -n 'pub fn scan_subtree' -A 10 crates/photon-core/src/scanner.rs`.

```rust
#[test]
fn a_folder_that_cannot_be_read_keeps_its_stars() {
    // Spec §7: failing to read is not evidence that the stars are gone, unlike a
    // successfully-read INI with no entry for the photo. The `Option` in read_stars's
    // return type is the only thing carrying that distinction, and this is the only test
    // that exercises the caller's side of it.
    //
    // Make the folder unreadable however your platform allows (on Unix, chmod 0o000 the
    // directory after the scan, and restore it before the temp dir is dropped). If that is
    // not expressible portably, say so in your report and put it on the README checklist
    // instead of writing a test that does not actually exercise the case.
    let (dir, lib) = temp_library();
    let root = photos_root(&dir);
    write_file(&root, "sub/a.jpg", &jpeg_bytes(4, 2));
    write_file(&root, "sub/.picasa.ini", b"[a.jpg]\nstar=yes\n");
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    scan(&lib, &watched, 1);
    assert_eq!(lib.starred_count().unwrap(), 1);

    // ...make root/sub unreadable, rescan, restore...
    // assert_eq!(lib.starred_count().unwrap(), 1, "no evidence is not evidence of no stars");
}
```

**Also drop `jpeg_with_xmp` from the test module's import list** (`scanner.rs:539`). Deleting
the test below leaves it the only unused name there, and `unused_imports` under
`clippy -D warnings` would fail this task's own gate.

**The existing test `a_scan_reads_the_xmp_rating_into_the_library` will break in this task.** It writes a photo with an XMP rating of 3 and asserts `starred_count() == 1`; once the pass runs, that folder has no INI, so the star is cleared and the count is 0. **Delete it** — it pins behaviour this feature removes. Task 3 removes the XMP scan-path caller that made it pass.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p photon-core picasa_stars star` (or run the whole scanner module). Expect failures for the new tests, and a failure for `a_scan_reads_the_xmp_rating_into_the_library` if you have not yet deleted it.

- [ ] **Step 3: Add the two library methods**

In `crates/photon-core/src/library/items.rs`, beside `update_items`. Match that file's house style exactly: `writer()` → `conn.transaction()` → `tx.prepare_cached(...)` in a scoped block → loop → `tx.commit()`.

```rust
/// Every live item in one folder, as `(id, lowercased file name)`.
///
/// Lowercased here because Picasa's INI may disagree in case with the files on disk, and
/// this codebase folds case in Rust rather than in SQL: there is no `COLLATE NOCASE` on
/// `file_name` and `lower()` is ASCII-only in SQLite without the ICU extension, which is a
/// native dependency photon does not take.
pub fn folder_item_names(&self, folder_id: i64) -> Result<Vec<(i64, String)>> {
    let conn = self.reader();
    let mut stmt = conn.prepare_cached(
        "SELECT id, file_name FROM items WHERE folder_id = ?1 AND missing_since IS NULL",
    )?;
    let rows = stmt
        .query_map(params![folder_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?.to_lowercase()))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Sets the rating on specific items, leaving every other column alone.
///
/// Deliberately not part of `update_items`: that rewrites a row from a rescanned file and
/// resets its thumbnail, which is wrong for a star that changed while the photo did not.
pub fn set_ratings(&self, ratings: &[(i64, u8)]) -> Result<()> {
    let mut conn = self.writer();
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached("UPDATE items SET rating = ?2 WHERE id = ?1")?;
        for (id, rating) in ratings {
            stmt.execute(params![id, rating])?;
        }
    }
    tx.commit()?;
    Ok(())
}
```

Add a unit test for each in that file's test module, following its conventions (`temp_library()`, `seed_folder(&lib, Path::new("/p"))`, `new_item(folder, path, taken_at)`): that `folder_item_names` lowercases and excludes missing rows, and that `set_ratings` changes only the rating.

- [ ] **Step 4: Implement the pass and wire both call sites**

In `crates/photon-core/src/scanner.rs`:

The function's doc comment must carry why it exists, since the naive alternative is so
tempting:

```rust
/// Applies each walked folder's Picasa stars to its photos.
///
/// Runs after the walk rather than inside `describe()`, which is called only for photos
/// whose size or mtime changed. Starring a photo in Picasa rewrites the folder's INI and
/// leaves the photo untouched, so on a rescan every photo takes the `unchanged` branch and
/// a star read in `describe()` would never be written.
///
/// The INI is the only authority: a photo it does not name is set to unstarred, so removing
/// a star in Picasa clears it here too. A folder that cannot be read is skipped instead,
/// because failing to read is not evidence that the stars are gone.
```

**Where to call it — this placement is load-bearing, and the obvious spot is wrong.**

In `scan_watched`, put the call **after the empty-root offline guard**, immediately before
`finish_mark_purge`:

```rust
    // A reachable but empty root usually means an unmounted volume left its mount
    // point behind, not that every known file vanished at once.
    if seen.files_seen == 0 && known.values().any(|k| !k.missing) {
        lib.set_watched_online(watched.id, false)?;
        progress(&seen);
        return Ok(ScanReport { offline: true, ..ScanReport::default() });
    }
    lib.set_watched_online(watched.id, true)?;

    apply_picasa_stars(lib, &walked)?;          // <-- here

    let (marked, purged) = finish_mark_purge(lib, known)?;
```

**Not after the `cancelled` block.** That guard exists because a reachable but empty root
usually means an unmounted volume left its mount point behind, and it deliberately leaves
every known item untouched. Running the pass before it means `read_stars(root)` returns
`Some(empty)` for the still-mounted-but-empty root and **every photo in it is set to
`rating = 0`** — a rescan of an unplugged drive silently wipes those stars. The existing
`empty_reachable_root_is_treated_as_offline` test never inspects ratings, so nothing would
catch it.

In `scan_subtree`, call it at the equivalent point — after its `cancelled` and
`skip_mark_purge` early returns, before its `finish_mark_purge`. Read the real function
before placing it; it uses `outcome.cancelled` rather than a destructured binding.

**And pass the folders the walk actually entered, not `folder_ids`.** In `scan_subtree`,
`folder_ids` is pre-seeded by `seed_ancestors`, which inserts the watched root *and every
ancestor directory* before the walk begins:

```rust
let (mut folder_ids, parent_id) = seed_ancestors(lib, watched, relative, scan_id)?;
```

Handing that to the pass would re-read and rewrite ratings for every ancestor of the scanned
folder — contradicting `scan_subtree`'s own documented contract that "everything this touches
is restricted to that subtree", and spec §7's "only for folders the walk completed". It would
also make every watcher event touch the whole ancestor chain, when `scan_subtree` exists
precisely to be cheap.

So have `walk_tree` record what it entered. Add a field to `WalkOutcome`:

```rust
struct WalkOutcome {
    report: ScanReport,
    seen: ScanProgress,
    /// The directories this walk actually entered, as opposed to those `seed_ancestors`
    /// pre-inserted into `folder_ids`. The Picasa pass must only touch these: a subtree
    /// scan that rewrote its ancestors' ratings would break its own isolation contract.
    walked: Vec<(PathBuf, i64)>,
    incomplete_prefixes: Vec<PathBuf>,
    skip_mark_purge: bool,
    cancelled: bool,
}
```

Populate it in `walk_tree` at the same place `folder_ids` is filled:

```rust
let id = lib.upsert_folder(watched_id, parent, path_str, scan_id)?;
folder_ids.insert(path.to_path_buf(), id);
walked.push((path.to_path_buf(), id));
continue;
```

and have `apply_picasa_stars` take `&[(PathBuf, i64)]`:

```rust
fn apply_picasa_stars(lib: &Library, walked: &[(PathBuf, i64)]) -> Result<()> {
    for (dir, folder_id) in walked {
        let Some(stars) = crate::picasa::read_stars(dir) else {
            tracing::debug!(?dir, "leaving stars alone for an unreadable folder");
            continue;
        };
        let ratings: Vec<(i64, u8)> = lib
            .folder_item_names(*folder_id)?
            .into_iter()
            .map(|(id, name)| (id, u8::from(stars.contains(&name))))
            .collect();
        for chunk in ratings.chunks(BATCH) {
            lib.set_ratings(chunk)?;
        }
    }
    Ok(())
}
```

`scan_watched` destructures `WalkOutcome`, so add `walked` to its pattern; `scan_subtree` binds
the whole struct as `outcome`, so it reads `&outcome.walked`.

- [ ] **Step 5: Run the tests**

`cargo test --workspace` — all green, including the pre-existing scanner and items tests.

- [ ] **Step 6: Prove the central test discriminates**

`a_star_added_after_indexing_is_picked_up_without_the_photo_changing` is the test this whole task exists for, so prove it properly. Temporarily move the star read into `describe()` — the naive implementation the spec rejects — by having `describe()` call `crate::picasa::read_stars(entry.path().parent().unwrap())` and set `rating` from it, and removing the `apply_picasa_stars` calls. Run the scanner tests.

Expect `a_star_added_after_indexing_is_picked_up_without_the_photo_changing` to FAIL (the photo is unchanged, so `describe()` never runs) while the first-scan tests still pass. **Paste the real failure.** Restore.

If it passes against that implementation, the test does not pin §4 and must be replaced — say so in your report.

- [ ] **Step 7: Full gate and commit**

```bash
git add crates/photon-core
git commit -m "feat(stars): apply Picasa INI stars as a per-folder pass after the walk"
```

---

### Task 3: Unhook XMP from the scan path

**Files:**
- Modify: `crates/photon-core/src/metadata.rs` (remove the `read_rating` call ~line 28; fix the tests it changes)
- Modify: `crates/photon-core/src/scanner.rs` (`describe()` ~line 480)
- Modify: `README.md` (upgrade note and manual checklist)
- Keep untouched: `crates/photon-core/src/xmp.rs` and all of its tests

**Interfaces:**
- Consumes: nothing. Runs last so the tree is never without a star source.
- Produces: nothing.

**Context you need.** `crates/photon-core/src/metadata.rs` currently has:

```rust
// Zero rather than None when there is no packet: the file was read and had nothing to
// say. None is reserved for rows no scan has ever looked at.
meta.rating = Some(crate::xmp::read_rating(path).unwrap_or(0));
```

That is the **only** `xmp::`-qualified call site in the entire repo. `xmp.rs`'s own tests call `read_rating` and `rating_from_xml` unqualified from inside the module and must keep passing — **the module stays, with its tests green and no caller.**

`ImageMeta.rating` has exactly one production consumer, `describe()`'s `rating: meta.rating`.

- [ ] **Step 1: Remove the scan-path caller**

Delete the `meta.rating = ...` line and its comment from `read_image_meta`. `ImageMeta.rating` then stays `None` as initialised. Update its doc comment, which currently claims `Some(0..=5)` once read:

```rust
/// Always `None`: ratings come from Picasa's per-directory INI, applied by the scanner
/// after the walk (see `scanner::apply_picasa_stars`), not from anything in the file
/// itself. Kept on the struct because `NewItem` still carries the column.
pub rating: Option<u8>,
```

In `describe()`, `rating: meta.rating` now always yields `None`, which is correct — `None` means "no scan has looked at this yet", and the star pass fills it in moments later. Leave the field wired rather than hardcoding `None`, and add a one-line comment saying the pass sets it.

- [ ] **Step 2: Fix the tests this changes**

In `metadata.rs`'s test module:
- **Delete** `reads_the_xmp_rating_alongside_exif` and `a_photo_without_xmp_reads_as_unrated_rather_than_unread`. Both pin behaviour that is being removed; keeping them would mean keeping the call.
- In the three full-struct comparisons (`reads_exif_orientation_and_date`, `png_without_exif_gets_defaults`, `unreadable_file_yields_zeroed_meta`), change `rating: Some(0)` to `rating: None`.

Run `cargo test -p photon-core` and fix anything else that falls out. Do not delete or weaken any test in `xmp.rs`.

- [ ] **Step 3: Fix one stale doc comment**

`crates/photon-core/src/library/items.rs` (~line 682) documents `rated()` as producing what
"the scanner produces once it has read the file's XMP". That becomes false in this task.
Reword it to say ratings come from the Picasa INI pass. Find it with
`rg -n "XMP" crates/photon-core/src/library/items.rs`.

- [ ] **Step 4: Confirm the XMP module is still built and tested**

Run: `cargo test -p photon-core xmp`
Expected: every `xmp` test passes. Then `rg -n 'xmp::' crates/` — expect **no** hits outside `xmp.rs` itself. Paste both results into your report: this is the evidence that the module survived intact with no caller.

- [ ] **Step 5: Update the README**

Two changes, matching the file's existing wording and bullet style:

1. **Checklist line ~110 becomes false.** It currently reads that a library carried over from
   v0.2.0 "shows no stars until it is deleted and rebuilt". After this feature a carried-over
   library gets stars on the first scan of each folder — spec §9 promises exactly that.
   Rewrite it to check that instead: that stars appear on a rescan without a rebuild. Find it
   with `rg -n 'shows no stars' README.md`.
2. The upgrade note near the top currently tells people to delete `photon/library.db` to pick up ratings. Reword it for this release: stars now come from Picasa's `.picasa.ini` / `Picasa.ini`, a library from v0.3.x holds XMP-derived values that no INI has confirmed, and deleting the library is the honest way to get a clean state.
3. Add to `## Manual smoke checklist`:
   - a folder starred in Picasa shows exactly those photos under Starred after a scan;
   - starring a photo in Picasa and rescanning makes it appear, **without deleting the library**;
   - un-starring one in Picasa and rescanning makes it disappear.

   Do **not** add a fourth bullet about modification times — the checklist already carries
   one ("After a full scan, no photo file's modification time has changed").

- [ ] **Step 6: Full gate and commit**

```bash
git add crates/photon-core README.md
git commit -m "refactor(stars): stop reading xmp:Rating during scanning"
```

---

## Coverage against the spec

| Spec section | Task |
|---|---|
| §1 stars come from the INI, not XMP | 1 (parser), 2 (applied), 3 (XMP unhooked) |
| §1 the invariant — photon never writes an INI | 1 (read-only by construction), 3 (README manual check) |
| §2 XMP module kept, no scan-path caller | 3 (Step 3 verifies both halves) |
| §3 `.picasa.ini` preferred, never merged | 1 (Step 1 test, Step 5 revert-proof) |
| §3 that folder only, no inheritance | 1, and 2 (`a_parent_folder_s_ini_does_not_star_photos_in_a_subfolder`) |
| §3 `yes`/`true`/`1`, case-insensitive; other keys ignored | 1 (Step 1 tests, Step 5 revert-proof) |
| §3 case-insensitive file names | 1 (parser lowercases), 2 (`folder_item_names` lowercases; end-to-end test) |
| §3 bounded read | 1 (`MAX_INI`) |
| §4 the pass runs after the walk, not in `describe()` | 2 (Step 4, pinned by Step 6's revert-proof) |
| §4 **both** `walk_tree` call sites | 2 (`a_subtree_scan_applies_stars_too`) |
| §5 the INI is the only authority; unconfirmed stars cleared | 2 (removal and deleted-INI tests) |
| §6 `star=yes` → `rating = 1`, no migration | 2 (`u8::from(...)`; no schema file is touched by any task) |
| §7 unreadable folder keeps its stars | 1 (`None` vs `Some(empty)`), 2 (`a_folder_that_cannot_be_read_keeps_its_stars`) |
| §7 cancelled scan applies nothing | 2 (Step 4 placement, after the `cancelled` return) |
| §7 an offline/empty root does not clear stars | 2 (Step 4 placement, after the offline guard) |
| §7 only folders the walk entered | 2 (`WalkOutcome.walked`, not the pre-seeded `folder_ids`) |
| §8 duplicate sections | 1 (`a_duplicate_section_starring_the_file_wins`) |
| §3 the bounded read actually bounds | 1 (`the_read_is_bounded`) |
| §8 parser tests | 1 |
| §8 the pass's tests | 2 |
| §8 manual checklist | 3 (Step 4) |

**Two notes for whoever reviews this plan.** The coverage table above was written against the spec section by section, and it is worth checking rather than trusting — on a previous feature in this project the table mapped a frontend requirement to a backend-only task and nothing implemented it. And §7's "a folder that cannot be read keeps its stars" is carried by the `Option` in `read_stars`'s return type; if a task ever flattens that to a plain `HashSet`, the distinction between "no stars" and "no evidence" is lost silently.
