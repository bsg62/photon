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
`.svelte.ts` factory tested with fake timers (`createSearchBox`, `createThumbRequest`,
`createSlideshow`, `createCropTool`) or a pure module (`timeline.ts`, `crop.ts`, `picture.ts`);
what is left in the component is effect wiring, verified by `svelte-check` and the README's
smoke checklist, not by a test. Layout *can* be measured without the GUI: a static page
holding the component's CSS, run through headless Chromium with `--dump-dom` and a load script
that writes `getBoundingClientRect()` into `document.title`.

**Test fixtures.** Solid-colour JPEGs of different dimensions often encode to the *same byte
size*; a test that needs distinct sizes appends bytes after the end-of-image marker. A fixture
of several megapixels costs seconds in a debug build: a thin strip proves a resolution claim as
well as a square does.

**Waiting for CI on a PR:** `gh pr checks N --watch`. Straight after a push it can answer
"no checks reported" and exit 0; wait until checks exist before trusting it.

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
and fold it into `touched_rows`, or the grid silently never rebuilds. Today the counters
beyond the obvious four are `restarred`, `refaced` (Picasa faces) and `enriched` (the
metadata backfill).

**Grid order** (`items.rs`, `GRID_ORDER`) is the folder's oldest photo descending, then each
folder's photos oldest to newest. The sidebar groups by the same value, so the list is an index
of the grid. Changing one without the other splits them onto different axes. `GRID_ORDER` reads
columns only `folder_order(filter)` supplies, and `grid_query(select, filter)` is the one place
the two are paired: in a filtered view (Starred) the driver's filter must equal the outer
`WHERE`, so a folder is placed by its oldest *matching* photo, which is what keeps the sidebar
and the grid agreeing. A query assembled by hand with `GRID_ORDER` and no driver compiles and
fails at `prepare`, only when that view is opened.

**Parameterised views.** `GridView::Search`, `Person`, `Album` and `Tag` are selected by an
argument held beside the view in `ViewState.arg` (the query, a contact hash, an album id,
a keyword). `entries_for(view, arg)` interprets it per view and binds it as a SQL
parameter; `GridInfo` reports it in typed fields (`searchQuery`, `person`, `album`, `tag`)
so the TS mirror stays explicit. `set_view` clears the argument unless it is re-entering the
same parameterised view, so a query can never be read as a contact hash. The membership
views filter the driver as well as the outer `WHERE`, the same way Starred does, or the
sidebar's year groups and the grid disagree.

**Search** is one view, not a family of them. `search_entries` builds the haystacks — file
and folder name, make, model, lens, `50mm`/`f/1.8`/`iso400`, keywords through `EFFECTIVE_TAGS`,
and the capture date as `YYYY-MM-DD` — and `search::Query` holds the grammar: words AND,
capitals-only `OR`/`AND`, quotes, `camera:`/`lens:` confined to `Fields.camera`/`.lens`, dangling
pieces ignored. A new searchable fact is a haystack there, not a view; a new *filter* is a
prefixed term. The query string is the whole interface, so UI links (the info panel's camera
and lens) go through `searchBox.search()`, which cancels a pending debounce first.

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
ancestor, so a per-folder pass must use `walked`, not `folder_ids`. The one post-walk pass
today is `apply_picasa`, which applies stars *and* faces from one `picasa::read_folder`.

**The metadata backfill.** `items.exif_version` records which generation of
`read_image_meta` last read a file; `metadata::EXIF_VERSION` is the current one. An
unchanged file whose stored version is behind is re-described and written through
`update_item_meta` (camera columns, keywords, `taken_at`, the version; not the fingerprint
columns, not `rating`). `taken_at` is included because a capture date outside 1970..tomorrow
is refused (`plausible_taken_at`) and the backfill is the only way an unchanged file is
re-dated. Adding a field to `describe()` without bumping `EXIF_VERSION` leaves every
existing photo without it forever. Keywords come from the file (XMP `dc:subject` and IPTC
2:25, `keywords.rs`) into `item_tags`; every writer of an item row goes through
`write_tags`. Every *reader* of keywords goes through `EFFECTIVE_TAGS` or `TAG_FILTER` in
`library/tags.rs`, which apply the user's rename/remove rules; a reader of `item_tags` that
bypasses them shows tags the user renamed or removed.

