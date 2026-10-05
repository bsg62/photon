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
`.svelte` file cannot be rendered or asserted on. A `.svelte.ts` module compiles there for
Svelte's *server* runtime, where effects never run; a test that needs real reactivity
(an effect, a derived depending on state it reads) is named `*.client.test.ts` and runs in the
`client` vitest project (`ui/vite.config.ts`), still in Node but with the client runtime -
`page-signals.client.test.ts` is why: a lazily created per-page signal passed every
server-runtime test and left every tile deaf to its page. Logic that needs a test goes in a
`.svelte.ts` factory tested with fake timers (`createSearchBox`, `createThumbRequest`,
`createSlideshow`, `createCropTool`) or a pure module (`timeline.ts`, `crop.ts`, `picture.ts`);
what is left in the component is effect wiring, verified by `svelte-check` and the smoke
checklist (`docs/smoke-checklist.md`), not by a test. Layout *can* be measured without the GUI: a static page
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

**Seeing the UI without launching it** (not in CI; needs Chromium on `PATH` or in `CHROMIUM`):

```bash
cargo run -p xtask -- screenshots                     # thirty-five PNGs into target/screenshots/
cargo run -p xtask -- screenshots --only viewer-info-light --no-build
cargo run -p xtask -- scroll-probe   # the end of a 300k-photo library is reachable
```

The project website (`site/`, deployed to photon.webcodr.io by `pages.yml`) uses these
screenshots with real photos: `--photos crates/xtask/screenshots/photos` serves the CC0 photos
there (credited in their `CREDITS.md`) in place of the gradients. Regenerate `main-light`,
`main-dark` and `viewer-info-light` with it and convert them to `site/img/*.webp`.

`npm run dev` runs the app with hot reload. **Do not run it to verify a change** — see
Conventions.

## Architecture

Three crates plus the UI:

- **`photon-core`** — headless. SQLite library, scanning, thumbnails, metadata. Knows nothing
  about Tauri.
- **`photon-app`** — the Tauri shell. Owns `Engine`, the IPC surface and the custom protocol
  that serves thumbnails.
- **`xtask`** — repository chores, and `screenshots`. Binary only, no `lib.rs`.
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
checks are what make that safe, and both are needed - each has a test that fails only when
that guard is removed, which is worth keeping that way: the epoch's own window (the state
moved, its rebuild not yet landed) went untested for months while the `seq` stamp quietly
covered every scenario written for it. A setter's own rebuild is the authority
for a view change. A discarded rebuild never loses rows: every commit is followed on its own
thread by a rebuild stamped after it, and the highest stamp always publishes.

`Engine::open` does not build the grid: it publishes an empty index at `NOT_BUILT` (version 0,
which no real publish reaches, since each adds one) so `setup` - on the main thread, before the
webview can load - costs a library open, not a whole-library query. `startup` builds it first
thing. The UI treats that version as "not known yet" (`grid-state.ts`): no empty-library
notice, no photo count, and no last-folder restore, whose `len === 0` guard waits for it.
`open` still fails on a library the grid query cannot run against, through
`check_grid_query`, which prepares the query without running it. A first build that fails
anyway is retried (`FIRST_GRID_BACKOFF`) and then published *empty* (`build_first_grid`):
left at `NOT_BUILT` the window drew nothing, and an unchanged library rebuilt only on a view
switch. The empty grid carries the error (`GridInfo::build_error`), so the UI says the
library could not be read instead of "No photos yet"; it is never published over a grid something
else built meanwhile, and the next successful publish clears it.

`LibraryChanged::data_changed` tells the UI whether to refetch the sidebar's collections
(albums, people, tags - the tag counts alone are ~220ms at 300k photos carrying three keywords
each; `tag_counts_100k` measures a third of that library). Every `refresh_grid`
marks the engine-wide `data_dirty` before it snapshots; the view setters' rebuilds
(`rebuild_or_restore`) do not, and neither does `refresh_grid_derived`, the rebuild after a
stored poster frame and after the hashing passes in `hash_after_scan` - a thumbnail state, a
content hash or a look-alike group is read by the grid and `GridInfo` (the Duplicates count),
never by an album, person, tag, tag rule or folder count, and frames arrive up to once a
second. Whichever rebuild *publishes* next swaps the flag back and sends it - except a failed
first build's empty stand-in, which announces a data change without taking the flag, so the
build that finally succeeds still carries it. Engine-wide
rather than per rebuild, because a data rebuild discarded by a view switch would otherwise
take its flag with it. A new writer calls `refresh_grid`; `refresh_grid_derived` only for a
write that no collection or Settings query reads, checked against those queries. A command
that has committed its write rebuilds through `refresh_after_write`, which logs a failed
rebuild rather than returning it: returned, the UI undid its side of a write that stands (a
star flipped back, a selection kept for a retry), and the flag the failed rebuild left set
has the next publish announce the write anyway.

`grid_info` sends the sections and folders only when their `layout_gen` - published with the
grid, moved only when a publish changes them - is not the one the UI names, and reads its four
counts from a cache keyed by `counts_epoch`. `data_snapshot` moves the epoch beside
`data_dirty`, and `hash_after_scan` moves it before each of its rebuilds (the Duplicates count
reads both `content_hash` and `similar_group`); a new writer that
changes something a count reads without going through `refresh_grid` must move it too, or the
sidebar shows a stale count until the next data write.

