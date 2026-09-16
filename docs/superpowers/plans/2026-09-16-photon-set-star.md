# photon Set Star Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A star button in the viewer and a badge on grid tiles. A star set in photon is written into the folder's Picasa INI, so Picasa sees it.

**Architecture:** `photon-core::picasa` gains a byte-preserving writer sharing the reader's line classifier. `Engine::set_star` writes the INI first, mirrors the row second, refreshes the grid third, under one lock. A `createStarToggle` state machine drives the viewer button; the viewer keeps an unstarred photo on screen when it leaves the Starred view.

**Tech Stack:** Rust (std only for the write), Svelte 5 runes, vitest.

**Spec:** `docs/superpowers/specs/2026-09-16-photon-set-star-design.md`

## Global Constraints

- **photon never writes, moves or deletes photo files.** `picasa::set_star` is the one write inside a watched folder, and it changes `star=` lines only.
- **No new dependencies.** The atomic write and the Windows hidden attribute use `std` alone.
- **No schema change.** `rating` is already the column; `set_ratings` already writes it.
- **Never launch the GUI.** Verification is the test suites; anything needing eyes goes on the README's manual checklist.
- **The Rust gate** — `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run` — **and the UI gate** — `npm run check`, `npm test` — must pass before any commit.
- **Every new test must be demonstrated to fail with its change reverted.** A compile error is not a revert-proof. The two-threads-one-folder lost update, crash-mid-write atomicity and the stale-scan heal have no deterministic seam and are stated as untested in the commit message.
- **Disclose every deviation from this plan.**

---

### Task 1: The writer — `crates/photon-core/src/picasa.rs`

- [x] Extract `classify(line) -> Line` and rewrite `parse_stars` over it.
- [x] `ini_path` returns `io::Result<Option<PathBuf>>`; `read_capped` returns `io::Result<Vec<u8>>` with `FileTooLarge` past the cap; `read_stars` maps both to `None`.
- [x] `rewrite(bytes, file_name, starred) -> Option<Vec<u8>>`, `split_eol`, `line_ending`.
- [x] `write_atomically` with a `RemoveOnDrop` guard, unix permissions and the Windows attribute from the original's metadata.
- [x] `set_star(dir, file_name, starred) -> io::Result<bool>` and `ini_name(dir)` for the error path.
- [x] Tests (spec §7), each shown to fail against a writer that lacks the behaviour.

### Task 2: Core data and errors

- [x] `Item.rating`, selected by `Library::item`; `is_starred(rating)` shared by `map_grid_row` and the viewer; test `item_reports_its_rating`.
- [x] `Error::IniWrite { path, source }`; `AppError` kind `iniWrite`.

### Task 3: `Engine::set_star` and the command chain

- [x] `ini_write: Mutex<()>` on `Engine`; `set_star` as spec §3, with the doc comment forbidding self-write suppression.
- [x] `commands::set_star`, `ipc::set_star`, `generate_handler!`, `api.setStar`; `ViewerItem.starred`; `viewer_item` refuses a soft-deleted row.
- [x] Engine tests: the INI is written and the grid follows; a rescan agrees; unstarring in Starred drops the photo; missing/unknown refused with no INI; a failed INI write leaves the database and version alone; `viewer_item` reports the star and refuses a missing photo.

### Task 4: The UI

- [x] `ui/src/lib/star-toggle.svelte.ts` and its vitest suite.
- [x] `Viewer.svelte`: the button, `.star` in the pan exemption, `orphaned`/`reload`, the probe in the rebind effect, `goto` and `close` aware of an orphaned offset.
- [x] `Tile.svelte`: the badge.

### Task 5: Docs and the promise

- [x] This plan and its spec; the superseded note in the 2026-09-13 spec.
- [x] CLAUDE.md's promise bullet; README's intro, "Stars and Picasa" section and the smoke checklist.
