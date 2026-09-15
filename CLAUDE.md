# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

photon is a local photo manager for Linux, macOS and Windows — a spiritual successor to
Picasa 3. Rust core, Tauri 2 shell, Svelte 5 UI.

## Commands

**The Rust gate — all four must pass before any commit.** CI runs exactly these, so a commit
that skips one is a commit CI will reject:

```bash
cargo fmt --all            # run this FIRST, not just --check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

**The UI gate**, from the repo root:

```bash
npm run check   # svelte-check --fail-on-warnings: 0 errors AND 0 warnings
npm test        # vitest
```

**Single test:**

```bash
cargo test -p photon-core the_test_name          # by name substring
cargo test -p photon-core --lib scanner          # one module
npm test -w ui -- src/lib/nav.test.ts            # one UI file
```

**There is no component test harness.** vitest runs with `environment: 'node'`, so a
`.svelte` file cannot be rendered or asserted on. Logic that needs a test goes in a
`.svelte.ts` factory tested with fake timers (`createSearchBox`, `createThumbRequest`); what
is left in the component is effect wiring, verified by `svelte-check` and the README's smoke
checklist, not by a test.

**Repository chores** (also run in CI, so run them before tagging):

```bash
cargo run -p xtask -- versions            # Cargo.toml, tauri.conf.json, ui/package.json agree
cargo run -p xtask -- versions --tag v0.5.0   # ...and match the tag
cargo run -p xtask -- metadata            # licence and installer metadata are complete
```

`npm run dev` runs the app with hot reload. **Do not run it to verify a change** — see
Conventions.

## Architecture

Three crates plus the UI:

- **`photon-core`** — headless. SQLite library, scanning, thumbnails, metadata. Knows nothing
  about Tauri.
- **`photon-app`** — the Tauri shell. Owns `Engine`, the IPC surface and the custom protocol
  that serves thumbnails.
- **`xtask`** — repository chores. Binary only, no `lib.rs`.
- **`ui/`** — Svelte 5 runes + TypeScript, an npm workspace.

### The grid index is the spine

`Engine` holds one in-memory `GridIndex` (`grid.rs`) built from a single ordered query, plus a
version number. Everything downstream — paging, folder sections, viewer navigation, neighbour
preloading, the sidebar — reads from that one index, which is why a "view" is just a different
row set rather than a different code path.

The refresh chain is worth knowing end to end, because a change that alters rows without
travelling it will update the database while the UI shows stale data:

```
scan/mutation → Engine::refresh_grid() → snapshot (view, query, epoch, seq)
              → query + GridIndex::build(), no engine lock held
              → publish_if_current(): discarded if the view changed (epoch) or a
                later-stamped rebuild already published (seq); else bumps version
              → events::library_changed → UI listener → api.gridInfo() → re-render
```

Rebuilds run unlocked so a view switch never waits behind a scan's rebuild; the two publish
checks are what make that safe, and both are needed. A setter's own rebuild is the authority
for a view change. A discarded rebuild never loses rows: every commit is followed on its own
thread by a rebuild stamped after it, and the highest stamp always publishes.

`ScanReport::touched_rows` gates the end-of-scan refresh on whether a scan actually moved
rows. A change that alters data by some *other* means must add its own counter to `ScanReport`
and fold it into `touched_rows`, or the grid silently never rebuilds.

**Grid order** (`items.rs`, `GRID_ORDER`) is the folder's oldest photo descending, then each
folder's photos oldest to newest. The sidebar groups by the same value, so the list is an index
of the grid. Changing one without the other splits them onto different axes. `GRID_ORDER` reads
columns only `folder_order(filter)` supplies, and `grid_query(select, filter)` is the one place
the two are paired: in a filtered view (Starred) the driver's filter must equal the outer
`WHERE`, so a folder is placed by its oldest *matching* photo, which is what keeps the sidebar
and the grid agreeing. A query assembled by hand with `GRID_ORDER` and no driver compiles and
fails at `prepare`, only when that view is opened.

**A grid offset is only meaningful against one index version.** Indexing a photo into a
folder that sorts earlier shifts every later offset, so anything holding an offset across a
rebuild — the viewer, the grid selection — must re-find its photo by id through
`grid_offset_of_item`. Clamping catches only the offset falling off the end; in range the
consumer silently shows a different photo.

### IPC is three files per command

Adding a command means touching all three, in this order:

1. `commands.rs` — a plain `pub fn` taking `&Engine`, returning `CmdResult<T>`. The logic.
2. `ipc.rs` — a `#[tauri::command(async)]` wrapper that only delegates.
3. `app.rs` — an entry in `tauri::generate_handler![...]`. Forgetting this compiles fine and
   fails at runtime.

A feature backed by a Tauri plugin has a fourth file: `capabilities/default.json` must grant
the permission (`clipboard-manager:allow-write-text`, say). A missing grant compiles and fails
only at runtime, inside the webview.

### TypeScript mirrors are hand-written and unchecked