**The watcher drops access events** (`watcher/fs.rs`, `may_have_changed`). On Linux, notify
registers for inotify's open and close events, so reading a file's EXIF, listing a
directory or walkdir entering one all arrive as `Access` events on that directory. A scan
does all three to every directory it walks; treated as changes they scheduled the next
subtree scan of the same directories two seconds after every scan, forever. Anything that
makes the watcher react to more event kinds must keep a scan's own reads out.

`skip_mark_purge` is set by a walkdir error carrying **no** path — a mid-iteration `read_dir`
failure (walkdir `lib.rs:1026`), not something the filesystem can be made to do on demand. An
unlistable walk root sets it too, but `read_stars` lists the directory as well, so that
branch's star behaviour cannot be tested from the filesystem either way.

### Schema

`library/schema.rs` holds `MIGRATIONS: &[&str]`, one entry per version, each run in its own
transaction with `PRAGMA user_version` bumped after it. A library from a newer photon is refused
with `SchemaTooNew`. SQLite runs in WAL mode, so `library.db` has `-wal`/`-shm` siblings.

A schema bump breaks tests on purpose: `library/mod.rs` asserts the literal version number
twice (the opened version and `SchemaTooNew`'s `supported`) and the table count once, and the
migration tests seed from `MIGRATIONS[..N-1]`. Update the numbers rather than loosening them
to `MIGRATIONS.len()`; the hardcoding is the tripwire. An index that serves a
specific query gets a plan test (`the_recent_view_is_served_by_its_index`), so drift between the
index and the `ORDER BY` fails rather than silently regressing.

**Reads are pooled, writes are one connection.** `Library::reader()` returns a `Result` and never
waits on another reader: it hands out an idle pooled connection or opens one (at most eight are
kept). `writer()` is a single mutexed connection.

**Thumbnail garbage collection is gated.** Any write that can orphan a thumbnail — deleting an
item, or changing anything the thumbnail key is made of (`path`/`size`/`mtime_ms`, the
fingerprint columns, and `edit_turns`/`edit_crop`) — must call
`settings::bump_thumb_gc_epoch` inside its own transaction. Today that is `purge_items`,
`update_items`, `remove_watched_folder` and `set_item_edit`. The tripwire test in `settings.rs`
enumerates those four, so a *new* orphaning write is not caught automatically; the seven-day `THUMB_GC_MAX_AGE`
in `engine.rs` bounds the damage of a miss.

**Edits are rendered by the backend, and the thumbnail key includes them.** An edit
(`photon_core::edit`: EXIF orientation, then quarter turns, then a crop of the turned picture)
lives in `items.edit_turns`/`edit_crop` and is applied in exactly three places: the thumbnail
renderer, `/image/<id>` in `protocol.rs`, and the face rectangles in `viewer_item`. The UI draws
no edit; it writes one and the refresh chain reloads the picture under its new `thumbKey`.
Anything that names a thumbnail must use `Item::thumb_key()` (or `Edit::thumb_key` over the
fingerprint, as `map_grid_row` and `live_fingerprints` do); a bare `fingerprint` names the
*unedited* photo's thumbnail. The untouched photo's key is the bare fingerprint on purpose, so
caches from before edits existed stay valid. `set_thumb_state_if_unchanged` compares the edit
too, or a worker that rendered the pre-edit picture marks the row `Ready` with nothing cached
under the new key. For an edited photo `ViewerItem` reports `width`/`height`/`orientation` and
`faces` *as shown*. Keys can recur ("Original", a fourth turn), so the thumb handler answers
`immutable` only when the URL's key is the photo's current one and `no-store` otherwise; and
every write of an edit takes `Engine.edit_write`, because a turn reads the edit it builds on.
Full-size renders run one at a time (`protocol.rs`, `RENDERING`), outside the thumbnail pool
that otherwise bounds decode memory, and `neighbours` leaves edited photos out of the preload.

**What reloads the viewer** is `pictureChanged` (`ui/src/lib/picture.ts`), fed by the re-read
the viewer makes on every grid version. A reload blanks the photo, resets zoom and pan and
closes the crop tool, so the comparison must stay exact: `thumbState` counts only across
`failed`. Anything that sends a row back to `pending` (an edit does) would otherwise make the
*next* unrelated change — a star, a scan finishing — reload the photo on screen.

**The window's fullscreen state is persisted** (`WINDOW_STATE_FLAGS`), and the slideshow uses
the window's own fullscreen, so quitting mid-show reopens fullscreen with no title bar. `F11`
(global, `App.svelte`) is the way out and the reason it exists.

**The duplicate finder hashes after the scan, in the engine.** `items.content_hash` (XXH3-128,
NULL for almost every row) is filled by `photon_core::duplicates::hash_candidates`, which reads
only files sharing a byte size with another live file. `Engine::hash_duplicates` runs it at the
end of every `run_scan` - not inside the scanner, so neither of `walk_tree`'s callers can be
forgotten, and because a duplicate is a fact about the whole library. Any write that replaces
a file's fingerprint must set `content_hash = NULL` (today `update_items`); a row that keeps a
stale hash is never a candidate again. `set_content_hash` refuses a row whose size or mtime
moved since the candidate was listed.

There is no `COLLATE NOCASE` anywhere and `lower()` is ASCII-only without ICU (a native
dependency this project does not take), so **case-insensitive matching is done in Rust**, not
in SQL.

### Styling

Almost every colour is a token in `ui/src/tokens.css`, in a light and a dark block selected by
`data-theme` on `<html>` plus a theme-independent scales block for the colours that are never
themed, for two different reasons: `--photo-line`, `--shadow-ink`, and `--scrim` where it lies
over a photo (a face's name plate in the viewer, the dimming outside a crop) are drawn onto the
photo itself, which is never themed either, so there is nothing for a second block to vary;
`--shadow-menu`, `--shadow-dialog`, and `--scrim` where it dims the app behind the Settings
dialog fall on `--surface` or `--chrome`, which *are* themed, but are dark ink by design in both
themes - a shadow or a scrim reads dark against a light UI too. The viewer's black ground is a
literal by design, not an oversight: a
photo is judged against black regardless of theme, so it is the one colour literal
`no-literals.test.ts` allows, and only there, only once. That test also fails on a removed
variable name, a glyph icon, and now (after the reskin's last hardening pass) a named colour
(`white`/`black`) or a `color-mix`/`oklch`/`oklab`/`lab`/`lch` function used as a component
colour, and any `var(--x)` a component reads that `tokens.css` does not declare (`--sidebar-width`,
set by a `style:` binding in `App.svelte`, is the one exception). It does not catch a colour in
an inline `style=` attribute or written in a form its regex does not know. `tokens.test.ts`
holds the palette to WCAG contrast, light/dark parity and the dark-after-light block order
(equal specificity on `<html>`, so source order is what lets dark win). Icons are `Icon.svelte`
over vendored Lucide path data in `lib/icons.ts`; a new icon is copied from `lucide-static` and
its licence is already in `THIRD-PARTY-NOTICES.md`.

The theme blocks match any element, which is how the viewer is dark in both themes
(`data-theme="dark"` on its root). An inherited property set from a token on `:root` (`color`,
`accent-color`) is computed there, in the app's theme, so a themed subtree must set it again on
its own root - which is why `accent-color` is declared again on the bare `[data-theme]`
selector rather than only on `:root`.

The theme choice lives in the `settings` table; `theme-boot.js` applies a `localStorage`
mirror before first paint because the database answers too late, and it is a file rather than
an inline script because the CSP forbids inline scripts. The database wins when the two
disagree. `createTheme` is generation-counted like `LibraryStore`, because the singleton
outlives an App remount.

`[tabindex='-1']:focus-visible { outline: none }` is global, for script-focused containers. A
roving-tabindex widget's items carry `tabindex="-1"` too and would silently lose their focus
ring: scope the rule before adding one. For the same reason a tile's selection ring comes from
`.selected`, not from focus - and it is drawn inside the tile, not as an outline around it,
because the grid scrolls a row flush to the top of its container (ArrowUp, Home, Recent's
first row), which clips anything sitting outside the tile's own box.

The look cannot be tested here, but it can be seen without launching the app: build the UI,
serve `ui/dist` with a script that fakes `window.__TAURI_INTERNALS__.invoke` with canned data,
and screenshot it in headless Chromium (`--screenshot`, with `--force-dark-mode` or not);
thumbnails can be served by mapping `photon.localhost` with `--host-resolver-rules` and a
Windows user agent, since `mediaUrl` uses `http://photon.localhost` there.

## Conventions

- **photon never writes to, moves or deletes photo files.** The one file it writes inside a
  watched folder is Picasa's own `.picasa.ini` (or `Picasa.ini`), through `picasa::set_star`
  only, to set or clear a single `star=` line; every other byte of that file is preserved.
  This narrowed the older "never writes inside watched folders" promise on 2026-09-16 (spec
  `2026-09-16-photon-set-star-design.md`); any further write is a spec-level decision, not a
  code change. The writer and the reader in `picasa.rs` share one line classifier on purpose:
  a writer with its own header/key logic drifts from the reader. Faces and contacts are read
  from the same INI and never written; keywords are read from the photo and never written (the user's renames and
  removals are `tag_rules` rows applied on read, `library/tags.rs`);
  albums and edits (turns and crops) live only in `library.db` (both are by item id, so a
  renamed file leaves its albums and loses its edit when its old row is purged — a recorded
  limitation, not a bug). An edit never touches the photo: it is rendered on the way to the
  screen.
- **No native library dependencies.** Nothing wrapping a C/C++ SDK. This is what made packaging
  tractable on three platforms, and it is why XMP and INI parsing are hand-rolled or pure-Rust.
- **Never launch the GUI to verify a change.** Verification is the test suites plus
  `svelte-check`; anything needing eyes goes on the README's `## Manual smoke checklist`.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not
  proof — it shows a symbol was missing, not that an assertion discriminates behaviour. Tests
  that pass with and without the change have shipped here more than once. When a change
  genuinely cannot have one — a race with no seam, a Svelte effect — say so in the commit
  message and why, rather than adding a test that passes either way. **A probe that passes is
  a finding, not a formality:** it has exposed a missing test (the slideshow's still-loading
  case) and a line whose comment called it load-bearing when it did nothing (an `IS NOT NULL`
  "planner hint"). Revert with an exact replacement; a loose `sed` that also hits a
  neighbouring writer fails ten tests and proves nothing.
- **A large branch gets an independent read before it merges.** Every whole-branch review here
  has found a real bug no test could see; the edits branch's was the viewer reloading on any
  unrelated library change. Point the reviewer at the effect wiring and at anything a new
  feature *arms* in old code, not only at the new code.
- **Read the code before proposing a feature.** "Date search" and camera search were pitched
  as new and already existed as search haystacks; the viewer already had a rotate key.
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

Release commits go straight to `main`. A schema bump makes the release a minor. The generated
notes are a list of PR titles; replace them with hand-written ones in the shape of v0.14.0 and
v0.15.0 — Highlights, Your files are untouched, Upgrading (say when an older photon will refuse
the library, and any behaviour that changed under the user), Not in this release, the unsigned
line, the changelog link. `gh release edit --notes-file` has reported success while changing
nothing: read the body back before `--draft=false`.

Installers are deliberately unsigned. There is no signing identity, no notarization and no
auto-updater; the README explains the per-OS warnings.
