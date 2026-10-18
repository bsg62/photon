# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

photon is a local photo manager for Linux, macOS and Windows — a spiritual successor to
Picasa 3. Rust throughout: a headless core, an engine over it, and a window drawn by egui
on the GPU.

## Commands

**The Rust gate — all five must pass before any commit.** CI runs exactly these:

```bash
cargo fmt --all            # run this FIRST, not just --check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo bench -p photon-core --bench grid --no-run
```

Tests run through **cargo-nextest** (`cargo install --locked cargo-nextest`; settings in
`.config/nextest.toml`). It runs no doctests and the workspace has none: a doc example is
written as a test. **photon-ui's whole-application tests are one binary**, `all`
(`tests/main.rs`, `autotests = false`): a new file in `tests/` needs a `mod` line there, or
it is never built (`every_file_in_tests_is_a_module_of_the_binary` checks).

```bash
cargo test -p photon-core the_test_name               # by name substring
cargo test -p photon-core --lib scanner               # one module
cargo nextest run -p photon-ui --test all dialogs::   # one file of the whole-app tests
```

- **Fixtures.** Solid-colour JPEGs of different dimensions often encode to the same byte
  size; append bytes after the end-of-image marker when sizes must differ. Keep fixtures
  small (a thin strip proves a resolution claim; megapixels cost seconds in debug).
- **Windows is slow at small files**, its C: drive slowest (`ci.yml` moves TEMP to D:). A
  fixture writes its rows in one transaction (`Library::set_thumb_states`) and makes no
  more files than its test needs.
- **CI:** `gh pr checks N --watch` (straight after a push it may say "no checks reported";
  wait until checks exist). Caches are saved from `main` and `native-ui` only and builds
  carry no debug info, so a PR changing `Cargo.lock` recompiles the difference every run;
  a job that suddenly takes 4x longer lost its cache (`gh cache list`).
- **Repository chores** (also in CI; run before tagging): `cargo run -p xtask -- versions`
  (`--tag vX.Y.Z` to match a tag), `cargo run -p xtask -- metadata`.
- **Seeing the window:** `cargo run -p xtask -- native-shot` draws it off screen into
  `target/screenshots/native-*.png` (needs a GPU adapter, no display; not in CI). The
  website (`site/`, photon.webcodr.io: `index.html`, `faq.html`, `picasa.html`, `404.html`,
  hand-written, no build step) uses the five `native-site-*` pictures as `site/img/*.webp`
  (`magick x.png -quality 85 x.webp`), `-main-light` also as `site/img/og.jpg` at quality
  88. Every page's colours are held to the app's tokens by `tokens.rs`'s tests (a new page
  goes into its `PAGES`; a page's dark palette is written twice, for the system's setting and
  for the visitor's choice made through `site/theme.js`, and the tests hold the two equal),
  the front page's icons to `icons.rs`'s; its font, Geist, is served
  from `site/fonts/`, so the site requests nothing from anywhere else. **Never run
  `cargo run -p photon-ui` to verify a change.**
- **Two checkouts never share a `CARGO_TARGET_DIR`**: cargo will silently run the other
  checkout's binary.

## Architecture

Four crates:

- **`photon-core`** - headless. SQLite library, scanning, thumbnails, metadata, faces.
- **`photon-engine`** - `Engine`, its commands (`commands.rs`), the folder watcher. No UI
  runtime dependency (`the_engine_depends_on_no_ui_runtime` checks its manifest).
- **`photon-ui`** - the window, on egui and wgpu. Binary `photon`.
- **`xtask`** - chores, `native-shot`, `package` (installers), `fixture-library`.

The UI was Svelte 5 in Tauri 2 until v0.53.1 (git history holds it); names like
`Grid.svelte`, `nav.ts`, `createSelect` in the notes say where a rule was ported from.
Specs live in `docs/superpowers/specs/`, plans in `docs/superpowers/plans/`.

### The architecture notes — read the relevant one before changing an area

Every rule in them is a bug that was found once, usually with a test named after it.
Search them (`grep -n` the type or function you are touching) before editing.

| File | Covers |
|---|---|
| `docs/architecture/grid.md` | `GridIndex`, the refresh chain, `data_dirty`/`counts_epoch`, grid order, sort and grouping, hidden photos, parameterised views, search grammar, offsets |
| `docs/architecture/scanning.md` | the walk, post-walk passes, Picasa INI, metadata backfill, recycle bins, followed moves/renames, the watcher, path canonicalisation |
| `docs/architecture/library.md` | schema and migrations, index/plan rules, readers/writer, thumbnail budget and GC, edits, duplicates and look-alikes, face detection, people |
| `docs/architecture/ui-shell.md` | styling tokens, state modules vs views, wheel, grid layout, motion, thumbnails/textures, text, nav and the views' queue, sidebar, search box, controls, dropdown, year strip, status bar, launch, empty library |
| `docs/architecture/ui-grid.md` | selection, the grid's keys, rubber band, the write queue, menus, hit testing, rename field, egui text-field lessons, dialogs |
| `docs/architecture/ui-platform.md` | folder drop, export, folder picker, clipboard, Reveal/Open, GPU adapter, AccessKit |
| `docs/architecture/ui-viewer.md` | viewer, full picture and levels, zoom/pan, info panel, sRGB conversion, faces in the viewer, person dialog, crop, slideshow, compare |
| `docs/architecture/ui-pages.md` | video playback and posters, Settings, shortcuts and `?`, copies view, People page |
| `docs/architecture/ui-testing.md` | screenshot tests, the test `Driver`, installers, locale, `--probe`, allocator |
| `docs/architecture/conventions.md` | the long form of Conventions below |