`ScanReport::touched_rows` gates the end-of-scan refresh on whether a scan actually moved
rows. A change that alters data by some *other* means must add its own counter to `ScanReport`
and fold it into `touched_rows`, or the grid silently never rebuilds. Today the counters
beyond the obvious four are `restarred`, `refaced` (Picasa faces), `rehidden` (Picasa's
`hidden=yes`), `realbumed` (Picasa's albums), `enriched` (the metadata backfill) and `moved` (a
row re-pointed to its file's new path, below).

**Grid order** (`items.rs`, `GRID_ORDER`) is the folder's oldest photo descending, then each
folder's photos oldest to newest. The sidebar groups by the same value, so the list is an index
of the grid. Changing one without the other splits them onto different axes. `GRID_ORDER` reads
columns only `folder_order(shown, filter)` supplies, and `grid_query(select, shown, filter)` is the one place
the two are paired: in a filtered view (Starred) the driver's filter must equal the outer
`WHERE`, so a folder is placed by its oldest *matching* photo, which is what keeps the sidebar
and the grid agreeing. A query assembled by hand with `GRID_ORDER` and no driver compiles and
fails at `prepare`, only when that view is opened.

**The user's sort** (`photon_core::sort`, `ViewState.sort`, the `grid_sort` setting) is applied
*after* the view's query, in Rust, by `Library::sorted_entries`. Date is the view's own order
(`GRID_ORDER`, or Recent's), untouched; `reverse` turns the whole list over, so folder runs stay
contiguous. Modified, name and size sort every photo together and lay the grid out flat
(`Sort::layout`), which has no headers, no timeline, and a folder jump that lands on the
folder's first photo (`offset_of_folder`); the sidebar then drops its year groups and orders
folders by `FolderTally`'s `bytes`/`modified_ms` or by name (`arrangeFolders`). The sort is part
of the `ViewState` a rebuild snapshots, so a sort change is guarded by the epoch like a view
switch; do not read it at query time instead. `GridEntry` carries `size`/`mtime_ms` unserialized
for it, and a name sort reads the names in a side query, because `GridEntry` is `Copy`.

**Hidden photos** (`library/hidden.rs`, schema 13) are in no view but `GridView::Hidden`.
`grid_query` takes a `Shown` argument (`Visible`, `Hidden`, or `Either` for bookkeeping like the
thumbnail queue) so every caller has to say which set it wants; the queries that do not go
through it - Recent, every sidebar count, `duplicate_ids!`, `copies_of`, `similar_of` - each
carry `hidden = 0` themselves, and `library/hidden.rs`'s tests pin every one. A new
user-visible query must filter on it too, or photos the user put away turn up again. Hidden
photos are still scanned, thumbnailed and hashed, so unhiding is instant. **Hide folder** (schema
16) is `folders.hidden` plus `items.hidden` on its photos, and `insert_items` gives a new row its
folder's flag - so visibility stays that one column and no query joins `folders` to decide it.
Any other writer that creates item rows must inherit the flag the same way, and
`set_folder_hidden` writes missing rows too, since `update_items` revives a row without touching
`hidden`. `move_items` (below) does: it writes `MAX(hidden, the destination folder's)`, so a
hidden folder hides what arrives in it and a photo the user hid never comes back out by moving.
**It also forgets `picasa_hidden` for a row that is hidden once that write is done.** The
column is the INI's answer for the name the file had in the folder it was in, and the hidden
pass follows a *change* in it (`apply_folder_hidden`): renamed, or moved without its INI, the
photo has no line where it is now, and against a kept "yes" that read as Picasa un-hiding it -
a photo hidden in Picasa came back visible by being renamed
(`a_photo_hidden_by_picasa_stays_hidden_when_it_is_renamed`,
`a_photo_hidden_by_picasa_stays_hidden_when_it_is_moved_alone`), and a visible one moved into a
hidden folder was un-hidden there
(`a_photo_moved_into_a_hidden_folder_is_not_unhidden_by_the_ini_it_left`, the reason it is the
flag *after* the write that is asked, spelled out a second time in the SQL because a SET reads
the row as it was). Forgotten, the pass makes a first read, which follows a `hidden=yes` and
ignores a missing line. A row that stays visible keeps its answer, which is the whole record of
an unhide made in photon: forgotten too, a folder renamed with its INI hid the photo again
(`a_photo_unhidden_in_photon_stays_visible_when_its_folder_is_renamed_with_its_ini`;
`a_moved_row_forgets_picasas_answer_only_when_it_is_hidden` is the column's own test). The cost:
a photo hidden in Picasa, then renamed and un-hidden there between two scans of photon, stays
hidden until it is unhidden in photon.

**Parameterised views.** `GridView::Search`, `Person`, `Album` and `Tag` are selected by an
argument held beside the view in `ViewState.arg` (the query, a person's key, an album id,
a keyword). `entries_for(view, arg)` interprets it per view and binds it as a SQL
parameter; `GridInfo` reports it in typed fields (`searchQuery`, `person`, `album`, `tag`)
so the TS mirror stays explicit. `set_view` clears the argument unless it is re-entering the
same parameterised view, so a query can never be read as a person's key. The membership
views filter the driver as well as the outer `WHERE`, the same way Starred does, or the
sidebar's year groups and the grid disagree.

**Search** is one view, not a family of them. `search_entries` builds the haystacks — file
and folder name, make, model, lens, `50mm`/`f/1.8`/`iso400`, keywords through `EFFECTIVE_TAGS`, the caption,
and the capture date as `YYYY-MM-DD` — and `search::Query` holds the grammar: words AND,
capitals-only `OR`/`AND`, quotes, `camera:`/`lens:`/`tag:`/`person:`/`album:`/`folder:` each
confined to its own field, `is:`/`has:`/`near:`/`on:`/`faces:` asking about the photo rather than its text
(`has:face` and `faces:N`/`faces:N+` count its faces, below), `size:`/`iso:`/`aperture:`/`focal:`/`mp:`
comparing a number of the photo's (`Term::Number`, inclusive bounds in whole units, so every
spelling of one comparison is one term), a
leading `-` negating a token, dangling pieces ignored. A fact a term reads is a field of
`Haystacks` set per photo in `search_entries` and cleared in `Haystacks::truncate`; one set
only inside an `if let` (the caption's) and not cleared there carries over to the next photo. People, albums and the face counts are read from side
tables only when a query names one (`Query::needs`). A new searchable fact is a haystack there, not a view; a new *filter* is a
prefixed term. The query string is the whole interface, so UI links (the info panel's camera
and lens) go through `searchBox.search()`, which cancels a pending debounce first.

**A grid offset is only meaningful against one index version.** Indexing a photo into a
folder that sorts earlier shifts every later offset, so anything holding an offset across a
rebuild — the viewer, the grid selection — must re-find its photo by id through
`grid_offset_of_item`. Clamping catches only the offset falling off the end; in range the
consumer silently shows a different photo.

**The grid's `scrollTop` is the layout's position, not the viewport's.** Engines cap a box
at 33,554,428 px (less in CSS px on a scaled Windows display), and a large library at large
tiles in a narrow window is taller; past the cap `.canvas` is held at a measured
`domHeight` and `scroll-map.ts` maps the layout's position onto it, drawing rows at
`row.top - shift`. Under the cap the map is the identity. Anything new that reads or writes
`viewport.scrollTop` in `Grid.svelte` goes through the map (`virtualAt`, `scrollToVirtual`,
`writeDom`) - a direct read is a DOM position, and in a big library it names another photo.
`cargo run -p xtask -- scroll-probe` checks the end of a 300,000-photo library is reachable.

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
today is `apply_picasa`, which applies stars, faces, hidden flags *and* albums from one read
of the folder's INI. It finds that INI from the walk's own listing (`read_folder_listed`), not
by listing the directory again - but a listing is a snapshot, and a star photon writes during
a long scan lands after it, so an INI the walk did not see is still probed for, and one it
did see and is gone falls back to `read_folder`; so does any folder whose listing the walk
could not complete. A pass that trusted the listing alone would zero that star. The hidden flag is followed on *change* (`items.picasa_hidden` records
the INI's last answer), not mirrored like a star: photon never writes `hidden=`, so a mirror
would undo every unhide in photon on the next scan.

`apply_picasa` has a third caller: `refresh_picasa`, the INI pass. The watcher reports a
folder whose only changed files are its INI (`picasa::is_ini_write`, which also knows photon's
own temporary) in `Changed::ini_dirs`, and those are reread without a walk under the same scan
slot, queued in `Pending::ini`, which has no cap - photon starring across many folders and a
Picasa album rename used to overflow the walk queue into a rescan of the whole root. A
rename of the writer's temporary name must change `is_ini_write` with it, or every star
becomes a walk again.

**The metadata backfill.** `items.exif_version` records which generation of
`read_image_meta` last read a file; `metadata::EXIF_VERSION` is the current one. An
unchanged file whose stored version is behind is re-described and written through
`update_item_meta` (camera columns, keywords, `taken_at`, the version; not the fingerprint
columns, not `rating`). `taken_at` is included because a capture date outside 1970..tomorrow
is refused (`plausible_taken_at`) and the backfill is the only way an unchanged file is
re-dated. Adding a field to `describe()` without bumping `EXIF_VERSION` leaves every
existing photo without it forever. The position (`gps_lat`/`gps_lon`, schema 23, read
from the EXIF GPS IFD by `read_gps`) rides in `CameraMeta`, so the four item writers
(`move_items` is the fourth, below) carry it with the camera columns. Keywords come from the file (XMP `dc:subject` and IPTC
2:25, `keywords.rs`) into `item_tags`; every writer of an item row goes through
`write_tags`. Every *reader* of keywords goes through `EFFECTIVE_TAGS` or `TAG_FILTER` in
`library/tags.rs`, which apply the user's rename/remove rules; a reader of `item_tags` that
bypasses them shows tags the user renamed or removed.

**The walk does not enter a recycle bin**, nor anything with a dot-name. One predicate,
`passed_over`, for the two places that test a name: `walk_tree`'s `filter_entry`, and
`scan_subtree`'s check of the path below the watched root. The second is there because the
watcher hands `scan_subtree` the directory an event came from and `walk_tree` exempts its own
depth 0: without it a subtree scan indexes what every full scan skips, marks missing and purges
(`subtree_scan_of_a_hidden_directory_indexes_nothing`,
`subtree_scan_of_a_recycle_bin_indexes_nothing`). The bins are `RECYCLE_BINS`: `$RECYCLE.BIN`
and XP's `RECYCLER` (inside the root when a drive's root is watched), Synology's `#recycle` and
QNAP's `@Recycle` (inside it when the share is), matched as a whole component, without ASCII
case, at any depth; `.Trashes` and `.Trash-1000` are dot-names already. They are skipped
because of the feature below: deleting in Explorer or over SMB is a rename on the same volume
that keeps size and mtime, so the row was followed *into the bin* and the deleted photo stayed
in its albums and person views until the bin was emptied
(`a_photo_deleted_into_a_recycle_bin_leaves_the_library`). The watched root itself is never
passed over, whatever it is called (`a_watched_root_named_like_a_recycle_bin_is_scanned`; the
exemption had no test before it). A photo an earlier photon indexed inside a bin is no longer
found, so the next two scans that reach the folder above it mark it missing and purge it:
intended (`a_photo_indexed_inside_a_recycle_bin_by_an_earlier_photon_is_purged`).

**A renamed or moved photo keeps its row** (spec `2026-10-05-photon-follow-moved-files-design.md`,
schema 26). Everything photon keeps about a photo hangs on its `items` row, and a path the walk
does not find is marked missing and purged, while the path it has never seen is a new row with
none of it - which cost a reorganised folder its albums, edits, names, and the hours of face
work. So when the walk is about to insert a file, it first asks which row that file used to be.
The rule is `moved::pick` (pure: the filesystem arrives as a closure) and `moved::Probe` (the
stats): `Library::move_candidates` finds the rows with the file's size, mtime and kind
(`items_moved`, a plain index on `(size, mtime_ms)`, not partial, so it is outside the
partial-index tie in Schema; `the_move_lookup_is_served_by_its_index` is its plan test); `pick`
then wants the same width, height and `taken_at` (the date ignored for a row an older reader
described), the old file gone, and exactly one row left, the file name breaking a tie. Gone is
`NotFound` and nothing else - a permission error or a timed-out share is "cannot tell" - or the
old path still opens something that `paths::canonicalize` resolves to the new path, which is
a case-only rename on macOS and Windows, or a file or folder replaced by a symlink to where it
went. By what the old path *resolves to*, not by `paths::same_path`, which folds case by
platform and not by volume: on a case-sensitive volume under macOS two identical files `a.jpg`
and `A.JPG` would have traded one row back and forth on every scan. The drive must be there
(rule 4 of the five the spec numbers: the row's watched folder online, its root a non-empty
directory, checked once per watched folder per scan), because an unplugged or unmounted drive
answers `NotFound` for every file on it - `an_offline_or_empty_root_gives_no_candidates`. The
exception is the folder being walked, whatever its stored flag: the flag is written only after
the walk, and without it a drive that came back with renamed files had every rename inserted
as new and its row purged (`a_drive_that_comes_back_with_a_renamed_file_keeps_its_row`).

Two bounds on the rule. **More than 32 rows of one size, mtime and kind are no candidates at
all** (`MOVE_CANDIDATE_CAP`: the lookup reads 33 and then answers none, before anything is asked
of the filesystem). Each row that fits costs a stat, so in a cluster of identical files -
uncompressed frames, placeholders, a dataset unpacked with one mtime - every file asked about
every other: 4,000 of them took 20 s to import and 41 s to scan after a rename, against 0.4 s
and 0.5 s with the cap. The price is that a file moved inside such a cluster is a new photo
(`a_move_is_followed_among_identical_rows_only_up_to_the_cap`). The `LIMIT` bounds the rows
read and changes no outcome; the count check after it is what the tests can see. **And a read
that fails is an error, not a "no"**: `Probe::gone` and `pick` are fallible, and a failed read
of the watched folders fails the scan before `finish_mark_purge`, which loses nothing.
Swallowed, it read as "no drive is there": every renamed file of the walk was inserted as new
and the same scan marked the old rows missing, with nothing logged
(`a_scan_that_cannot_ask_which_drives_are_there_fails_before_it_calls_a_file_new`). A watched
folder that is merely not listed, removed meanwhile, is a state and stays "not there".

It runs in `flush_new` (`find_moves`, then `apply_moves`, then the insert of what is left), which
is `walk_tree`'s, so `scan_watched` and `scan_subtree` both have it; the watcher reports a move as
its two directories, scanned separately in either order, so a rule that paired a walk's vanished
and new files would have caught only moves whose ends lie in one walk. `find_moves` is two
passes, so a row is claimed by one file: the file that kept the row's name has it and the other is
new, within one batch (`of_two_files_that_fit_one_row_the_one_with_its_name_has_it`); across
batches the first file met has the row, and no row is ever picked for two files
(`a_row_is_claimed_by_one_file_and_the_next_takes_the_one_left`), nor guessed between when two fit
(`two_rows_that_fit_are_not_guessed_between`). `Library::move_items` is `update_items`' opposite:
the file is the same, so the hashes, the look-alike group, the detections, `face_version`, the
edit, the rating and every row hanging on the id stay
(`move_items_repoints_the_row_and_keeps_what_hangs_on_it`). What it writes is the place;
`missing_since = NULL`, so a row the source's scan had already marked missing is revived with
its detections, where `update_items` would have deleted them; the metadata just read, the
file's own keywords among it (`write_tags`); `thumb_state = 0`; `hidden` and `picasa_hidden` as
Hidden photos describes; and the GC epoch. A copy is not a move - its original is still there,
so rule 2 (the row's file is gone) refuses it (`a_copy_beside_the_original_is_a_new_row`).

Two hazards come with it, both closed by `mark_missing_at` and `purge_at`, which write only a row
still at the path the walk's `known` has for it and report the rows changed. The walk's own
`known` still lists a followed row at its old path, which the walk will never find, so by id alone
the scan that had just followed a move marked it missing - or, when an earlier scan of the folder
it left had marked it already, purged it with everything on it
(`a_row_already_missing_is_not_purged_by_the_walk_that_follows_it`). And a scan of another
watched folder, each having its own slot, may have read `known` before the move was written (the
stale writes in `a_move_is_followed_when_only_the_destination_is_scanned`). The guard covers
those two writes only: `update_items` and `update_item_meta` still write by id from the same
list, left so because they need a file back at the old path of a row followed elsewhere, within
one walk. The same race has `move_items` answer `false` for a row no longer at its old path
(purged or claimed since the lookup), and `apply_moves` hands that file back for the insert:
dropped, it would stay out of the library until some later scan
(`a_file_whose_row_went_away_is_inserted`, on `apply_moves` directly, a race having no seam).
`ScanReport::moved` counts the rows and is in `touched_rows`; `apply_moves` logs each followed
row at debug level with its id and both paths, and `run_scan` logs a scan that touched rows at
info with its counters, because a row followed to the wrong file shows nowhere else.

**The thumbnails come along.** The key is made of the path, so the moved row names thumbnails that
are not cached, and re-rendering a renamed folder of 5,000 photos would also 404 the People
page's face crops until it was done. `ScanSink::moved(&[(old_key, new_key)])` has the engine's
reporter call `ThumbCache::rename` for each pair (a `fs::rename` per size, a missing one
skipped, a failure logged and costing one render, never a wrong picture); the keys are
`Item::thumb_key()` before and after, so an edited photo's are the ones carried. It is called
before `indexed` for the same rows: the row is `Pending` regardless, and the worker that then
reaches it finds the files and marks it `Ready` without decoding. `apply_moves` makes both
reports before it touches folders, which can fail: the rows are already re-pointed, and an
error there must not leave their thumbnails neither carried nor queued
(`a_failed_folder_inheritance_still_reports_the_rows_it_moved`).

**A renamed folder keeps its name and Hide folder flag, by a narrower rule than it looks.** A
folder row is found by path, so a renamed directory is a new row; `inherit_folder_flags(from, to)`
hands the old row's `alias` and `hidden` to the new one when the old directory is gone (or is
the new one under another spelling, `moved::vacated` again), the new folder has neither of its
own, and **the walk created its row** (`FolderRows::created`: a directory it had no row at the
path of). Without the last, photos of a hidden, named folder moved into a long-standing folder
whose old directory was then deleted hid and renamed it
(`photos_moved_into_a_folder_that_was_already_there_do_not_rename_or_hide_it`). The cost to state
plainly: the name and flag are lost whenever an earlier scan made the new folder's row before the
old directory was gone. The commonest is a folder made first and filled afterwards: "New
folder" in the file manager, which the watcher scans two seconds later, then the photos dragged
in and the old folder deleted - the photos keep their own hidden flag, but the folder has no
name and is not hidden, so a photo added to it later is visible. The others: a folder moved file
by file across volumes with the watcher scanning mid-move, a child's subtree scan seeding its
parent (`seed_ancestors`), a walk that errored after `ensure`, a first scan cancelled before a
flush. An alias equal to the new directory's own name is not carried, by the comparison
`set_folder_alias` stores NULL by (`repeats_name`): copied, it stuck, and the directory renamed
once more was still shown under the name it had left
(`an_alias_that_is_the_new_folders_own_name_is_not_inherited`). A folder row that is gone by
then gives `Ok(false)`, not an error (`a_folder_row_that_is_gone_gives_and_takes_nothing`): the
scanner calls it after `from` may have been emptied and pruned by another watched folder's scan.
A flag is taken through `set_folder_hidden`, so photos of the new folder inserted before the
move was noticed are hidden with it (and no order here is load-bearing: inheriting before the
batch's insert, like asking the root before the stat, saves one call and changes no outcome).

**What does not follow, on purpose.** A photo moved out of the library and back later (its row is
purged by the second scan that misses it; keeping vanished rows for a period was declined for
now), a copy made first and the original deleted later (at the copy there was no move; at the
deletion nothing is new), a file edited and moved at once (its size and time differ), and Picasa's
data except with the folder's INI - `apply_picasa` mirrors the INI of the folder the photo is in
*now*, under the name it has *now*, so a photo moved alone loses its Picasa faces and albums.
**And its star, one set in photon included, which a rename in place loses as well**: the star is
the INI's `star=` line under the file's name (`set_star` writes it there) and `rating` mirrors
the INI, so the new name has no line and the pass clears it
(`a_photo_moved_without_its_ini_loses_its_picasa_star`), while a folder renamed or moved takes
its INI along and keeps every star (`a_folder_moved_with_its_ini_keeps_its_stars`). Carrying it
would be the scanner writing `star=yes` under the new name, which Conventions makes a spec-level
decision; it has not been taken. Three more, by rule: moving every photo out of a watched
folder into another (the emptied root is what an unmounted volume leaves behind - to rule 4
when it has no entry at all, and to the scanner's own guard, which marks it offline, when it
has no photo and its scan comes first - so the photos arrive as new rows and the old ones stay
as an offline drive's); a move to a filesystem with
coarser timestamps, FAT's two seconds or exFAT's ten milliseconds (the mtime is not the one
stored); and a volume mounted inside a watched folder and unplugged on its own (rule 4 looks at
the watched root only, so its files are `NotFound` under a root that is there: they are marked
missing and purged as deleted files are, and a copy of one turning up elsewhere meanwhile is
taken for it). A case-only rename on a case-insensitive mount under Linux (vfat, exFAT, CIFS)
is a fourth: `realpath` does not fold case there, so the old spelling resolves to itself, rule 3
answers "another file", and the rename is a new row and a purge. **A move between volumes is a
copy then a delete**, so if the watcher scans the destination while the originals are still
there the copies are new photos, and when the originals go their rows are purged. A single file
is normally followed (both halves land inside the watcher's two-second debounce, `DEBOUNCE`); a
long folder move between drives while photon runs may be followed in part. Two tests,
`a_case_only_file_rename_keeps_its_row` and
`a_case_only_folder_rename_keeps_its_rows_its_name_and_its_hide_flag`, are compiled everywhere
and ignored on Linux (`cfg_attr(..., ignore = ...)`): no filesystem a Linux runner has can make
the case-only rename. They run in CI on macOS and Windows and are the only tests of rule 3's
positive half (the old path opens this very file) on a case-insensitive filesystem - do not
un-ignore or delete them.

**The watcher drops access events** (`watcher/fs.rs`, `may_have_changed`). On Linux, notify
registers for inotify's open and close events, so reading a file's EXIF, listing a
directory or walkdir entering one all arrive as `Access` events on that directory. A scan
does all three to every directory it walks; treated as changes they scheduled the next
subtree scan of the same directories two seconds after every scan, forever. Anything that
makes the watcher react to more event kinds must keep a scan's own reads out. The debouncer
runs with `NoCache` on every platform, not its `RecommendedCache`: on Windows and macOS that
is a file-id map which walks every root and opens (Windows) or stats (macOS) every file under
the debouncer's lock, again on every rescan flag - minutes on a network share - and it only
stitches a rename's two halves together, which photon never needs, since each half is reduced
to a directory to rescan either way.

`skip_mark_purge` is set by a walkdir error carrying **no** path — a mid-iteration `read_dir`
failure (walkdir `lib.rs:1026`), not something the filesystem can be made to do on demand. An
unlistable walk root sets it too, but `read_stars` lists the directory as well, so that
branch's star behaviour cannot be tested from the filesystem either way.

**Every stored path is `paths::canonicalize`, never `dunce::canonicalize` or
`fs::canonicalize`.** dunce strips the verbatim `\\?\` prefix from disk paths only, so a
network share canonicalizes to `\\?\UNC\server\share\...`: a form the Windows shell rejects
(`ILCreateFromPathW` returns null, which is Reveal failing with "failed to convert path to
ITEMIDLIST") and nobody recognises as their share. `paths::simplified_unc` rewrites it to
`\\server\share\...`, and migration 10 rewrote the three path columns of libraries indexed
before it. The form has to be the *same everywhere*: `paths::same_path` compares
component-wise, so one path in each form is two different folders to photon - a re-added
root rather than a recognised one, and a `scan_subtree` whose `strip_prefix` misses its own
watched root. `paths::overlaps` is symmetric - it answers "do these two collide",
which is the question `add_watched_folder` asks. "Is this inside the library" is
`paths::is_within(child, root)`: an export destination checked with `overlaps` refuses a
folder that merely *contains* a watched root, and then says the opposite of what is true.

### Schema

`library/schema.rs` holds `MIGRATIONS: &[&str]`, one entry per version, each run in its own
transaction with `PRAGMA user_version` bumped after it. A library from a newer photon is refused
with `SchemaTooNew`. SQLite runs in WAL mode, so `library.db` has `-wal`/`-shm` siblings.

A schema bump breaks tests on purpose: `library/mod.rs` asserts the literal version number
twice (the opened version and `SchemaTooNew`'s `supported`) and the table count once, and the
migration tests seed from `MIGRATIONS[..N-1]`. Update the numbers rather than loosening them
to `MIGRATIONS.len()`; the hardcoding is the tripwire. An index that serves a
specific query gets a plan test (`the_recent_view_is_served_by_its_index`), so drift between the
index and the `ORDER BY` fails rather than silently regressing. A query that reads *every* live
photo writes `+missing_since IS NULL`: photon never runs `ANALYZE`, and without statistics the
bare term makes SQLite walk a partial index on that predicate - `items_size`, in random table
order, 7-12x slower at 300k photos. `library/mod.rs` has the reasoning; each such query has a
plan test, and a new one needs its own.

The same missing statistics make **partial indexes tie**: without `ANALYZE` SQLite costs every
partial index the same, a constant reduction whatever its predicate, and on a tie takes the
one created *last*. A view's grid query is safe by shape: its `folder_id` group key and
equality pick the `(folder_id, taken_at)` index holding its rows. A count is not: a scan with
no `folder_id` term ties with every partial index whose predicate its WHERE implies, and wins
only by creation order. Today `starred_count`, `video_count` and the Hidden count beat
`items_pending`, `items_size` and `items_recent` that way, so a later partial index whose
*predicate* those counts' WHERE implies - a new `WHERE missing_since IS NULL` index, say -
takes them over; `the_starred_and_video_counts_are_served_by_their_indexes` (and Hidden's plan
test) is the tripwire. The counts are cached (`counts_epoch`), so a flip costs a cache refill,
not every grid version. The same tie is why the All view has no `items_visible`
(`missing_since IS NULL AND hidden = 0`, same shape as the view indexes, schema 22): every
visible view's WHERE implies it, and created after `items_starred` and `items_videos` it took
Starred and Videos over, the whole library read for a grid of 3%.

**Reads are pooled, writes are one connection.** `Library::reader()` returns a `Result` and never
waits on another reader: it hands out an idle pooled connection or opens one (at most eight are
kept). `writer()` is a single mutexed connection.

**Thumbnail garbage collection is gated.** Any write that can orphan a thumbnail — deleting an
item, or changing anything the thumbnail key is made of (`path`/`size`/`mtime_ms`, the
fingerprint columns, and `edit_turns`/`edit_crop`) — must call
`settings::bump_thumb_gc_epoch` inside its own transaction. Today that is `purge_items`,
`purge_at` (the scanner's purge by path), `update_items`, `remove_watched_folder`,
`set_item_edit` and `move_items` (a moved row's old key is garbage). The tripwire test in
`settings.rs` enumerates those six, so a *new* orphaning write is not caught
automatically; `apply_moves` returns before calling `move_items` for a batch with no move, since
the epoch moves even for no rows and every import would make the walk of the cache due
(`an_import_that_follows_no_move_orphans_no_thumbnail`); the seven-day `THUMB_GC_MAX_AGE`
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
`faces` *as shown*. A key names one picture, so the thumb handler serves a
thumbnail already cached under the URL's key from the key alone, with no database read, as
`immutable`; only when none is cached does it look the photo up, and since that answers with
the photo's *current* thumbnail and keys recur ("Original", a fourth turn), it is `immutable`
only when the file served is the URL key's own and `no-store` otherwise; and
every write of an edit takes `Engine.edit_write`, because a turn reads the edit it builds on.
Full-size renders run one at a time (`protocol.rs`, `RENDERING`, which export shares - held
across the render and never across the write), outside the thumbnail pool
that otherwise bounds decode memory, and `neighbours` leaves edited photos out of the preload.

**What reloads the viewer** is `pictureChanged` (`ui/src/lib/picture.ts`), fed by the re-read
the viewer makes on every grid version. A reload blanks the photo, resets zoom and pan and
closes the crop tool, so the comparison must stay exact: `thumbState` counts only across
`failed`. Anything that sends a row back to `pending` (an edit does) would otherwise make the
*next* unrelated change — a star, a scan finishing — reload the photo on screen.
`pictureChanged` takes a `Pick` of `ViewerItem` that leaves `faces` and `unnamedFaces` out on
purpose: a detection landing on the photo on screen must not reload it. The type is the whole
defence - there is no test, because a field the `Pick` cannot see cannot be compared.

**The window's fullscreen state is persisted** (`WINDOW_STATE_FLAGS`), and the slideshow uses
the window's own fullscreen, so quitting mid-show reopens fullscreen with no title bar. `F11`
(global, `App.svelte`) is the way out and the reason it exists.

**The duplicate finder hashes after the scan, in the engine, in two passes.** `items.content_hash`
(XXH3-128, NULL for almost every row) is filled by `photon_core::duplicates::hash_candidates`,
which reads only files sharing a byte size with another live file - this finds byte-identical
copies. `items.percep_hash` (a 64-bit difference hash, `photon_core::similar`) and
`items.similar_group` (a union-find id) find look-alikes - the same picture after a resize or
a re-save - and are filled by `photon_core::similar::update`. **The hash only nominates a
pair; the pixels decide it.** At the hash's 9x8 resolution a second shot of the same pose *is*
the same picture, so `group` unites a pair within the distance only when `same_picture`
agrees - both cached grid thumbnails reduced to 32x32 greyscale, mean removed, mean absolute
difference at most `SAME_PICTURE_MAX_DIFFERENCE` (measured: copies at or under 2.3, same-pose
second shots 19 and up). The reductions are cached in the engine's `hashing` lock between
passes, and confirming honours `cancel`: a cancelled regroup writes no groups. The same lock
holds a digest of the last finished regroup's input (distance, and every `(id, hash, thumb_key)`)
and of the groups it stored; a pass whose input and stored groups both still match skips the
regroup, which is what keeps a no-op scan from banding the whole library. Candidates come
from four 16-bit bands, each bucket also compared with those one bit away, which is exact up
to `EXACT_RECALL_DISTANCE` (7) - Conservative. Treating the hash as the verdict brings the
false pairs back. Both passes run inside
`Engine::hash_after_scan` (once `hash_duplicates`), requested through `request_similar_pass`
at the end of every `run_scan` but one that
finds its root still offline (the 30-second poll of an unplugged drive, or the startup scan of
one), which read and changed nothing. The pass runs on its own thread, after the scan has
released its slot: inline, it held back `startup`'s `start_watcher` for the session's first
whole-library regroup. Every pass is a thread spawned by `spawn_pass` and counted in
`background_passes` (the face pass's too, below), which is what `shutdown` (`stop_passes`,
bounded) and a test's `Fixture::settle` (`wait_for_passes`, unbounded) wait on - a test that
reads hashes or groups after a scan waits with `settle`, not `wait_for_scans`. They also run, through
`request_similar_pass`, whenever the thumbnail queue has stayed quiet for `THUMB_HASH_SETTLE`
after making new thumbnails ready (`start_thumb_hashing`, `ThumbQueue::wait_drained`), since a
scan's own pass runs while the queue it fed is still rendering and its new photos otherwise
waited for the next scan. Not inside the scanner, so neither of `walk_tree`'s callers can be forgotten, and because a duplicate or a
look-alike is a fact about the whole library, not about one changed file. The perceptual hash
is taken from the photo's **already-cached grid thumbnail**, not from the source file: the
thumbnail renderer is skipped whenever a thumbnail is already cached, so a hash computed inside
the renderer would never run for a single photo in an existing library, only for ones rendered
after the feature shipped. Reading the cache instead means an upgraded library fills in for
every photo whose thumbnail already exists, and a photo is never decoded a second time just to
be hashed. Any write for a file whose *content* changed (its size or mtime) must set
`content_hash = NULL` (today `update_items`, which also clears `percep_hash` and
`similar_group` - a rewritten file has lost whatever picture those described); a row that keeps
a stale hash is never a candidate again. A write for a file that only *moved* is the opposite
case: `move_items` changes the path, and with it the fingerprint and the thumbnail key, and
keeps all three on purpose, the bytes and the picture being the same.
`set_content_hash` refuses a row whose size or mtime moved since the candidate was listed.
`set_item_edit` clears `percep_hash` and `similar_group` too, for the same reason it clears the
thumbnail: a look-alike is a fact about the photo *as shown*, and an edit changes what that is.

**Face detection is a third pass, off until the user switches it on.**
`Engine::request_face_pass` runs `photon_core::face_detect::pass::run` on a thread from
`spawn_pass`, so it is counted in `background_passes` with the look-alike pass and `shutdown`
and `settle` cover it with no second mechanism. It is requested in five places: at the end of
`run_scan`, on the thumbnail queue's drain, by the switch itself, once by `startup` after
the first grid is built, and after every correction of the People data (`write_people`,
below); with the switch off the request is a no-op. That is not everywhere the
look-alike pass is requested: `write_edit`, `remove_folder` and `set_similar_distance` request
that one and no face pass - an edit reaches the face pass through the drain, once the edited
photo's thumbnail has been remade. The drain is the trigger that matters for a new photo, which
is a candidate only once its preview exists; the scan's is the only one a running photon gets
for a library whose thumbnails are all cached; and `startup`'s is what resumes an unfinished
pass on a launch where every root is offline, whose scans request nothing. A pass asks each
step for one row of work before it loads a model (`face_work`, in `run_face_pass`, which names
the first step that has some; only detection, which makes faces, has the pass ask the next
question again): with none it sends its last event and leaves - a probe per step (a photo to
detect and a face to embed, each a walk of its whole table when there is none; a face to
group, an index search over the faces with no group), the last event's count, and no model
load, which is what keeps the request after every scan cheap (a library holding a photo whose
preview can never be read never gets this path, that photo being always a candidate) - and a
detection or embedding step with work reports `running` at once, the next report being a
whole batch away. A pass the quit ends skips its
rebuild and its last count. The
pass reads each photo's **cached 1600 px preview**, never the source, for the reason the perceptual hash reads the grid thumbnail: inside the
renderer it would never run for a photo whose thumbnail is already cached. So its candidate
list has no `online` term, unlike the look-alike pass's: an unplugged drive's photos are
detected from photon's own cache. **The detector (YuNet, through `tract`) runs twice per
picture**, at a 1280 input (`INPUT`) and at 320 (`CLOSE_UP_INPUT`), and the two runs' faces go
through one overlap suppression, in fractions of the picture: 1280 is what finds a group
photo's small faces, and it does not find a face that fills the frame (measured: one taller
than about two thirds of the preview's long side, a head shot or a selfie), which 320 does. A face both runs find comes out once, as the stronger of the two boxes. A face with
a number that is not finite is dropped before the suppression (`Raw::is_finite`): SQLite binds
NaN as NULL, the table refuses it, and the refused batch would be listed first by every pass
after. `items.face_version` records which `DETECTOR_VERSION` looked,
faces found or not; bump the constant when the model file, either input size, or the threshold
or overlap limit in `decode.rs` changes, and every photo is detected again. The pass pages by id: a photo
whose preview cannot be read, or whose decode panics, is skipped *unwritten*, so asked for from the start it would be
handed back for ever. Such a photo stays a candidate - right for a removed cache file, which
comes back, but a file libwebp refuses is never re-rendered, so it costs one failed read per
pass and the progress count stops short of the total. A detection that errors or panics
(`tract` unwinds, unlike rav1d) is written as looked-at with no faces instead, unless every
photo detected in its batch failed and there were at least `BREAKER_FLOOR` (8) of them: that is
taken to be the detector failing, not the photos (the rule assumes it; a count cannot tell), so the
batch is not written and `pass::run` errs (fewer than 8 failing photos are still marked, so a lone
bad photo is not retried on every pass; each later trigger fails one batch again, by design; and
the reverse cost: 8 or more genuinely bad photos with no success in one batch trip it on every
pass, and no photo with a higher id is ever detected - improbable, but it follows from the rule). `write_face_batch`
has two guards, each with a test that fails without it: a row whose size, mtime or edit moved
since it was listed is skipped, and nothing is written with the setting off, read inside the
batch's own transaction so the switch's delete and a batch in flight cannot interleave.
`update_items` and `set_item_edit` clear a photo's detections and its version, beside the hashes
they already clear. The candidate list and `face_progress` each read every live photo, so each
writes `+missing_since IS NULL` and has its own plan test. A pass that only detected rebuilds
through `refresh_grid_derived`: a detection is read by a face search's grid and by the viewer,
never by a collection count. Once at its end if it wrote anything, and during detection at most
every 30 seconds and only while the view is a search whose query reads faces
(`view_reads_faces`) - a branch no test reaches, its only trigger being that wait. A pass that
grouped a face rebuilds through `refresh_grid` instead (below).

**The switch is two values and one lock.** The stored `face_detection` setting is the authority;
`Engine.face_enabled` mirrors it so the pass's cancel check is not a database read per photo.
`set_face_detection` moves the mirror first (a pass in flight stops at its next photo), then
the stored value, and off deletes every detection, every person with their contact links and
rejections (`people`, `person_contacts`, `face_rejections`), and nulls every `face_version`, in
that same transaction. All of it, the mirror's restore on a failed write included, is under
`Engine.face_write`, as an edit is under `edit_write`: two toggles close together otherwise
reached the library's writer in either order and left the two disagreeing - mirror on and
stored off runs the detector over the whole library on every scan and drain while every batch
is refused. The lock is released before the pass request, the rebuild and the event. A pass
that ends with the switch off sends the cleared progress event itself, although the command
has sent one: the batch the switch interrupted can still report "running" after it, and the UI
would be left with a progress line for a pass that is gone.

**Recognising people is the face pass's second and third steps** (spec
`2026-10-03-photon-people-design.md`). After detection, `embed_step` runs
`photon_core::face_embed::pass::run`: a photo with a face whose `embedding_version` is not
`EMBEDDER_VERSION` has its cached preview decoded once, and each such face is aligned by its
five landmarks and turned into 128 numbers by SFace, through `tract` like the detector. A step
of its own rather than part of detection, so a library detected by 0.47.0 is recognised without
being detected again, at the cost of a second decode of a photo with faces. Its candidate query
(`EMBED_CANDIDATES_SQL`) applies the photo's conditions (live, preview ready) *inside* the
page's `LIMIT`, faces driving through a `CROSS JOIN`: outside it, a page made of missing photos
came back empty, and the pass, which reads an empty page as no work, stopped there. A face
narrower than `MIN_FACE_PX` (35) in the decoded preview is marked looked-at with no vector and
is never grouped (measured on LFW faces shrunk into a 1600 px picture: the same person still
matched 91% of the time at 35 px and 58% at 18 px, at a similarity of 0.45). `write_embeddings`
has `write_face_batch`'s two guards. The embedding breaker has detection's floor but counts
*faces*, not photos, and that has a cost: one group photo with 8 or more faces that all fail
(landmarks the aligner refuses), in a batch where no other face is asked about, trips it alone,
on every pass, and no photo with a higher id is embedded.

The **grouping step** (`group_ungrouped_faces`) places every face with a vector, no group and
not ignored (`UNGROUPED`, one string for the step and for `has_ungrouped_faces`; the vector one
the embedder made for that face, `embedding_version` set, of the right length - a malformed
blob matched there kept the probe true for ever), in face order on one thread, by
`people::choose`: the most alike group whose average is at least
`GROUP_SIMILARITY` (0.50), else a new unnamed group (measured on 2,674 LFW faces: 27-40
misplaced at 0.45, 6-11 at 0.50, 2 at 0.55 but with about 35 more of 500 people split - and a
split is one merge to fix, a mixed group a face at a time). **The averages are computed from the
faces at the start of each run, never stored**: a stored one has to be kept right by every
writer that moves a face - each operation, the edit and file-change clearing in `items.rs`, the
switch, the carry-over below - and one forgotten writer leaves it silently wrong. A named
person's average is their confirmed faces alone (`counts_toward_centroid`), so suggestions
cannot pull them towards themselves. Groups left with no face are deleted at the start of a run,
unless named or linked to a contact. Grouping runs during the embed step, and at the pass's end
while any face is still ungrouped (what an operation leaves). During the step it is paced by
`GroupPacer`: the first batch that writes is grouped at once, and each later run waits
`GROUP_COST_FACTOR` (9) times the last run's own duration, so grouping is at most about a tenth
of the step - every run reads every grouped face's vector, and run after every batch a first
recognition would read about 40 GB at 100k photos. Only the timing is kept between runs, never
the groups: an edit or a rewritten file deletes detections without `people_write`, so groups
held over could name deleted faces. The choosing runs inside the writer's transaction, so
it is kept cheap: `people::Group` keeps its sum's length beside the sum (private fields, so
nothing changes one without the other) and `face_embed::dot` sums in eight lanes - about 9 ns
a comparison against 95 ns recomputing both lengths. **Grouping is announced as a data change;
detection is not.** Not because the sidebar's People list, the Person view, `person:` or the
viewer move: they read confirmed faces only, and grouping writes none - only suggestions, new
unnamed groups and the removal of emptied ones. What reads those is `people_page`, which the
People page and its sidebar count of groups to name refetch on `data_changed`, so a pass that
grouped a face rebuilds through `refresh_grid` (`data_dirty`, `counts_epoch`) - at its end,
and during the embed step at most every 30 seconds whatever the view.

**The People page is `mainPage`, not a `GridView`** (`ui/src/lib/main-page.svelte.ts`): it has no
rows, so it is not a view of the grid index and does not travel the refresh chain as one. It is
drawn *over* the grid, which stays mounted beneath it (`visibility: hidden` and `inert`, in
`App.svelte`'s `.grid-layer`): unmounted, a returning grid starts at the top after the launch
restore is long done, and its "remember the folder at the top" effect overwrote the user's
place with the first folder on every return; `display: none` is no better, since it resets
`scrollTop` and shows the ResizeObserver a zero width. What leaves the page: every sidebar view,
a folder jump, typing in the search box, the viewer's search links, Locate, Show duplicates and
Statistics' links. Opening a face's photo does not: the viewer opens over the page, and the grid
is switched to All photos only when its view lacks the photo. `createPeoplePage`
(`people-page.svelte.ts`) holds the behaviour and is tested (server runtime: it has no effect or
derived value): a hide is optimistic and settled by a reload that *started* after the write (one
already in flight may have read the old rows), and a case-only rename of a person's own name is
a rename, not a merge. "Show more" survives a reload, re-fetched a page of `MORE` (200, the
backend's `MAX_FACE_PAGE`) at a time - one call is clamped to 200 and folded a longer strip back
- and a "Show more" or "Show fewer" made while a reload is in flight wins over it (each strip's
generation). **A reload keeps the order of the groups on screen**: the backend sorts Unnamed by
size, and re-sorted rows are keyed, so they moved under the user - the name box being typed in
lost focus and a click could land on another group; only the page's first answer is taken as
sorted, and new groups go at the end (a group the 200-group cap below now leaves out simply
goes; one it now lets in arrives at the end). Library changes reload it through `changed()`, at once
and then at most once a second (`CHANGE_GAP_MS`, trailing): each reload reads every visible
face, 148 ms at 150,000 (`people_150k_faces` in the grid bench), and a scan announces a change
per rebuild; the page's own actions call `load` and are never held back. **Unnamed lists the 200
largest groups** (`LISTED_GROUPS`) with `unnamed_count` counting all of them; the sidebar's "N
to name" is `people_to_name`, equal to `unnamed_count`, and is fetched with the collections on
`data_changed`. A write refused with `notAPerson` (its group is gone) is reported as "That group
changed". "Confirm all" confirms the faces on screen, not the person's whole suggestion list.
Confirmations (switching detection off with named people, Delete, Merge) are the native `ask`,
as everywhere else. The page's crops are `/face/<face id>/<thumb key>` in `protocol.rs`, cut
only from the cached preview: 404 for a gone face, a stale key or no cached preview (never a
render, so a page of crops cannot start a burst of renders), 500 for an undecodable one, and
`immutable` because a face id is never reused and never names another picture. It is the one
`photon://` route that decodes per request outside the other bounds (an edited `/image` render
takes `RENDERING`, a thumbnail the pool), so it has its own: a crop waits on `FACE_CROPS`, a
semaphore of one permit a core, acquired *before* `off_thread` so a waiting request holds no
blocking thread, and `ThumbCache` keeps the last eight decoded previews by key
(`decoded_preview`), so a group photo's faces decode it once - a key names one picture, so a
kept decode is never stale.

**`people_write` serialises the corrections with grouping.** Every operation on the People data
(`name_group`, `rename_person`, `confirm_faces`, `reject_faces`, `merge_people`,
`set_person_ignored`, `set_faces_ignored`, `delete_person`, and the grid's and the viewer's
`name_faces`, `name_items` and `remove_from_person`) runs through `Engine::write_people`,
which holds the lock for the write, then rebuilds through `refresh_after_write` and requests a
face pass; each grouping run takes the same lock. An operation never places a face itself: a
rejected face, or one no longer ignored, is left ungrouped, and the pass's grouping step places
it, passing over every group it was rejected from (`face_rejections`). A merge (`merge_into`,
behind `merge_people` and a name another person has) leaves out, ungrouped, every face of the
merged group that was rejected from the person it merges into: grouping puts a face taken out
of Anna in another group, and naming that group Anna would otherwise make it a confirmed Anna.

**The grid and the viewer name and take off faces by id** (spec
`2026-10-03-photon-people-from-grid-and-viewer-design.md`). `name_faces` names faces as a person
resolved by the one rule (`clean`, `same_name`, contacts linked by name): confirmed, no longer
ignored, any rejection from that person forgotten; it makes a person only when one of the faces
still exists, so a stale request leaves no empty named person behind, and it names a face too
small for a vector, which puts the photo in the Person view without ever shaping suggestions.
`name_items` is the grid's, by photo, and **never confirms a face the user did not choose**: a
photo's candidates are its detections not confirmed as a named person, not ignored, not in an
ignored group and not under a face Picasa names (linked or not, through the photo's edit:
`merge::shown`, then `merge::same_face` - that face is someone already, drawn as Picasa's
plate), and the photo is named only with exactly one. It is skipped as *already* when the person
has a confirmed face on it, or Picasa names on it a contact linked to them or an unlinked contact
of that name - the one this very write would link (`link_contacts_by_name`), so it counts whether
or not the person exists yet - checked first, because the photo's one *other* face is then a
stranger the user did not mean; as *rejected* when a candidate was rejected from the person (a
guess per photo never overrules "Not Anna"; `name_faces`, where the user picks the face, does
clear the rejection); as *several* with more than one candidate (listed by file name, so the user
opens it and picks the face in the viewer; "the largest face" was asked about and declined); and
as *none* with no candidate ("no unnamed face": its faces may all be someone's or ignored). An
ignored face is no candidate, so a background stranger the user put away does not make a photo
ambiguous. `on_person_view`, the "already" test, keeps hidden photos on purpose: in the Hidden
view "Add to Anna" on a hidden photo that shows Anna must not name its other face.
`remove_from_person` is "Not this person" for every face of the person on each photo, confirmed
or suggested, through `reject_in`; a photo the person is on through a linked Picasa face stays
in their view, and `kept_by_picasa` counts it so the toast can say why. The person dialog lists
`named_people` (every named person, read when it opens), not the sidebar's `people_with_counts`,
which leaves out a person with no visible photo whom a typed name still joins. **None of the three
changes Picasa's faces**: photon never writes a name to an INI. **Which detection a viewer face
is** is `ItemFace.face_id`/`UnnamedFace.face_id` (`viewer_item`): a detection's plate or outline
carries its own id; Picasa's plate of a *linked* person carries the detection beneath it
(`merge::same_face`) confirmed as that same person; an unnamed Picasa outline the detection
beneath it that no named person is confirmed on (it is drawn in that detection's place); and a
plate of an unlinked contact (`c:`) carries none, since the face beneath may be someone else's.
A face with no id is Picasa's alone, and the viewer offers nothing on it: its context menu
hit-tests every face as drawn (`toLayer` through the face layer's own bounding rectangle, which
already holds the zoom and pan, then `faceAt`) whether or not the info panel shows the outlines,
and `faceActionsAt` (`lib/faces.ts`) offers "Name this face…" on an unnamed face, "Not Anna" on
her plate, and elsewhere "Not …" once for each person photon has a face of on the photo. **The
viewer's "Not Anna" is photo-level**, the grid's Remove for one photo (`remove_from_person`),
so it is offered only where photon holds a face of hers, and its toast (`removedMessage`) says
when Picasa keeps her there. A rejection reloads nothing (`pictureChanged`'s `Pick` leaves the
faces out); in that person's view the photo leaves the grid and `orphaned` keeps it on screen,
as after Hide. The slideshow's countdown is held (`hold`) while the dialog is over the viewer.

**Only confirmed faces carry a name.** A face the rule puts with a named person is a suggestion
(`confirmed = 0`), listed among `people_page`'s suggestions and nowhere else: the Person view,
the People list's counts, `person:` and the viewer's name plates read confirmed faces only.
Naming an unnamed group confirms its faces, and so does merging one into a person; renaming a
named person does not confirm their suggestions, which the user has not looked at. A name
another person has, compared without case in Rust, merges into them. **A person's key** is
`p:<id>`, or `c:<hash>` for a Picasa contact no person is linked to: the Person view's argument,
`GridInfo.person`, `Person.key` and `ItemFace.key`, prefixed so nothing depends on what a hash
may contain; an argument with neither prefix gives an empty grid. A contact is linked to the
person whose name it carries (`link_contacts_by_name`) when `upsert_contacts` records it and
when a group is named or renamed - the user typing a name Picasa uses is saying it is the same
person - and a linked contact is that person everywhere: their key and name in the viewer, their
Person view, and `person:` finds the contact's Picasa-only photos by the person's name. The
viewer draws a face once: a confirmed detection over a named Picasa face, linked or not, leaves
the plate to Picasa's, and one over an unnamed Picasa face takes its outline's place - so a
detection confirmed as Ben over a Picasa face named for a contact Anna that no person is linked
to shows only Anna's plate in the viewer, while Ben's Person view lists the photo. The Picasa
name offered for an unnamed group counts only its faces that sit on a Picasa face under a named
contact (more than half of those must be one contact's): an unnamed Picasa face at the same
place neither votes nor hides the named one. `people.id` and `detected_faces.id` are
`AUTOINCREMENT` - migration 25 rebuilds `detected_faces`, released in 0.47.0, to get it -
because the UI holds those ids (`people_page`) and acts on them later: SQLite otherwise hands
the highest deleted id to the next insert, and a confirm or a rename would land on a face or
person the user never saw. Every named person is in `people_page`'s People section, with a
`face_count` of 0 when none of their confirmed faces is visible: off it they could not be
renamed, merged or deleted, yet `face_data_summary` would still count them.

**What clears what.** An edit or a rewritten file deletes the photo's detections, and with them
their group, confirmation and rejections: re-detected and grouped again, a confirmed face comes
back as a suggestion (carrying it across a turn or a crop would mean mapping rectangles through
the edit, for a case one click fixes). A detector re-run over an unchanged picture - a
`DETECTOR_VERSION` bump - must not do that to a whole library: `write_face_batch` hands each new
face the group, confirmation, ignored flag and rejections of an old face at the same place
(`merge::same_face`), each old face to one new face, best overlap first (`pairs_by_fit`: the
centre test alone let a small face inside a large one take the large face's name). The old
vector goes too, with no `embedding_version`: the face is embedded again, and meanwhile counts
towards its group's average but is not placed by it (`UNGROUPED` asks for the version) -
without it a re-run emptied every named person's average for the whole embedding step. A
renamed or moved file or folder keeps its row (`move_items`, not `update_items`: the content is
the same, so the detections, their groups, confirmations and rejections stay, and a face's
rectangle is in fractions of the picture as shown, which a move does not change), so it keeps
them as it keeps its albums; it loses them only where it is still a purge and a new row - out of
the watched folders and back, a copy whose original goes later, a move between volumes the
watcher scanned half-way (above). A named person whose confirmed faces are all gone
has no average, so draws no suggestions until a group is named for them again. **Hidden
photos' faces are grouped** like any other, so unhiding is instant, and appear in no People
reader: the page's two face queries, the People list, the Person view and the search grid each
filter `hidden = 0` and `missing_since IS NULL` (`hidden_photos_are_in_no_person_reader`,
`a_hidden_photo_is_in_no_section`). They still count towards their group's average.

**Two tables of faces, in two frames.** Picasa's `faces` rows are fractions of the *unedited*
picture and are mapped through the edit on read; `detected_faces` rows are fractions of the
picture *as shown*, because that is the preview the detector reads. `face_detect::merge` is the
one place they meet: `merge::shown` is the single mapping of a Picasa rectangle into the
picture as shown (the viewer's named faces go through it too), and its rule for "the faces on
this photo" is every Picasa face the edit still shows plus each detection that is none of them,
where either rectangle's centre inside the other is the same face - not an overlap ratio, since
Picasa draws a face loose and YuNet tight. `viewer_item` and `search_face_counts` both go
through it, so the viewer and `has:face`/`faces:N` cannot disagree, and a new reader of either
table goes through it too. A Picasa face no INI names counts as a face under that rule, with
detection on or off: left out, it would hide the detection lying over it.

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
outlives an App remount. The native title bar is themed twice, on purpose: by `setup` in
`app.rs`, from the stored choice, as soon as the library has opened, and again by the UI on
every change. The mirror cannot reach the window, and the UI's own call only lands once the
webview has loaded and asked for the setting, so without the first a pinned theme opened under
the desktop's title bar. `System` is `None` in both, never the scheme resolved in code: `None`
is what lets the title bar keep following the desktop while photon runs.

**A dialog belongs in `App.svelte`, not in the component that opens it.** `covered` makes the
topbar, sidebar, splitter and `<main>` `inert` while an overlay is up, and the grid is inside
`<main>`: a dialog mounted there is made inert *by its own opening* - Tab walks out of an
`aria-modal` dialog into the tiles and the gear, and Settings can then be opened on top of it.
A new overlay renders beside `Settings`, counts towards `covered`, and hands focus back when it
closes - after `await tick()`, because `<main>` is inert until the DOM catches up and focusing
an inert element silently does nothing. The grid's keys live on its viewport, so a dialog that
closes onto `<body>` leaves the arrow keys, Enter and Escape dead until the user clicks.
A dialog opened **over the viewer** (the person dialog, naming a face) has one more thing to
stop: the viewer's keys live on `<svelte:window>`, not on anything `inert` can reach, so typing
"h" in the name field would hide the photo behind it and the arrows would step past it. The
dialog stops the propagation of every `keydown` inside it, and the viewer ignores every key
while `paused` (App passes `personPicker.visible`) for the key that arrives with focus on
`<body>`; the viewer itself sits in a `display: contents` wrapper made `inert` while the dialog
is up, so Tab cannot walk out of the dialog into its controls.

**The keys are listed in one place, `lib/shortcuts.ts`,** which the `?` sheet
(`ShortcutSheet.svelte`) and Settings → Shortcuts both draw. `shortcuts.test.ts` reads the
key handlers' source and fails on a key a handler answers that the list does not name, and on
a listed key no handler answers - but only in the files its `ANSWERED_IN` names, so a handler
in a new file is added there with its group, and it cannot check the wording: a key whose
meaning changes needs its row changed. The sheet is the one overlay that opens over the
viewer *and* over compare, so both sit in an `inert` `display: contents` wrapper while it is
up and the viewer is `paused`, as under the person dialog; it does not open over another
dialog, or while the grid holds a rubber band (`Grid.dragging`, the "fourth ending" below).

**A context menu is placed by `use:fitMenu`** (`lib/menu-place.ts`), never by binding
`left`/`top` to the pointer: it opens down-right and flips on an axis where that would leave
the window, re-placing itself when it grows (the tile menu's copies line arrives after it
opens) or the window resizes. A menu bound straight to the pointer is clipped when opened near
the bottom or right edge; every menu was, until the folder menu grew to five items.

**A pointer gesture has three endings, not two.** `pointerup` finishes it and Escape abandons
it, but a browser that claims the gesture for itself - a touchscreen pan, which Windows
laptops have - sends **`pointercancel`** and nothing else. Every drag needs one teardown that
all three reach: `App.svelte`'s splitter wires `onpointercancel={endResize}`, and the grid's
rubber band reaches `abandonBand` the same way. **`inert` does not stop a
`requestAnimationFrame`**, so an overlay opening over a drag is a fourth ending - the grid's
key handler honours nothing but Escape while a band is live, for exactly that reason.

The reason this is here rather than in the band's own spec: the missed teardown was a *stale
rectangle* until autoscroll gave it a loop to drive, and then it scrolled to the end of the
library rewriting the selection, with a pointer id it never cleared blocking every later drag.
A new loop or timer is worth asking what already-known-broken path now drives it. Anything
per-frame is worth the same question about time: multiply by the frame's own duration, or it
runs twice as fast on a 120Hz screen as on a 60Hz one.

**A shrinking scroll container clamps `scrollTop` for you, synchronously.** When a scroll
container's content gets shorter, the browser clamps `scrollTop` to the new maximum during the
very layout that reading `scrollTop` forces - before any script gets to react. A guard that
asks "has the viewport moved since I pinned it?" by comparing a saved `scrollTop` to the
current one is therefore always false on a size decrease: the browser already moved it, on the
guard's own read, so the guard sees no movement to restore. That shipped to a whole-branch
review before being caught, in the code that restores scroll position after the tile size
changes. Anything that restores a scroll position after changing content height needs to ask
what the *content* did, not what `scrollTop` reads now.

The focus-ring suppression is `.focus-container[tabindex='-1']:focus-visible { outline: none }`,
not the bare `[tabindex='-1']` it once was: a roving-tabindex widget's items carry
`tabindex="-1"` too, and an unscoped rule strips their ring so the keyboard user cannot see
where they are. A container focused from script - the viewer, a dialog, an open menu - therefore
has to be given `.focus-container` itself, or it draws a ring around the whole window;
`tokens.test.ts` is the tripwire, failing both on the unscoped form coming back and on the
scoped one going away. A tile and the timeline are deliberately *not* classed: they are never
script-focused, so they rely on that rather than on a ring hidden after the fact - which is why
a tile's selection ring comes from `.selected` rather than from focus, as `Tile.svelte` says
where it draws it. It is drawn inside the tile, not as an outline around it, because the grid
scrolls a row flush to the top of its container (ArrowUp, Home, Recent's first row), which clips
anything sitting outside the tile's own box.

The look cannot be tested here, but it can be seen without launching the app: `cargo run -p xtask --
screenshots` builds the UI, serves `ui/dist` itself with `mock.js` (in
`crates/xtask/screenshots/`) standing in for Tauri's IPC, and writes thirty-five PNGs, in both themes,
to `target/screenshots/` with headless Chromium. It claims a Windows user agent and maps
`photon.localhost` to its own port, because `mediaUrl` uses `http://photon.localhost` there
and no plain browser can load `photon://`. It is Chromium's rendering, not WebKitGTK's or
WKWebView's, so it replaces no item of the smoke checklist, and it is not run in CI. A new
IPC command must be given an answer in `mock.js` (`canned`, or `SILENT` when nothing draws its
result): a test in `screenshots.rs` reads `api.ts` and fails otherwise, because an unanswered
command resolves to null and the screenshot of what the UI makes of that reads as a styling
bug. A new surface worth seeing is a new entry in `SHOTS` and, if it needs a click, a new
action in `mock.js`.

## Conventions

- **photon never writes to, moves or deletes photo files.** The one file it writes inside a
  watched folder is Picasa's own `.picasa.ini` (or `Picasa.ini`), through `picasa::set_star`
  only, to set or clear a single `star=` line; every other byte of that file is preserved.
  An INI that is a symlink or not a regular file is refused, by the reader too: the writer
  keeps every byte it reads and renames over the link, so following a planted
  `.picasa.ini -> ~/.ssh/id_ed25519` in a shared folder copied the key into it on one star.
  Export (`photon_core::export`, spec `2026-09-20-photon-export-copies-design.md`, 2026-09-20)
  writes photo files, but only *new* ones, only where the user pointed a folder picker, and
  never inside a watched root - `Engine::export_items` refuses that destination, because the
  scanner would index the copies as new photos. No watched photo is ever opened for writing,
  and every file it does write is created with `create_new` (`O_CREAT|O_EXCL`):
  `exists()`-then-write is a race, and `exists()` follows a symlink, so a dangling link in the
  destination landed a write inside a watched folder.
  This narrowed the older "never writes inside watched folders" promise on 2026-09-16 (spec
  `2026-09-16-photon-set-star-design.md`); any further write is a spec-level decision, not a
  code change. That is why a star does not survive the renaming or moving of a single file,
  though the scanner follows the photo (`photon_core::moved`, above): the star is the INI's
  line under the file's name, a star set in photon included, and carrying it would be the
  scanner writing `star=yes` for the new name with no gesture from the user. It follows a
  renamed or moved *folder*, whose INI goes with it. The writer and the reader in `picasa.rs` share one line classifier on purpose:
  a writer with its own header/key logic drifts from the reader. Faces, contacts, albums and `hidden=` flags are read
  from the same INI and never written; keywords are read from the photo and never written (the user's renames and
  removals are `tag_rules` rows applied on read, `library/tags.rs`);
  photon's own albums and edits (turns and crops) live only in `library.db` (both are by item
  id, and a renamed or moved file keeps them because the scanner re-points its row
  (`photon_core::moved`, above); a photo moved out of the library and back, or copied and the
  original deleted later, is still a purge and a new row, which loses them - a recorded
  limitation, not a bug), and so do the faces photon detects itself (`detected_faces`,
  deleted when the switch goes off) and the people the user names among them (`people`, deleted
  with them; a name is never written to an INI or a photo); Picasa's albums are read from its
  INI, as noted above, and live nowhere else. An edit never touches the photo: it is rendered on the way to the
  screen.
- **No system library dependencies beyond the web view.** photon's C is vendored and compiled in with `cc`:
  SQLite, libwebp and libjpeg-turbo (the mozjpeg crate, for the scaled decode of a JPEG's
  preview and the encode of an edited photo's full-size render, `photon_core::turbo`). Nothing
  wrapping a C/C++ SDK: that bar is what made packaging tractable on three platforms, and it is
  why XMP and INI parsing are hand-rolled or pure-Rust. nasm is a build tool on the CI and
  release runners only, for libjpeg-turbo's x86 SIMD code and rav1d's x86-64 assembly; a build
  without it still works, as plain C and plain Rust, because rav1d's assembly is the opt-in
  `avif-asm` feature (it fails to build without nasm rather than falling back). Release turns
  it on for every x86-64 installer and never for arm64, where rav1d 1.1.0's published assembly
  does not build; CI tests photon-core through it. A feature cannot be limited to a target, so
  that is the workflows' job, not Cargo's. A new C dependency is a spec-level decision
  (`2026-09-29-photon-turbo-jpeg-thumbnails-design.md` is the worked example). Face detection
  kept to the same bar, which rules out ONNX Runtime and OpenCV: the network runs in `tract`,
  pure Rust but for `tract-linalg`'s assembly kernels, which it compiles in with `cc` and no
  nasm, and nothing outside `face_detect` and `face_embed` names it. Both models are files in
  `crates/photon-core/models/`, embedded with `include_bytes!` - the YuNet detector and the
  SFace recogniser (38.7 MB, Apache 2.0, in `THIRD-PARTY-NOTICES.md`) - and nothing is
  downloaded. Both run on the CPU: GPU inference has no one route across three platforms, cannot
  be tested in CI, and would speed up only the first run. The workspace `Cargo.toml` builds
  `tract-linalg`, `tract-core` and `tract-data` optimised in the dev profile: unoptimised, one
  detection takes about five seconds, and the tests run the real models.
- **AVIF is decoded in `photon_core::avif`, not by `image`**, whose AVIF decoder is dav1d (C).
  `zenavif-parse` reads the container, `rav1d` (its assembly only under `avif-asm`, above)
  decodes the AV1, and `avif/av1.rs` and `turbo.rs` (libjpeg's error manager) hold the only
  `unsafe` code in photon-core. Every full decode goes through `decode::decode_image`, which sniffs the `ftyp`
  box; the uncropped thumbnail's preview goes through `decode::preview_decode`, which tries
  libjpeg-turbo's scaled decode on a JPEG first and hands everything else to
  `decode_image`'s own path. Calling `ImageReader` directly skips
  AVIF. The container's `irot`/`imir` are applied in the decoder, so an AVIF's stored
  orientation is always 1 and its EXIF orientation is ignored. `imir` axis 1 is left-to-right,
  as libavif reads it, whatever `zenavif-parse`'s doc comment says. A panic inside rav1d cannot
  unwind out of its `extern "C"` entry points, so it aborts photon rather than failing one
  thumbnail the way the thumbnail service's `catch_unwind` handles other formats (see
  `crates/photon-core/src/avif/av1.rs`'s module doc and the spec's "Limits" section);
  `thumbs/inflight.rs` keeps that from repeating on every launch.
- **Never launch the GUI to verify a change.** Verification is the test suites plus
  `svelte-check`; anything needing eyes goes on `docs/smoke-checklist.md`.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not
  proof — it shows a symbol was missing, not that an assertion discriminates behaviour. Tests
  that pass with and without the change have shipped here more than once. When a change
  genuinely cannot have one — a race with no seam, a Svelte effect — say so in the commit
  message and why, rather than adding a test that passes either way. **A probe that passes is
  a finding, not a formality:** it has exposed a missing test (the slideshow's still-loading
  case) and a line whose comment called it load-bearing when it did nothing (an `IS NOT NULL`
  "planner hint"), and an export collision ledger that changed no outcome. Revert with an
  exact replacement; a loose `sed` that also hits a neighbouring writer fails ten tests and
  proves nothing.
  **A probe proves the rule only for the inputs the tests actually use.** Six green probes
  on the rubber band left four of its rules undefended, because each probe only exercised
  cases the existing tests happened to cover: the range-merge rule was one of them, and
  without it a narrow band selects seven photos where three were drawn. The same blind spot
  had left `publish_if_current`'s epoch guard pinned by nothing at all — the whole app suite
  passed with it removed, including the test named after it. When a probe passes, ask what
  input would make the reverted code *differ*, and write that case.
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