`ui/src/lib/api.ts` mirrors the serde structs in `commands.rs` and `events.rs` (camelCase).
**Nothing validates the mirror.** A Rust field added without its TS counterpart is silently
`undefined` at runtime. Change both in the same commit.

Note `ui/tsconfig.json` includes `src/**/*.ts`, so **test files are typechecked too** — adding a
field to `GridInfo` breaks every `GridInfo` literal in `library.test.ts`, and `npm run check`
fails on it.

### Scanning, and why "unchanged" matters

`scanner.rs` walks with `walkdir` and calls `describe()` **only for files whose size or mtime
changed**. Anything derived from data that can change *without* the photo file changing —
Picasa's per-directory `.picasa.ini` stars are the worked example — cannot be read in
`describe()`; it needs its own pass after the walk, over `WalkOutcome.walked`.

`walk_tree` has **two** callers: `scan_watched` (a whole root) and `scan_subtree` (the file
watcher's path). Wiring a post-walk pass into only the first leaves the common case broken while
every test passes. `scan_subtree`'s `folder_ids` is pre-seeded by `seed_ancestors` with every
ancestor, so a per-folder pass must use `walked`, not `folder_ids`.

`skip_mark_purge` is set by a walkdir error carrying **no** path — a mid-iteration `read_dir`
failure (walkdir `lib.rs:1026`), not something the filesystem can be made to do on demand. An
unlistable walk root sets it too, but `read_stars` lists the directory as well, so that
branch's star behaviour cannot be tested from the filesystem either way.

### Schema

`library/schema.rs` holds `MIGRATIONS: &[&str]`, one entry per version, each run in its own
transaction with `PRAGMA user_version` bumped after it. A library from a newer photon is refused
with `SchemaTooNew`. SQLite runs in WAL mode, so `library.db` has `-wal`/`-shm` siblings.

A schema bump breaks tests on purpose: `library/mod.rs` asserts the literal version number in
two places, and the migration tests seed from `MIGRATIONS[..N-1]`. Update the numbers rather than
loosening them to `MIGRATIONS.len()`; the hardcoding is the tripwire. An index that serves a
specific query gets a plan test (`the_recent_view_is_served_by_its_index`), so drift between the
index and the `ORDER BY` fails rather than silently regressing.

**Reads are pooled, writes are one connection.** `Library::reader()` returns a `Result` and never
waits on another reader: it hands out an idle pooled connection or opens one (at most eight are
kept). `writer()` is a single mutexed connection.

**Thumbnail garbage collection is gated.** Any write that can orphan a thumbnail — deleting an
item, or changing `path`/`size`/`mtime_ms`, the fingerprint columns — must call
`settings::bump_thumb_gc_epoch` inside its own transaction. Today that is `purge_items`,
`update_items` and `remove_watched_folder`. The tripwire test in `settings.rs` enumerates those
three, so a *new* orphaning write is not caught automatically; the seven-day `THUMB_GC_MAX_AGE`
in `engine.rs` bounds the damage of a miss.

There is no `COLLATE NOCASE` anywhere and `lower()` is ASCII-only without ICU (a native
dependency this project does not take), so **case-insensitive matching is done in Rust**, not
in SQL.

## Conventions

- **photon never writes to, moves or deletes files inside watched folders.** This is the
  project's central promise; it appears in the README and every spec. Read-only, always.
- **No native library dependencies.** Nothing wrapping a C/C++ SDK. This is what made packaging
  tractable on three platforms, and it is why XMP and INI parsing are hand-rolled or pure-Rust.
- **Never launch the GUI to verify a change.** Verification is the test suites plus
  `svelte-check`; anything needing eyes goes on the README's `## Manual smoke checklist`.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not
  proof — it shows a symbol was missing, not that an assertion discriminates behaviour. Tests
  that pass with and without the change have shipped here more than once. When a change
  genuinely cannot have one — a race with no seam, a Svelte effect — say so in the commit
  message and why, rather than adding a test that passes either way.
- Comments carry the reasoning, not the mechanics. Several exist specifically to stop a future
  reader "simplifying" a load-bearing line; a wrong justification is treated as a defect.
- Design docs live in `docs/superpowers/specs/`, implementation plans in
  `docs/superpowers/plans/`.

## Releasing

The version lives in three files — `crates/photon-app/tauri.conf.json` is authoritative, with
`Cargo.toml` and `ui/package.json` agreeing — plus the regenerated `Cargo.lock`. Bump all
three, regenerate both lockfiles in place with `cargo update -p photon-core -p photon-app -p
xtask --offline` and `npm install --package-lock-only` (a release commit touches exactly five
files), run `cargo run -p xtask -- versions --tag vX.Y.Z`, commit as `chore(release): X.Y.Z`,
then push the tag. The tag push triggers `.github/workflows/release.yml`, which builds all
platforms and creates a **draft** release; verify the six artifacts attached (two `.dmg`,
`.deb`, AppImage, `.msi`, `SHA256SUMS`) before publishing it.

Installers are deliberately unsigned. There is no signing identity, no notarization and no
auto-updater; the README explains the per-OS warnings.