When you learn a new non-obvious rule, add it to the matching note, not here.

### Rules that cut across the code

Core and engine:

- **The refresh chain.** A change to rows must travel `Engine::refresh_grid()` →
  `publish_if_current()` → `library_changed` → `App::adopt`, or the UI shows stale data. A
  committed command rebuilds through `refresh_after_write`; `refresh_grid_derived` only for
  writes no collection, count or Settings query reads. Both publish guards (epoch, `seq`)
  have their own test - keep it that way. A writer that changes what a sidebar count reads
  without `refresh_grid` must move `counts_epoch`.
- **`ScanReport::touched_rows`**: a scan that changes data by a new means adds its counter
  there, or the grid never rebuilds for it.
- **Grid order**: `GRID_ORDER` only through `grid_query(select, shown, filter)`, which pairs
  it with its driver. The user's sort is applied in Rust after the query and is part of the
  `ViewState` snapshot; the grouping is a field of `Sort` (change a `Sort` by spreading it).
  "Flat" means `!has_header`, not "no folder".
- **Hidden photos**: every user-visible query filters `hidden = 0` (`grid_query` takes a
  `Shown`); any writer creating item rows inherits the folder's hidden flag.
- **An offset is valid against one index version only**: re-find by id
  (`GridIndex::position_of`). Likewise in the UI: anything held across a rebuild, a frame
  or a round trip is held by id, never by offset - and library ids are reused once freed.
- **Search**: a new fact is a haystack field (set per photo, cleared in
  `Haystacks::truncate`); a new filter is a prefixed term; `search_help.rs`'s tests hold
  its list to the parser.
- **Scanner**: `describe()` runs only for files whose size/mtime changed - data that can
  change otherwise needs a post-walk pass over `WalkOutcome.walked`. `walk_tree` has two
  callers (`scan_watched`, `scan_subtree`); wire into both. A new `describe()` field bumps
  `metadata::EXIF_VERSION`. Keyword readers go through `EFFECTIVE_TAGS`/`TAG_FILTER`,
  writers through `write_tags`. Skipped directories are the one predicate `passed_over`.
- **Paths**: store only `paths::canonicalize` (never `dunce`/`fs::canonicalize`);
  "inside the library" is `paths::is_within`, collision is `paths::overlaps`.
- **Schema**: append to `MIGRATIONS`; a bump breaks the literal version/table-count tests on
  purpose - update the numbers, never loosen them. An index serving a query gets a plan
  test; a query over every live photo writes `+missing_since IS NULL`; without `ANALYZE`
  partial indexes tie and the last created wins, so a new partial index can steal the
  counts (`the_starred_and_video_counts_are_served_by_their_indexes`).
- **Thumbnails**: name one with `Item::thumb_key()`, never a bare fingerprint. A write that
  can orphan a thumbnail calls `settings::bump_thumb_gc_epoch` in its transaction (the
  tripwire in `settings.rs` lists them). A write for changed *content* clears
  `content_hash`, `percep_hash`, `similar_group` and detections; `move_items` keeps them.
- **Edits** are rendered by the backend, never drawn by the window; writes take
  `Engine.edit_write`, full-size renders `RENDERING`.
- **Background passes** go through `spawn_pass`; tests wait for them with `settle`, not
  `wait_for_scans`.
- **Faces and people**: bump `DETECTOR_VERSION`/`EMBEDDER_VERSION` when a model or its
  thresholds change; every People write goes through `Engine::write_people`; only confirmed
  faces carry a name; any reader of both face tables goes through `face_detect::merge`.
- **Case-insensitive matching is done in Rust**, never in SQL (no ICU, no `COLLATE NOCASE`).

The window:

- **State modules vs views**: state modules are plain Rust with no egui type
  (`STATE_MODULES` in `lib.rs`, kept by hand; `state_modules_name_no_egui_type` checks).
- **`eframe` is pinned exactly**; read the pinned version's source, never egui from memory.
- **Nothing that reads SQLite runs on the UI thread** (`tasks::Latest`). A step that moves
  the engine's view goes through `tasks::Queue`, never `Latest`, and what a grid shows comes
  from the step that built it (`nav::Landed`, `Nav::adopt`), never read from the engine.
- **Writes go through `App::write` and the write queue**; one that changes a view's rows
  belongs in `Write::moves_grid`. A key that writes carries the selection it was pressed on
  (`Asked`) and is not answered on repeat.
- **Every key the grid gains goes through `GridView::elsewhere`.** Menus, dialogs and a held
  band take keys out of the frame before anything else reads them.
- **egui text fields**: build `EventFilter` with all its fields (`default()` drops the
  arrows); read and take Enter/Escape before the field is drawn; give each opening a fresh
  `TextEditState` (undo history).
- **"Is this press on me?"** is `contains_pointer` plus `Context::layer_id_at` - one judge.
- **A dialog acts on what it took when it opened**; a menu rebuilds its items every frame.
- **A number is shown beside the state it was read with, or not at all.**
- **Colours are tokens** in `theme/tokens.rs` (contrast-tested); icons in `icons.rs` from
  `lucide-static`. Text that may be in another script goes through `text::paint_line`.
- **Per-frame work multiplies by the frame's duration**, and a new loop or timer is worth
  asking what known-broken path it now drives.
- **Tests of the window**: drive it through `tests/common`'s `Driver` (`until`, `settle`,
  `frames_in`); wait for what is said, never for a count of messages; launch
  `without_animations`, with `no_platform` and `no_posters` stand-ins. Text read with
  `include_str!` is CRLF on Windows - read by line. A test that asserts a still window
  draws nothing runs through the event-loop driver, not by stepping kittest.

## Conventions

- **photon never writes to, moves or deletes photo files.** The only file it writes inside
  a watched folder is Picasa's `.picasa.ini`/`Picasa.ini`, via `picasa::set_star` only, one
  `star=` line, every other byte preserved; an INI that is a symlink or not a regular file
  is refused, by the reader too. Export writes only *new* files (`create_new`), only where
  the user pointed, never inside a watched root (`Engine::export_items` refuses). Albums,
  edits, detected faces and people live only in `library.db`; Picasa's faces, contacts,
  albums and `hidden=`, and the photo's keywords, are read and never written. **Any further
  write is a spec-level decision, not a code change** (which is why a star does not follow
  a renamed single file). The INI writer and reader share one line classifier on purpose.
- **No system library dependencies beyond the system's own** (graphics via wgpu, the OS
  video framework, D-Bus in Rust). photon's C - SQLite, libwebp, libjpeg-turbo - is
  vendored and built with `cc`; nothing wrapping a C/C++ SDK. A new C dependency is a
  spec-level decision. nasm is optional (CI/release only; `avif-asm` for x86-64 only).
  Faces run in `tract` on the CPU with embedded models; nothing outside `face_detect` and
  `face_embed` names it.
- **AVIF is decoded in `photon_core::avif`, not by `image`.** Every full decode goes
  through `decode::decode_image` (previews through `decode::preview_decode`); calling
  `ImageReader` directly skips AVIF. A panic in rav1d aborts the process
  (`thumbs/inflight.rs` keeps it from repeating).
- **Never launch the GUI to verify a change.** Use the tests and `native-shot`'s pictures;
  anything needing eyes goes on `docs/smoke-checklist.md`.
- **A new test must be shown to fail with its change reverted.** A compile error is not
  proof. When a change cannot have one (a race with no seam, a picture only a person can
  judge), say so and why in the commit message. A probe that passes is a finding: ask what
  input would make the reverted code differ, and write that case. Revert with an exact
  replacement, not a loose `sed`.
- **A large branch gets an independent review before it merges** - point it at the effect
  wiring and at anything a new feature *arms* in old code.
- **Read the code before proposing a feature** - several "new" features already existed.
- **Comments carry the reasoning, not the mechanics.** Several stop a future reader from
  "simplifying" a load-bearing line; a wrong justification is a defect.

## Releasing

The version lives in the workspace's `Cargo.toml` alone, plus `Cargo.lock`: bump it, run
`cargo update --workspace --offline` (a release commit touches exactly two files), run
`cargo run -p xtask -- versions --tag vX.Y.Z`, commit as `chore(release): X.Y.Z` straight to
`main`, push the tag. `release.yml` builds and reads the installers and creates a **draft**
release; check the six artifacts (two `.dmg`, `.deb`, AppImage, `.msi`, `SHA256SUMS`) before
publishing. A schema bump makes the release a minor.

Replace the generated notes with hand-written ones shaped like v0.14.0 and v0.15.0:
Highlights, Your files are untouched, Upgrading (when an older photon will refuse the
library, behaviour that changed), Not in this release, the unsigned line, the changelog
link. `gh release edit --notes-file` can report success while changing nothing: read the
body back before `--draft=false`. Installers are deliberately unsigned (no signing, no
notarisation, no auto-updater).
