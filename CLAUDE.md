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
cargo nextest run --workspace
cargo bench -p photon-core --bench grid --no-run
```

The tests run through **cargo-nextest** (`cargo install --locked cargo-nextest`; CI takes it
from `taiki-e/install-action`), which runs every binary's tests in one pool where `cargo
test` runs the binaries one after another; `cargo test --workspace` still runs the same
tests, more slowly. nextest runs no doctests, and the workspace has none: a doc example is
written as a test. Its settings are `.config/nextest.toml`. **photon-ui's tests of the whole
application are one binary**, `all` (`tests/main.rs`, `autotests = false`), each file in
`tests/` a module of it: twenty-five binaries each linked egui, wgpu and the engine. A new
file there is a `mod` line in `tests/main.rs`, or it is never built
(`every_file_in_tests_is_a_module_of_the_binary` says so).

**The UI gate**, from the repo root:

```bash
npm run check   # svelte-check --fail-on-warnings: 0 errors AND 0 warnings
npm test        # vitest
```

**Single test:**

```bash
cargo test -p photon-core the_test_name          # by name substring
cargo test -p photon-core --lib scanner          # one module
cargo nextest run -p photon-ui --test all dialogs::   # one file of the whole-app tests
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

**CI's caches are saved from `main` and `native-ui` only**, and its builds carry no debug
info (`ci.yml` says why: the 10 GB cap, and Windows' linker). So a pull request that changes
`Cargo.lock` compiles the difference on every run, a backtrace in CI has no line numbers,
and the benchmark is built on Linux alone. A job that takes twenty minutes where it took
five has lost its cache: `gh cache list` shows what is there.

**Small files are what Windows is slow at, and its runner's C: drive slowest.** NTFS makes
and removes files several times slower than Linux does, and the runner's TEMP, on C:, several
times slower again than D:, where the checkout is - so `ci.yml` moves TEMP to D:. The native
window's tests build libraries of thousands of cached thumbnails in temporary directories,
and that alone was twenty of the Windows job's thirty minutes (2026-10-14: one test of
18,000 photos, 155 s on C:, 23 s on D:, under 3 s on Linux). A fixture writes its rows in one
transaction (`Library::set_thumb_states`), not a commit a row, and makes no more files than
its test needs; a Windows machine with an antivirus scanning TEMP pays more still.

**Repository chores** (also run in CI, so run them before tagging):

```bash
cargo run -p xtask -- versions            # Cargo.toml, tauri.conf.json, ui/package.json agree
cargo run -p xtask -- versions --tag v0.5.0   # ...and match the tag
cargo run -p xtask -- metadata            # licence and installer metadata are complete
```

**Seeing the UI without launching it** (not in CI; needs Chromium on `PATH` or in `CHROMIUM`):

```bash
cargo run -p xtask -- screenshots                     # forty-four PNGs into target/screenshots/
cargo run -p xtask -- screenshots --only viewer-info-light --no-build
cargo run -p xtask -- scroll-probe   # the end of a 300k-photo library is reachable
```

The project website (`site/`, deployed to photon.webcodr.io by `pages.yml`) uses these
screenshots with real photos: `--photos crates/xtask/screenshots/photos` serves the CC0 photos
there (credited in their `CREDITS.md`) in place of the gradients. Regenerate `main-light`,
`main-dark` and `viewer-info-light` with it and convert them to `site/img/*.webp`
(`magick x.png -quality 85 x.webp`), and `main-light` to `site/img/og.jpg` at quality 88, the
link preview. With `--photos` the server puts a flag before `mock.js`, and the mock then leaves
out the faces it says nobody has named: they are rectangles placed for a gradient, and on a
real photo they outlined skyline. A photo kept large for the viewer shots has a small copy in
`photos/thumbs/`, which the server gives to a grid tile and a face crop: a tile's `<img>`
decodes off the main thread and the virtual-time budget does not wait for it, so the 1600px
photo was blank in a few `main-dark` shots in a hundred (5 of 70, then 6 of 350; none of 250
with the `<img>` decoded synchronously), the website's own among them. A loop that checks for
it needs hundreds of shots: at that rate forty come out clean half the time with nothing fixed.

`npm run dev` runs the app with hot reload. **Do not run it to verify a change** — see
Conventions.

## Architecture

Five crates plus the UI:

- **`photon-core`** — headless. SQLite library, scanning, thumbnails, metadata. Knows nothing
  about Tauri.
- **`photon-engine`** — `Engine`, the commands over it (`commands.rs`) and the folder
  watcher. No UI runtime is a dependency, and `the_engine_depends_on_no_ui_runtime` reads
  its manifest to keep it so: it is what a second shell stands on.
- **`photon-app`** — the Tauri shell. Owns the IPC surface and the custom protocol that
  serves thumbnails, and re-exports the engine's modules under the names they had there.
- **`photon-ui`** — the native interface, on egui and wgpu, that is to replace `photon-app`
  and `ui/` (below, "The native UI"). Unreleased; its binary is `photon-native`.
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

**An empty library says why, and only what the UI knows** (`emptyLibrary` and `noPhotosLine`
in `grid-state.ts`, drawn by `Grid.svelte` beside its viewport, in All photos and Recent
only). The states are not the ones they look like. "No folder is watched" is the rare one:
`startup` watches the Pictures folder by itself when nothing is watched, so most first runs
are a watched folder with a scan in it, or with nothing in it - which therefore offers Add
folder… as well. **The UI knows a scan is running only because it is told, in two ways.**
A full scan reports once before it walks, with nothing seen, as soon as it has found its
root (`scan_watched`): the walk's own first report is 64 photos in, a flushed batch, or its
end, and an empty library said photon had found no photos in a folder it was a minute into
reading. Not a root that is not there (an unplugged drive is polled twice a minute), not one
last found offline (`watched.online`: an unmounted volume leaves an empty mount point, polled
the same way, and telling it from a live folder reads every known row first - the scan's
line came and went with each poll; the price is that the first scan after the drive is back
goes without the report), and not a subtree scan (the watcher runs one for every directory a
file changed in, each over in milliseconds: announced, each would flash the scan's line). And because the
startup scans begin before any webview exists and an event sent to no listener is lost, the
UI asks once, after it has subscribed, which scans are running: `scanning_folders`, which
answers from `Engine::reporting` - not from the scan slots, which hold INI passes that
report nothing. `reporting` is kept by the code that sends the events, on the same side of
each: in before a scan's first not-done event, out *before* its done. That order is the
whole point - a listener told "scanning" is always still owed the done that ends it - and
`scanning_folders_names_a_scan_from_its_first_report_until_it_is_done` reads the list from
inside the event sink to hold it. The UI leaves alone a scan that has reported since it
asked (its done can overtake the answer). What the UI writes into `scans` for a scan it was
told of is a note, not an event (`noteRunning`, `unheard`): the scan's first real event must
still count as its first, or the folder's size is never snapshotted and the status bar
counts files with nothing to measure them against. The slot's release takes a panicked scan
out of `reporting`, since it will never report done. `'no-photos'` is still never worded as a finished
search ("has found no photos", not "looked and found none"): a folder photon may not read is
scanned and reported like an empty one. Three smaller things beside it: a folder added
through `library.addFolder` has its scan counted as running at once (the command's answer
and the scan's opening event arrive in either order), unless a scan there has already
reported; a scan's first event in a folder the list does not hold reads the list again (the
list read at launch may predate the automatic Pictures); and a scan that reports done on an
empty grid counts as running until the grid has been read again (`settling`: the rebuild's
event and the scan's end arrive a fraction of a millisecond apart, a round trip before the
photos). Hidden photos
come before all of it: with any, the library is not empty. Nothing is said before the folder
list has been read once (`foldersKnown`). The scanning and no-photos states are one block
with one sentence changing, because a watched folder is rescanned at any time and buttons
replaced on each flip lost the focus and a click on its way; and the focus is read before
the panel's DOM changes (`$effect.pre`) and handed to the grid if its button is gone after.
The panel is outside the viewport because the viewport is the rubber band's surface.

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
folder's photos oldest to newest. While the grid is grouped by folder the sidebar groups by the same value, so the list is an
index of the grid (under a date grouping it is not: see the grouping, below). Changing one without the other splits them onto different axes. `GRID_ORDER` reads
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

**The grouping** (`sort::Grouping`, the `grid_group` setting) is a field of that `Sort`, not a
value beside it: by date it decides the order as well as the headers. `Folder` is the view's own
order and layout; `Day`, `Month` and `Year` are one timeline across folders, newest first
(`arrange`: a stable sort by `taken_at` over the view's rows), laid out as `Layout::Periods`,
in every view, Recent included; `None` is that timeline flat. Under any other key it is kept and
ignored, so the sort control changes a `Sort` by spreading it - built from its two fields, it
dropped the grouping. A `Section` carries its `period` as numbers (`grid::Period`, read with
`civil_from_unix` like search's `2024-06`), and the UI formats those (`periodLabel`), never
`takenAtMin`, which it would read again in the viewer's zone. A section has a header when it
has a folder or a period (`hasHeader`); code that means "flat" asks that, not
`folderId === null`, which a period section has too - `selectAll` did, and under a month header
selected a folder's scattered photos. The place photon remembers in the library is a folder, so
its restore and `returnToAll` ask `laidOutByFolder`, not the sort key. Under a date grouping or
none a folder has no header, so a sidebar click lands on the first of its photos the grid
reaches (`offset_of_folder`, the flat answer) and the sidebar is no longer an index of the
grid. The control's dimmed `Select` is `createSelect`'s `disabled`, tested there.

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
prefixed term. **The grammar is listed for the user in `lib/search-help.ts`**, which the `?` in
the search box draws (`SearchHelp.svelte`): `search-help.test.ts` reads `Query::terms` out of
`search.rs` and fails on a prefix, an `is:` or a `has:` the list lacks, and on one it offers
that the parser would drop. It finds them by shape - `prefixed("x:")`, and the `"word" =>` arms
between `prefixed("is:")` and the next prefix - so a parser rearranged away from that shape
changes the test with it; the wording and the examples nothing checks. The query string is the whole interface, so UI links (the info panel's camera
and lens) go through `searchBox.search()`, which cancels a pending debounce first.

**The grid's keys are answered on its viewport, and the focus is kept there.** A click
focuses the tile itself where a button takes the focus from the mouse (WebView2; WebKit hands
it to the viewport), and the keys then bubble - until that tile scrolls out of the rendered
rows and is removed, which drops the focus to `<body>` and leaves every key dead: two Page
Downs after a click. The viewport's `onfocusin` takes the focus any tile is given. A
navigation key moves on from `library.keyboardLead`, not from `selected`: Shift with an arrow,
Home, End or a page key is `extendSelection`, which moves the lead only when the range's ids
arrive, and a held key repeats faster than that. Page Up and Page Down are `pageMove`
(`layout.ts`); with nothing selected they are left to the viewport's own scrolling, because a
move would start from the first photo. The keys reach what a click cannot - a page the grid
has not loaded - and three rules in the store exist for that. A range takes its lead's photo
from the ids it fetched, never from the loaded pages (`extendSelection`): Shift+End's far end
is not loaded, and a lead with no id is collapsed by the viewer closing on it and loses its
anchor at the next rebuild. A range does not land over anything selected while its ids were
on their way (`picks`): it is a round trip per thousand photos, and Shift+End then Escape put
the whole range back under the next `H`. And a lead the plain setter put on an unloaded page
(End, Home, a folder jump) is selected once the page arrives (`adoptLead`): before, its tile
lost the ring the moment its entry came, and `H`, the star and Ctrl+C had nothing to act on.

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

**A tile is drawn wider than its size says.** `TILE_WIDTH` is the size chosen; `tileFor`
widens it so the columns fill the row (whole pixels, at most `TILE_MAX`), and that drawn width -
`tile` in `Grid.svelte`, never `gridSize.width` - is what the rows, the tiles and the band's hit
test are measured by. So every row's height moves with every pixel of a window or sidebar
resize, and a narrower grid is a *shorter* one. Six things follow, and the last three shipped
to a review before anyone saw them:

- The place is kept as a `Pin` (the row at the top, and the share of it scrolled past) and
  restored on any change of the tile or the column count, not only on a new size.
- The restore's own scroll event is not pinned again (`restoredTo`): re-read through the
  browser's rounding on every frame, a splitter drag walked the grid off its photo, 5px in a
  60px drag.
- Rows are keyed by their index in the layout, not by their `top`: keyed by `top`, every frame
  of a resize unmounted and remounted every tile on screen.
- Everything is drawn at `viewTop` (`placeIn`), not at `scrollTop`: on the render where the
  tiles change, `viewTop` is already the pin's place in the new rows, while the viewport is
  only moved by an effect after it. Drawn at `scrollTop`, that render showed the rows the old
  number names among the new ones - blank, deep in a library - for a frame on every frame,
  remounted every tile and named another folder at the top (24 `set_last_folder` writes in
  one drag). Anything new that asks "what is at the top" asks `viewTop`.
- The year strip is shown by `showsTimeline`, which asks whether the grid *as it would be
  beside the strip* has something to scroll (`.grid`'s own width, less the strip and the
  scrollbar's gutter). Asked of the grid as it stood, the strip narrowed the tiles, the grid
  then fitted, the strip went, the tiles widened, the grid overflowed: every frame.
- A scrollbar that takes room keeps it always (`scrollbar-gutter: stable`, `overflow-y:
  scroll` where that is not known): one that came with the overflow took the overflow away
  the same way, and Svelte's `bind:clientWidth` watches the border box, which a scrollbar
  does not change, so the tiles were sized for a row 15px wider than it was.

The effect wiring here has no test. What showed all of it was the built UI in headless
Chromium, driven by trusted mouse input on the sidebar's splitter, against a build with each
point taken out - **150,000px down a 5,000-photo library, not near its top**, where the stale
frame is inside the overscan and every probe passes; with scrollbars that take room, which the
screenshot harness hides; and in a view about one screen tall with two years in it.

### IPC is three files per command

Adding a command means touching all three, in this order:

1. `commands.rs` (in `photon-engine`) — a plain `pub fn` taking `&Engine`, returning
   `CmdResult<T>`. The logic.
2. `ipc.rs` — a `#[tauri::command(async)]` wrapper that only delegates.
3. `app.rs` — an entry in `tauri::generate_handler![...]`. Forgetting this compiles fine and
   fails at runtime.

A feature backed by a Tauri plugin has a fourth file: `capabilities/default.json` must grant
the permission (`clipboard-manager:allow-write-text`, say). A missing grant compiles and fails
only at runtime, inside the webview.

### TypeScript mirrors are hand-written and unchecked

`ui/src/lib/api.ts` mirrors the serde structs in `photon-engine`'s `commands.rs` and
`events.rs` (camelCase).
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
(inside the root when a drive's root is watched), Synology's `#recycle` and QNAP's `@Recycle`
(inside it when the share is), matched as a whole component, without ASCII case, at any depth;
`.Trashes` and `.Trash-1000` are dot-names already. Not XP's `RECYCLER`, and no other plain
word: matched without case it is a user's own folder "Recycler", whose photos the upgrade would
purge (`a_recycle_bin_is_not_walked` walks one). They are skipped
because of the feature below: deleting in Explorer or over SMB is a rename on the same volume
that keeps size and mtime, so the row was followed *into the bin* and the deleted photo stayed
in its albums and person views until the bin was emptied
(`a_photo_deleted_into_a_recycle_bin_leaves_the_library`). The watched root itself is never
passed over, whatever it is called (`a_watched_root_named_like_a_recycle_bin_is_scanned`; the
exemption had no test before it). A photo an earlier photon indexed inside a bin is no longer
found, so the next two scans that reach the folder above it mark it missing and purge it:
intended (`a_photo_indexed_inside_a_recycle_bin_by_an_earlier_photon_is_purged`). Except when
those were the watched root's *only* photos: the walk then sees no file, the empty-root guard
in `scan_watched` reads a root with rows and no file as an unplugged drive, marks it offline
and returns before anything is marked or purged, and the rows stay until a photo appears
outside the bin. That is the guard, not the bin rule, and it is why those fixtures hold a
`keep.jpg`.

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
Full-size renders run one at a time (`RENDERING`, in `photon-engine`'s `engine.rs`: `/image`
in `protocol.rs` takes it and export shares it - held
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

**The viewer's photo has an area, and it is not always the window.** While the info panel is
open the photo is fitted beside it (`lib/viewer-layout.ts`: `infoRoom`, `photoArea`), in
`.area`, which clips, so a zoomed photo stops at the panel too. Three things are measured in
that area and must stay so: the frame the photo and the face outlines are fitted into, the
zoom's and the pan's bounds (`viewport()`, worked out from the viewer and `info` rather than
read off the element, which is laid out a render later than the toggle that needs it), and
the point a zoom holds still (`fromCentre`: measured from the middle of the window, a
double-click beside the panel zoomed to a place half the panel's room away). Three more
things stand outside `.area` and are given its width by hand: the slideshow's crossfade
layer (`.outgoing` - the panel can be opened during a show, and a leaving photo fitted to
the window jumped larger on its way out), the error message, and the video's controls
(`VideoControls`' `room`). Anything new drawn over or instead of the photo is a fourth. The panel's
width and place are set on the element from the same constants. The previous and next
buttons are `aria-disabled` at the ends, not `disabled`: a button disabled while it holds the
focus drops the focus to `<body>`.

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
as everywhere else. The page's crops are `/face/<face id>/<thumb key>/<px>` in `protocol.rs`
(one size is cut, `CROP_PX`, twice the 72px the page draws; it is in the URL because the
webview keeps a crop for ever, so a new size must be a new name, and the UI's copy of the
number is held to the backend's by `people.test.ts`), cut
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

**A hidden sidebar is still there.** The top bar's first button and Ctrl+B
(`togglesSidebar`) close its two columns and stand it out of the flow at the width it had,
`visibility: hidden` and `inert` (`.no-sidebar` in `App.svelte`). Not `display: none`, which
resets the list's scroll position, and not a column of no width, which lays the whole list
out again as a sliver: unseen at its own width it goes on following the grid, and comes back
where it was. That is why `.sidebar`, `.splitter` and `.content` each name their grid column -
placed by order, taking one out of the flow moves the photos into its column. Hiding closes
the sidebar's menus and moves the focus to the photos *before* the list is inert, which is
also what stores a name being typed in a rename field. And anything that needs a row of the
list shows the sidebar first: "Rename in photon…" on a grid header does
(`renameFolderFromGrid`), because an inert list silently refuses the focus.

**The sidebar's width and open groups are in `localStorage` alone** (`lib/sidebar.ts`), not
in the settings table: they are how this window is laid out on this machine, and read there
they are known before the first paint, with no mirror to keep in step. The width is stored
when a drag or a key sets it, never when a narrow window clamps it. A sidebar rename field
closed from the keyboard hands the focus to its row (`backToRow`; to the grid, when the rename
was begun from a header there), and only while the focus is
on `<body>`: the field is removed while focused. While its write runs a field is `readonly`,
not `disabled`: one editor serves every row of its list, so a second rename begun during the
first one's write opened a disabled field that could not take the focus, and the name typed
for it went to the grid, where `h` hides the selected photo. And a held Enter does not press a
sidebar row twice (a listener on the list): the focus handed to a row arrives under a key
that is still down.

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

**A section's header is drawn twice.** In its row, and - once that row has scrolled away -
pinned over the top of the grid (`pinnedHeader`, a sticky box of no height, so the rows it
lies over keep their numbers); one snippet draws both, and both open the folder's menu
(`FolderMenu.svelte`, the one the folder's sidebar row opens, so the two cannot come to offer
different things - its Rename is the sidebar's inline field, reached through
`FolderTree.renameFolder`). Because something now lies over the grid's top edge, "in view" is
below it: `scrollToOffset` goes through `scrollIntoGrid` ('nearest') and `scrollToStart`
('start') with the header's height as the inset, or the row the arrow keys had just selected,
or the folder a sidebar click jumped to in the middle of a month, was half under the header. A
press on the pinned header starts no rubber band, since it would begin on photos nobody can
see. The folder at the top of the grid is also published as `gridPlace.folderId`
(`grid-place.svelte.ts`), which the sidebar marks (`.here`, a bar - not `.active`'s fill, which
is the current *view* and is on screen at the same time) and scrolls to keep in view, except
while the pointer is over the list or a name is being typed in it. Not "while the focus is in
the list": a clicked folder row keeps the focus, and the list would then stop following the
very scroll that click began.

**A context menu is `Menu.svelte`**, which every one of them is drawn by (the tile's, the
sidebar's three, the viewer's, the People page's merge list): the surface, the placing, the
keys and the focus. Its items are the caller's plain markup (`<button role="menuitem">`, with
`.hint`, `.sep`, `.heading`, `.sub`, `.note` and `.danger` styled from the component through
`:global`), and whoever opens it still holds its state and closes it. It is placed by
`use:fitMenu` (`lib/menu-place.ts`), never by binding `left`/`top` to the pointer: it opens
down-right and flips on an axis where that would leave the window, re-placing itself when it
grows (the tile menu's copies line arrives after it opens) or the window resizes; a menu bound
straight to the pointer is clipped near the bottom or right edge. The arrows, Home and End move
through the enabled items (`menuStep`). **Every plain key pressed in a menu stops there**
(`menuOwnsKey`): the viewer listens on the window, and with its menu open an `h` hid the photo
behind it. Escape is let through, since the opener's window listener is what closes the menu,
and so is a chord, since App must still prevent Ctrl+A. On closing, the menu hands the focus
back to what had it, but only while the focus went with the menu - an item that opened a
dialog or focused the grid has already said where it goes - and the grid passes `restore` so
it is the viewport that gets it, never the right-clicked tile: a tile must not be focused from
script (it draws a ring beside the selection's) and is recycled as the grid scrolls. The
opener keeps one `{#if}` block and replaces the point, so a menu re-opened elsewhere while open
is not remounted: the effect that takes the focus reads `at` for that, and without it the
second right-click left the focus on the tile, where an arrow moved the selection and `h` hid
photos behind the open menu. The function keys are let through with Escape and the chords
(F11). A component with a scoped `button`
rule (the People page) still gets the menu's look, because the menu's item selectors out-rank
it; a new item class has to be spelled `button[role='menuitem'].x` to do the same, as `.sub` is,
and a rule for a child must set every property it relies on - the viewer's own scoped `.sep`
(the toolbar's upright) gave the menu's separator its 1px width until `width: auto` was said.

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
`crates/xtask/screenshots/`) standing in for Tauri's IPC, and writes forty-four PNGs, in both themes,
to `target/screenshots/` with headless Chromium. It claims a Windows user agent and maps
`photon.localhost` to its own port, because `mediaUrl` uses `http://photon.localhost` there
and no plain browser can load `photon://`. It is Chromium's rendering, not WebKitGTK's or
WKWebView's, so it replaces no item of the smoke checklist, and it is not run in CI. A new
IPC command must be given an answer in `mock.js` (`canned`, or `SILENT` when nothing draws its
result): a test in `screenshots.rs` reads `api.ts` and fails otherwise, because an unanswered
command resolves to null and the screenshot of what the UI makes of that reads as a styling
bug. A new surface worth seeing is a new entry in `SHOTS` and, if it needs a click, a new
action in `mock.js`.

### The native UI (branch `native-ui`)

`crates/photon-ui` is the interface being rebuilt in Rust on egui and wgpu, in place of
`ui/` and the Tauri shell (spec `2026-10-09-photon-native-ui-design.md`). Its binary is
`photon-native` until the switch-over. **It is not launched to verify a change either**:
`cargo run -p xtask -- native-shot` writes the window to `target/screenshots/native-*.png`
off screen, and those are read. Its library's photos are dated by the day their copies
are given (`FOLDERS` in `tests/screenshots.rs`): they carry no capture date, and copied
they were all of today - one month, one year, no year strip. It needs a GPU adapter (no display), so it is an ignored
test that xtask runs, not part of the gate.

Two kinds of module, and `state_modules_name_no_egui_type` holds the line between them:
*state modules* are plain Rust tested without a context (`grid/layout.rs`, `motion.rs`,
`scroll.rs`, `thumbs/loader.rs`, `textures.rs`, `tasks.rs`), and *views* draw one and turn
input into calls on it, tested in whole frames without a window (`Context::run_ui`, or
`egui_kittest` where the application itself is run, as `tests/app.rs` does). The test's
list (`STATE_MODULES` in `lib.rs`) is kept by hand: a new state module is added to it.

`eframe` is pinned to an exact version and its API moves between minors: read the pinned
version's source, never an older egui from memory. `eframe::App` is `fn ui(&mut self, ui:
&mut egui::Ui, ..)`.

**Nothing that reads SQLite runs on the UI thread.** It goes through `tasks::Latest`, which
answers only the latest question. `Engine::published` and the `GridIndex` are in memory and
are read directly - `published` when the engine says the library changed, not per frame.

**A notch of the mouse wheel is as far as the web view of the system moved it**
(`scroll::wheel_notch`, `Notch`; `wheel.rs` reads Windows' setting). A wheel reports
notches and leaves the distance to the application: egui's is forty points, which is
WebKitGTK's too, and WebView2's is a hundred at Windows' own three lines a notch - so the
native grid looked right on Linux and was two and a half times slower on Windows, which
was the first thing said by the first person to run it (2026-10-10). A hundred for
everyone was the first answer, and the second thing said was that it was still not the
web view's speed with the lines turned up (2026-10-12): **WebView2 goes by Windows'
setting, a third of a hundred pixels for each line**, a page of what is in view at "one
screen at a time" (`WHEEL_PAGESCROLL`; all but forty points of it, Chromium's rule) and
nothing at nought. winit reports whole notches whatever the setting, so photon reads it
itself (`SystemParametersInfoW`, the one call it makes of the system, through the
`windows-sys` winit already brings): when the window is made, and **again whenever the
window comes back to the front**, since the setting is changed in a window of the
system's and photon is not told. egui is told the points in every frame (a page is a page
of the window as it is then), for the frames after. Anything measured against the Tauri
photon is measured on each system: the test of this through the whole application asks
the machine it runs on, and a test may not change a machine's setting, so the rule is
tested through `App::with_wheel`, which says what the system would. On Linux and macOS
the forty stands; a Mac's wheel has not been compared.

**The grid has no scroll map.** Its position is an `f64` in the layout's coordinates and
the rows are drawn at `row.top - position`; egui's `ScrollArea` is not used for it, because
its offset is an `f32`. Several rules of the Svelte grid have no successor (`placeIn`,
`viewTop`, pages, mounting): the spec of the slice lists them so they are not ported.

**A thumbnail not built yet is waited for, never blocked on** (`thumbs/loader.rs`): one
thread polls every pending `request_async`, as `protocol.rs` learned to. Three rules a
review added, each with its test. A built thumbnail is handed back only under its own key
(`thumbs/source.rs`): `request_async` answers with the photo's picture as it is *now*, and
keys recur, so one stored under the key that was asked for would be drawn for another
picture. A result is not read again between being handed back and being taken (`handed_back`):
the grid lists what it wants every frame. And *not there yet* is not *cannot be made*: a
video with no poster or a photo whose drive is unplugged answers `Unavailable`, which marks
the tile (`troubled`), counts as settled, and is asked for again after `RETRY_SECS` - by a
frame the grid requests for that moment, since a still grid draws none.

**Text goes through `text::paint_line`**, not through egui directly, wherever it can be in
another script: egui shapes text but runs no bidirectional algorithm, and draws a name that
mixes directions wrong. A right-to-left run is cut at everything that is not a letter, not
only at spaces, with what stands between two words turned round and its brackets mirrored.
The tests lay text out with egui's own fonts, which draw everything with one face: they
hold the cutting, and what a second face does is seen only in `native-shot`'s pictures or
by shaping with real faces, as the review that found the hyphen did. Not covered: a text
*field*, and cutting a right-to-left line short at its start.

**The lockfile ties two crates to one `windows` crate, by hand.** gpu-allocator takes any
version from 0.53 to 0.62 and wgpu-hal exactly 0.62; Tauri brings 0.61 into the same lock,
and cargo gave gpu-allocator that one, whose Direct3D types are not wgpu-hal's. It fails on
Windows only, as type errors that name no version. `the_gpu_crates_are_locked_to_one_windows_crate`
says so on every platform, and its comment says which line of `Cargo.lock` to change.

**A test that reads a file as text reads CRLF on Windows.** The runner's checkout converts
line endings and `include_str!` hands them over as they are, so a match on text with a `\n`
in it finds nothing there, and nowhere else: two of this crate's tripwires failed on Windows
alone for it. Read by line (`lines()`, `trim()`), and give the reader a test that feeds it
both endings (`the_lockfile_is_read_with_either_line_ending`).

The stored theme and tile size are read in `App::new`, before the first frame, and not by a
task: read after it, a theme pinned against the desktop's showed the desktop's for a frame
at every launch. A screenshot sets its theme twice, stored in the library and given to the
`egui_kittest` harness, because the harness sets one of its own (dark) after the
application has been made.

**The window is four areas around the grid** (`window_layout.rs`: the top bar, the sidebar
with its splitter, the status bar, the content), drawn by `shell.rs`, a view that answers
what the user did as a list of `Action`s; `app.rs` is the one place an action changes
anything. What needs a later sub-project is drawn in place and takes no press (the gear):
a button that will be there is there, so nothing moves when it starts to work
(spec `2026-10-10-photon-native-sidebar-views-search-design.md`).

**A step that moves the engine's view goes through `tasks::Queue`, never `Latest`.** The
view setters rebuild the grid on the thread that calls them, and a step begun cannot be
taken back, so they are made in the order asked; `Latest` keeps one waiting question and
would put a view clicked in the place of a sort still waiting, which would then never be
made. `nav.rs` holds where the user is: `settled` is what the grid on screen shows, and
`target` is where the last step asked leads - which is what the sidebar marks, at the
click and not when a rebuild of the whole library has landed. A step to where the user is
already going is not asked (`Nav::wants`). `grid_info` reads SQLite on a cache miss
although it returns no `CmdResult`: the sidebar's counts are a `Latest`, asked again at
every change.

**What a grid shows is known from the step that built it, never read from the engine when
the grid arrives.** `Engine::view_and_arg` and `sort` move when a step *begins*, a whole
rebuild before its grid is published, so with two steps on their way the first's grid
arrives under the second's view: read then, the starred photos were drawn as All photos,
at the place All photos had been scrolled to (a review found it; the Svelte store avoids it
by awaiting each step's `grid_info` before the next). So the queue's worker reads the view
when its step is done and before the next begins, and hands it back with the version the
setter answers (`nav::Landed`); and a grid published while a step is on its way is not put
on screen until an answer vouches for it (`Nav::adopt`, `App::adopt`). **An answer vouches
for the grid it names, and for a later one only when no step is on its way**: with the
first of two steps answered and the second still being made, a grid newer than the first's
may be the second's own, and taken as the first's it stood under the wrong name for good,
the second's answer finding no new grid to put on screen. Windows' CI found that by timing,
in a test of four quick clicks that had passed a hundred runs on Linux; the rule's test is
`a_grid_newer_than_the_last_answer_waits_while_a_step_is_on_its_way`, in `nav.rs`, which
needs no timing. All of it is exact only while nothing but the queue moves the engine's
view: **a new caller of a view setter goes through the queue.**

**How the window is laid out is in `layout.json` beside `library.db`** - the sidebar's
width, whether it is hidden, which groups are open - and not in the settings table, whose
accessors are photon-core's and which the Tauri photon reads too. It is read before the
first frame and written off the UI thread when the user changes one of the three; the
width is stored as it was dragged and clamped where it is shown.

**The sidebar is one list, drawn a window at a time** (`sidebar/list.rs`, a state module,
and `sidebar/view.rs`). Every row is an `Entry` with what it is (`What`, its identity from
frame to frame and what a click answers), and a height; `Stack` adds them up and the view
draws the entries its position takes in, as the grid draws rows. The position is the
view's own number, kept by `Shell`, so a hidden sidebar is not drawn at all and comes back
where it was. Building the entries is an allocation each, so `List::follow` builds them
again only when a key of what they were built from has moved - the counts, where the user
is going, the folds, the sort, the grid's layout generation - and the two sources too large
to compare, the collections and the folder list, live in `Held`, which has no way to
replace either but a setter that moves its number. A new thing the list is built from goes
into `Sources` and into its key; left out of the key, the list goes stale and no test of
the others will say so. The stack is *not* cached: it is a sum over the entries, and a
cache keyed by list, width and scale was removed when no test could tell it was there.
A press that lands a few points beside a row is the nearest row's: egui gives it to the
nearest button in reach, so the room between two rows is not dead as it is in a browser.

**A folder is gone to in the grid that holds it** (`Nav::jump`, `Nav::home_of_folders`,
`GridView::go_to`). A click on a folder from an excursion asks for All photos and keeps the
jump with the number of that step; `Nav::adopt` hands it out with that step's grid and with
no other, and the application looks the folder up in the index it has just put on screen.
Hidden is left as it is. The offset is given to the grid in the same frame as the index it
was looked up in, and the grid takes it once its rows are laid out. `LastFolder` (also
`nav.rs`) is the folder photon remembers: read once before the first frame and kept in
memory, since the application is its only writer - which is why the way back from an
excursion reads nothing, where the Svelte UI has to read the setting before the switch can
overwrite it. The launch goes back to it in the frame its first photos are on screen and
before they are drawn, so there is no frame with the grid at its top for `at_top` to
remember; a guard for that frame was removed as doing nothing. A run of the gate keeps no
folder at all (`LastFolder::off`): its programme starts from the top, over a library the
Svelte photon leaves a folder in.

**The mark of the folder at the top of the grid is a frame behind the grid**, the sidebar
being drawn before the grid has taken its place, and the application asks for the frame
that makes it up (`top_folder != marked`). Nearly always one would follow by itself: the
grid moves on input, on a task's answer or in a scroll, each an immediate request, and egui
draws two frames for each of those - which is why taking the request out changed nothing
any test could see, and it was taken out. A review put it back with the case that needs
it: eframe draws one frame for a window resize that nothing asked egui for, a grid at the
end of a library is held to its new end in it, and the mark was a folder behind for the
hundred and fifty milliseconds until the grid's own report of what is in view
(`the_mark_is_brought_up_to_a_grid_that_moved_in_a_frame_nobody_asked_for`). "Each way a
thing can move asks for a frame" has to be checked against eframe's own frames too.

**The list is held by an entry when its entries change** (`Kept`, in `sidebar/view.rs`):
the row under the pointer, else the marked folder's while it is in sight, else the first in
view, by what it is and not by its index, and the list is put where that entry is where it
was. The albums are read a moment after the folders, and at launch the list has by then
followed the grid to the remembered folder, flush at the bottom edge: their rows went in
above and pushed it out of sight, with nothing to bring it back, since the list follows
once for each folder. A fixture's lists are read inside a frame, so no test of the whole
application saw it; a review did, with a sleep in the task.

**A folder asked for is kept as the folder until the frame that draws it** (`App::going`,
`take_folder`), not as its offset: a click is acted on after its frame is drawn, and the
next frame may draw a scan's grid with photos more above the folder. Everything that sets
a place in the grid from outside is kept that way - a folder in `going`, the photo the
viewer closed on in `showing` (`take_photo`) - and taken after `adopt` and before the
grid is shown. The viewer's close handed the grid an offset at first, and a review's test
put a grid of three hundred photos more between the Escape and the next frame. A row half scrolled out is drawn and pressed through a child clipped to the
sidebar: `Icon::paint` paints through the `Ui` it is given, and through the shell's the
icon of such a row stood on the top bar.

**The collections are not asked for at launch**: the first grid the engine publishes is a
data change, built or failed, and that is what reads them. A row tells AccessKit whether
it is where the user is (`WidgetInfo::selected`), which is how a test of the whole
application sees the fill; a test that clicks two rows in a row draws a frame between
them, because the second is clicked where the last frame drew it and a fold moves it. And
a test that waits for something *not* to happen settles after the clock has passed the
moment it was due, not only before: `frames_in` jumps the clock without waiting for the
other threads, and a search sent in it has not been answered when it returns.

**The search box is three modules.** `search_box.rs` (state) holds the text and the send
that typing has made due, on a clock that is a number; `search_help.rs` (state) is the
grammar's list, string for string `search-help.ts`'s, held to `Query::terms` and to that
file by its tests; `search_bar.rs` draws egui's own text field, the two buttons and the
panel. A typed search is a `Step::Search` on the views' queue, taking the place of a search
still waiting there (`Step::is_search`) and of nothing else. **Whatever switches the view
tells the box first** (`App::go`: `cancel` before anything, `leave` as the switch is
asked for, `search` for a row that is a search), or a search half typed lands after the
switch and replaces the grid that was asked for; a folder click and All photos cancel too.
The box's text is written from this side alone: it is never read back from the engine.
Things about egui's field that cost a test, a picture or a review each. Escape is claimed
for the field (`TextEdit::event_filter`): left to egui it takes the focus away at the start
of the frame, before anything is drawn, the box cannot tell it had had the keys, and the
grid hears the key too. The field stands at the top of the room it is given, so it is given
a room one line tall, and a press on the rest of the field is answered by a second,
unfocusable widget under it. An `Area` starts at egui's default size, four hundred points
tall, and keeps the size it was first given: the help panel says how tall it may be every
frame. **The `/` that opens the box is let go of by whatever key comes up, and by the
window being left** - not by a release named Slash: where `/` is Shift+7 the key that comes
up after Shift is `7`, the slash was held for good, and every `/` typed in the box was
swallowed, `f/1.8` among them. And **the grid's keys are a text field's while it has the
focus, its scrollbar is not** (`grid/view.rs`): the early return that made the grid deaf
stood above the scrollbar too, and with the caret in the box the thumb did nothing - egui
gives up a field's focus on a click elsewhere, not on a press that is held. A press on the
bookmark is a write, made on a queue of writes (`App::writes`) and followed by a read of
the lists, since the engine announces no data change for a search saved; until they are
read the search does not look saved, so the box itself knows what is being saved
(`SearchBox::save`, `can_save`) and a second press saves nothing. **A window with the
caret in a text field is not still**: the caret blinks, about six frames a second, as in
any application. `egui_kittest` switches the blink off, so no test of the whole
application sees it, and `a_window_with_a_search_shown_draws_nothing` leaves the box with
Enter first to say what it does hold: a search shown, with the keys back in the grid.

**The controls at the right of the top bar hold nothing of their own.** `controls.rs`
(state) is what they offer and what a choice makes of the sort, `controls_bar.rs` draws
them, and the sort drawn is `Nav::sort_target`: the sort the last step asked leads to, as
`target` is the place. A control answers that sort with one field of it replaced, and a
change of sort is a `Step::Sort` on the views' queue, where it gives way to nothing and
nothing gives way to it - so a second change made before the first has landed builds on
the first, and a refused one puts the controls back. A sort moves no view and clears no
query: `App::go` leaves the search box alone for it. **A sort's error does not always mean
nothing moved**: `Engine::set_sort` rebuilds and publishes, then stores, so a sort it
could not store is on screen all the same, and the queue's worker answers that as a step
made, with the engine's words for a message - taken as refused, the photos were in one
order and the controls said the other for the rest of the session. And anything asked
*behind* a sort on its way asks `sort_target`, not the sort on screen: All photos did
not, and came back to the remembered folder in an order that has no folders. The size is not a step: it is
applied at the press and stored on the queue of writes, and a size the library would not
store is kept for the session, as the Svelte store keeps it.

**The dropdown is `select.rs` and `select_view.rs`**, the Svelte `createSelect` and
`Select.svelte`. The control has the keyboard and the list never does. What egui does
around a focused widget decided most of the view, each learnt from a test that failed or
a probe that passed. A click does not give a widget the keyboard: the control asks for it
at a press. Enter and Space on a focused widget are a click to egui, and so is the press
a screen reader makes: the keys are read as keys, and a click in a frame with Enter or
Space in it is not a press. **Tab is read as the control loses the focus**
(`lost_focus`): egui has moved the focus on by the time the control is drawn, and read
only while it had the keyboard, Tab closed the list without taking the option it was on.
The up and down arrows are claimed (`set_focus_lock_filter`), or egui walks the focus to
a neighbouring widget with them; while the list is open so are the sideways ones, which
do nothing in it and otherwise closed it by walking the focus off, and Escape, or egui
drops the focus before the control sees the key. A key the list used is taken out of the
frame's input - a letter typed with the key that typed it, as well as an arrow - because
the grid is drawn after the bar and would scroll by the same arrow, and will one day hide
a photo by the same letter; which is why the controls are drawn after the search box,
which has read its own keys by then. **A pointer that closes the list gives the keyboard
up**: by choosing from it, by pressing the control again, by pressing elsewhere. egui
lets go of the focus only at a click on something else, never at a wheel turned or a
scrollbar dragged, so a sort chosen with the mouse left End opening the list from
wherever the grid had been scrolled to, and Enter after it sorting by size (a review
found it). A screen reader's press is no pointer and leaves the keyboard where it is. A
dimmed control is no stop of the Tab key, where the Svelte one is focusable and inert.
**The list closes at a press outside it, by the view's own rule**: egui gives up the focus
at a *click* elsewhere, which is the button let go of, and never at a press that becomes
a drag - with only the "no keyboard, no list" rule, a list stayed open over a scrollbar
being dragged. A click on the list is a click elsewhere to egui, so the list is spared for the frame of
that click and for nothing else: a pointer merely resting on it does not hold it open
when Ctrl+F takes the keyboard. The pointer makes a row the active one when it moves
(`pointer.delta()`, not `is_moving()`, which is a smoothed speed that one step after a
rest does not move), not while it rests where the list opened. And the list is as wide as its longest option needs in a row, where the mark's
room is more than the chevron's: at the control's own width it printed "Date modi…",
which a picture showed and no test had (`Galley::elided` is how a test sees a text cut
short; `Galley::text` is the whole text either way).

**The year strip is asked for before the rows are laid out, because it decides how wide
they are** (`GridView::strip`): `shows_timeline` wants the number of year marks, and
`timeline::year_runs` counts them from the sections alone, held to `year_marks` by a test
that runs both. The count is kept for the layout generation it was made in. The strip
stands at the grid's right edge with the scrollbar left of it, as in the Svelte grid, and
the photos have what is left. When the strip comes or goes with a new index the tile
changes with it, which is the pin's own trigger - so the place carried over is the
photo's *offset*, from one index into another: right while nothing arrived before that
photo, and off by what did otherwise, as the kept number is across any new index (a
review's test with a folder of a newer year; the grid does not follow a photo by id). A press on
the strip is the primary button's and follows the pointer on or off the strip
(`is_pointer_button_down_on`); the year under the pointer is named over the photos, so it
is painted last and through the grid's own painter. It is no stop of the Tab key
(`Sense::CLICK | Sense::DRAG`). A guard against a strip of no height was removed: nothing
divides by it that a test or a panic could tell.

**The status bar's lines are `scans.rs` and `status.rs`**, what the Svelte store holds of
the engine's reports and `status.ts`. The application gives `Scans` every report and is
told what to do next (`Asks`): read the folder list again, read the published grid
again. The Svelte store's rules are there with their reasons and tests - the list read
once for a scan in a folder it does not hold, a done from a drive that is only polled
being no scan ending, a notice only for a folder still watched. **One is not: what a scan
is measured against is said by the scan** (`ScanProgress::known`, on every report but a
full scan's first, and let through the engine's quarter-second throttle the first time it
is said). The Svelte store counts the folder's photos when it hears a scan's first
report and takes out what that report had added; this window hears the report when a
*frame* takes it, and a first scan writes five hundred photos every few hundred
milliseconds - a review's test, with no frame drawn for 400 ms, read "3,000 of ~500
files". Only the scan knows what its folder held before it. **The bar of a first scan
has no clock** (`Bar::Unknown`): it steps once for each report heard of the scan, because
a bar that moved by itself would draw a frame sixty times a second for as long as the scan
ran - by the reports and not by the files they count, which arrive in any number. The
bar is one row: the count is drawn first and the lines have what is left, and a line with
no room for its words is left out with its bar.

**A number is shown beside the thing it was read with, or not at all.** Three times in one
review the window paired a number with a state it was not read for: the count above; the
hidden photos, counted a round trip after the grid that lost them was on screen, so every
photo hidden at once said "photon has found no photos" for that long (`App::counted` is
dropped when a grid that held photos comes back empty); and a folder's `online`, read
with the folder list a frame after the scan that changed it said it was done, so a drive
just back was one photon "cannot reach" (the scan's `FolderStatus` is written into the
list on hand at once). The Svelte UI gets the first two for nothing, from one answer that
holds both numbers.

**The launch is the one thing an empty library has to wait for that the Svelte UI never
met** (`scans::Launch`, `Engine::startup_scans_started`). The engine publishes its first
grid, *then* watches the Pictures folder and starts its scans; a webview is nowhere near
loaded by then, and this window is drawing. For those moments it has an empty grid, a
folder list with nothing in it and a count of hidden photos - everything it would need to
say "Choose a folder" over a library a moment from being scanned, and "No folders yet."
beside it. So nothing is known of the folders until the list *read after* the engine says
its first scans are started, and each folder of that list counts as being scanned until
its scan has been heard from, or the folder is no longer watched. A scan reports at least
its end unless it dies: one that panics says nothing more, the engine does not say so, and
an empty library waiting for it goes on looking. The engine is asked every fifty
milliseconds until it says its scans are started, since nothing reports the moment.
**The seam for all of it is the library's write lock**, taken from a second `rusqlite`
connection in a test: held, the engine's next write waits (a scan stands at its first
write with its reports made, the startup thread before it has watched the Pictures
folder); held past the five seconds a write waits, the write *fails* - which is the one
launch that sends no event at all, and the test of the fifty-millisecond asking and of
the list asked for again (a review found that one). A test that waits to *see* something
during a scan is held that way and never raced: on macOS' CI a scan of eight hundred files
was over before the window had drawn its second frame. `Driver::settle` spins while the
launch is being asked about - a frame is always due within fifty milliseconds - so a test
that holds the launch waits in real time instead. **What the Svelte UI's
`scanning_folders` and `noteRunning` are for does not exist here**: the channel is made
before the engine is opened, so no report is sent before someone listens.

**The empty library's panel is `empty.rs` and `empty_panel.rs`**, and the view of it in
All photos and Recent alone; every other view has its one line. "Add folder…" and
"Watched folders…" are drawn fainter and sense no click (`PanelButton::works`), and "Or
drop a folder onto this window." stands under them only where a drop can arrive
(`LibraryFacts::drops`, below): a line that offered one where none arrives would be
false, not inactive. The block is centred, so a longer sentence
pushes its buttons down, as in the Svelte grid; its sentence is wrapped by egui and does
not go through `text::paint_line`, so a folder's path in another script is drawn in the
order it is written. The sidebar's notes - the empty groups', and "No folders yet." -
tell AccessKit their words, which is how a test of the whole application reads them.
"No folders yet." stands at the folders' own inset with air over it, over a button to
Settings that is drawn and takes no press: at a note's indent it read as the note of the
group above it.

**The selection is `selection.rs`, and most of the Svelte store's has no successor**
(spec `2026-10-11-photon-native-grid-interaction-design.md`). The ids picked, the lead
(the photo the keys move on from) and the anchor (the photo a Shift+click ranges from),
each of the two as an offset *and* an id. Every call is given the index on screen: this
window holds the whole `GridIndex`, so a range's ids and the photo a key lands on are
read in the frame that asks, and `fetchIds`, `extendCall`, `reaching`, `picks`,
`adoptLead` and the landing lookups of `setHidden` are not ported - each guards a round
trip that does not exist here. **A new grid of the same view is followed** (`follow`,
given the old index and the new, in `App::adopt`): photos the new one does not hold
leave the selection, the lead and the anchor are found again by id, and a lead whose
photo left lands on the nearest that stays - the first after it in the old grid that the
new one holds, else the last before - and is selected there if nothing else is. It does
not ask what rebuilt the grid, which is why a hide, a star taken off in Starred and a
scan's purge need no rule each. **A grid of other results clears it**
(`Shown::other_results`: another view, search or order). An offset of the lead or the
anchor read after a new grid without `follow` names another photo. The photo landed on
is brought into sight only if the one that left was on screen in the last frame: a scan
purging a photo while the user is elsewhere must not carry the grid back to it.

**The grid has no widget for a tile**: it paints rows, one area senses the click, and
`tile_at` (in `grid/layout.rs`) says which photo a point is on - none in a gutter, on a
header or beside a short last row, and the pinned header's rectangle is taken out by the
view, since a photo lies under it. A click on no photo selects nothing and clears
nothing. The arrows, Home and End move the lead (`move_from`, `nav.ts`'s `move`: with
nothing selected a key starts from the first photo) and the page keys a screenful
(`page_move`). Until this the up and down arrows, Home, End and the page keys scrolled
the grid, and the sideways arrows did nothing; with nothing selected the page keys
still scroll. The grid follows the lead by the least movement, under the pinned
header's height. The keys are read from the frame's events, in order, repeats included -
but not Escape's repeat, and not a chord with Ctrl, Cmd or Alt, but for Ctrl/Cmd+A.

**The keys are the grid's only while nothing else has the keyboard, and had none when
the grid was last drawn** (`GridView::elsewhere`). The grid is drawn after everything
else in a frame and reads the frame's keys, and "unless a text field has them", which
was the rule while its keys only scrolled, had it answer every key a second time that
something else had just answered: the arrow that moved the splitter moved the selection,
Ctrl+A in an open list selected a folder behind it, the arrow that walked the focus from
a button the Tab key had reached moved the lead (a review found all of them, each with a
test). "Had none when last drawn" is for Escape: egui takes the keyboard off a widget at
Escape before anything is drawn, so in the frame of that key nothing has it, and the
Escape that left a control cleared the selection too. What keeps the keyboard by a lock
of its own and gives it up itself - the search box - takes its Escape out of the frame
(`consume_key`) instead. And what lies open over the photos with no keyboard of its own
says so (`GridData::keys`: the search box's help). A click on a photo gives the keys
back, egui taking the keyboard off whatever had it at a click elsewhere. **Every key the
grid gains goes through this one gate**: `.` and `H`, which write, came through it.

egui decides for itself whether a press is a repeat (the key has not come up), whatever
the event says: a test of a repeat holds the key down across frames. The held modifiers
reach egui by `Event::ModifiersChanged`, not by a field of `RawInput`: a test that
clicks with Shift sends it - and a click takes its modifiers from the release's own
event, not from what is held at the end of the frame, in which Shift may have come up.
`egui_kittest`'s `step` draws a frame for *each* queued event. Two events in one frame
of the whole application are put into the frame's input beside the queue
(`harness.input_mut().events.extend(..)`, then a step with nothing queued: `frame_with`
in `tests/dialogs.rs`); a test in whole frames of a view hands them over together.
**The area that senses the click takes
every press on the photos**, however near the scrollbar or the splitter: egui hands a
near miss to a neighbouring widget only where nothing is hit, so the splitter's reach is
its own five points now, as the Svelte one's is. A press held longer than egui's 0.8
seconds is no click (`max_click_duration`, which is also a touch's long press, so it is
left alone). A selected tile's ring is drawn inside the tile. The status bar says "N
selected" before the count from two photos on, and gives AccessKit those words, which is
how a test of the whole application reads them. A test presses a photo where
`App::tile_rect` says it was last drawn.

**The rubber band is a rectangle in the layout's coordinates** (`grid/view.rs`'s `Band`,
`take_band`), so it keeps its grip on the photos it began over while the grid scrolls
under it; its far corner is worked out again in every frame from where the pointer was
last seen in the window. `items_in_rect` says what it touches, by the rule `tile_at`
clicks by (a test walks a grid asking both of every point). The selection holds what the
band started from (`Selection::band_begin`, `band_to`, `band_end`, `band_cancel`): every
frame is that plus what the rectangle covers now, read from the index on screen - which
is why the Svelte band's second answer from the backend, its count of photos not yet
resolved and its abandoning when the grid was rebuilt under it have no successor: a
scan's grid arrives, `follow` takes what left out of the band's two sets, and the
rectangle covers what it covers in the new grid.

**A press on the photos is read from the button's own events, not from what egui calls
clicked and dragged** (`grid/view.rs`'s `Press`, `take_pointer`). The first version
asked egui, and a review found what that costs, each with a test: egui's click has a
time limit (`max_click_duration`, 0.8 seconds, which is also a touch's long press and so
is left alone), so a press held still for a second selected nothing; its drag ends at
the release of *any* button, and at Escape with anything held, so "no longer dragged"
was no evidence that the button had come up - Escape with Ctrl still down, which is how
an adding band is held, kept the band's photos; and after a release that is never heard
of the drag is stale for good, so the next one began no band. Now a press is the
photos' when the primary button goes down with the pointer over them and nothing above
them (`contains_pointer`, and the layer egui finds on top there: below, "one judge"), it
is a click when the button comes up without ever having
been a band, however long it was down, and a band once the pointer is further from where
the button went down than a click may be (`max_click_dist`). The corner a band begins at
is taken in the frame of the press, not in the frame it becomes a band: the wheel turned
under a button that is down and still had moved it two rows.

**A band has one teardown and four endings**: the button comes up, which keeps what the
band covers; Escape, which puts back what was selected before it; the gesture taken
away, which does the same; and the selection cleared under it by a grid of other
results, which leaves nothing to put back (the view finds the selection's band gone and
drops its rectangle). **Escape is read as the key being down that was not**
(`key_down`, an edge kept from frame to frame), not as an event: whoever read the event
first - the search box takes its Escape out of the frame - and whatever is held, the
band sees it; and the band takes the event out too, or the keys after it clear the
selection it has just put back. **A gesture is taken away without a release** in two
ways a window reports: `Event::WindowFocused(false)` - a button let go of over another
window may never be heard of, and a band that outlived it scrolled to the end of the
library selecting all the way, the Svelte band's old bug - and a touch the system
cancels (`Event::Touch` with `TouchPhase::Cancel`, which egui-winit sends with the
pointer gone and no button up; winit reports it on Wayland). The focus being *lost* is
the ending, not the focus being elsewhere: a window with the pointer and not the
keyboard draws bands. A pointer that leaves the window is no ending: it stays a press,
with no position (`latest_pos` is none), and the band keeps the last one it saw - the
edge it was scrolling towards.

**While a band is held it owns the window**: the application takes every key press
out of the frame before anything reads it (`GridOutput::banding`, of the frame before) -
Escape's too, which the band reads from the key being down. The grid's own gate was not
enough - Ctrl+F put the caret in the search box
under a band, and Ctrl+B built the rows again under its corner. A drag on the photos
takes the keyboard off whatever had it, as a click on one does: egui does that at a
click elsewhere and not at a press that becomes a drag. **The corner is a place in the
rows, and rows are built again**: a window narrowed under a held band - a tiling window
manager, the year strip arriving with a scan's grid - is other rows at other heights,
so the press's origin is held by the photo it was on, or the row where it was on none
(`hold_at`, `held_place`), as the grid's own place is by its `Pin`. The scroll at an
edge is the edge's speed times the time since the band's last frame, at most fifty
milliseconds of it, and a frame is asked for only while the pointer rests in a margin
*and the grid can move that way*: at its top or its end the band rests, and whatever
makes room again - the wheel, a longer grid - draws a frame that comes through the same
code. A band begun with Shift adds, as one begun with Ctrl does, though a Shift+click
ranges. No band begins in a grid with no photos, where the shell draws what an empty
library says.

**The native window writes through one queue, and tells the grid nothing**
(`App::writes`: `Write::Stars`, `Hidden`, `AlbumAdd`, `AlbumRemove`, `PersonRemove`, and
those of the pull requests after it). Each
is one of the engine's own commands, made in its turn off the UI thread, and the only one
that writes outside `library.db` is `commands::set_stars` - the `star=` line in the
folder's `.picasa.ini`. The photos acted on are read from the selection **at the pick**,
in the grid's order, not when a menu opened. What comes after a write is the engine's
rebuild, arriving like a scan's: `Selection::follow` takes out what left and lands the
lead, which is all of the Svelte store's `setHidden` (the landing after a hide, `H` again
going on from there, the last photo's neighbour) with no line here that knows a hide
happened. What a write's answer adds is only what a rebuild cannot say: a message when
fewer stars were written than asked (`stars_missed`: a folder whose INI cannot be written
costs that and not the photos beside it), the engine's own words for a refusal, what came
of taking photos off a person (`removed_message`), and a read of the lists after an album
changed - `albums_changed` announces a change only while an album is on screen, and the
sidebar's counts are wanted in every view.
`.` goes by one photo, `Selection::deciding`: the lead while it is selected, else the
selected photo that comes first in the grid. Neither `.` nor `H` is answered at its
repeat: each press writes, and a held `H` would hide its way through the library.
**The star's key is the key that types a period, wherever a keyboard has it**: egui names
a key by what it types and, for want of a name for that, by its place, so Shift with the
period's own key - `>` on most keyboards - is `Key::Period` too, and is told from a
period typed with Shift on a French keyboard by the event's `physical_key`.
**A key that writes carries the selection it was pressed on** (`grid::view::Asked`: the
ids, and the deciding photo). The application acts on it after the frame, when the
frame's later keys have been answered: read then, `H` and an arrow in one frame - a long
one, such as puts a rebuilt grid of a large library on screen - hid the photo the arrow
went to (a review's test). And in a frame with a mouse button in it no such key is
answered at all: a click is taken before the frame's keys, whichever came first, and a
write does not guess. **The window waits for its writes when it closes**
(`Queue::finish`, `WRITES_PATIENCE`): dropped with the queue, a hide pressed in the
instant before was lost without a word - and not for ever, since a write can hang.

**A menu is `menu.rs`, `menu_view.rs` and whoever opened it** (`App::menu`, `tile_menu.rs`
for what the tile's holds): `Menu.svelte`, `menu-keys.ts` and `menu-place.ts`. One menu
for the whole window, **shown before anything else in the frame**, because while it is
open the keys are its own, as a desktop's menus have them: it takes every key press and
everything typed out of the frame's input before the search box, the sidebar or the grid
reads it - a cut, a copy and a paste among it, which is what egui makes of their chords
and no key at all (nothing reads one under a held band yet; Copy photo will have to). A
chord too, where the Svelte menu lets one through to the window: Ctrl+F put
the caret in a box behind the menu, where nothing typed arrives. **Tab is not a key that
can be taken out of the frame**: egui decides its walk of the keyboard from the raw input
before anything is drawn, so the menu calls it off (`Memory::move_focus` with
`FocusDirection::None`), and so does the application under a held band. The menu is
placed in every frame from its own size, worked out from its items before it is drawn
(`place_menu`), so items that change under it and a window resized need nothing more. A
press outside closes it at the press, not at the click - a press that becomes a band is
never a click - and the press is left for what it landed on. egui fades a new `Area` in
over a few frames, each asked for: `fade_in(false)`. **The keys are on an item by its
number, and the items are built again in every frame**: when they are other items - a
grid took one of two photos, and two star items became one - the keys go back to the
menu itself (`signature`), or Enter picked what they were never taken to; and an item's
widget is named by its words as well as its place, so a press on one that is another by
the time the button comes up is no click.

**Whether a press is on something is asked of one judge.** egui answers "does this
widget contain the pointer" for one layer: a widget of a layer below still contains it
all along the inside of an area's edge, wherever no widget of the area covers the five
points round the pointer (`hit_test.rs`, `included_layers`). A menu opens with its
corner exactly at the pointer, which rests there - so a click in place was inside the
open menu and the photo's under it as well, and a drag from there drew a band under the
menu (a review found both). The grid now asks both `contains_pointer` and which layer
egui finds on top at the press (`Context::layer_id_at`), and the menu asks the same of
its own layer for "outside", not its own rectangle: egui puts an area on whole points
whatever it is asked for, and two judges half a point apart disagreed about the one
place every right-click leaves the pointer. For the same reason the menu's area is a
point larger all round than the menu drawn in it (`RIM`). An egui widget under an
area needs none of this: an area takes the clicks on its own ground. (`layer_id_at`
locks egui's memory: not inside an `input` closure.)

**The menu is asked for where the other button goes down**, read from that button's own
event as the band's press is (`take_pointer`, `Taken::menu`): egui's click of either
button gives up after 0.8 seconds, and a desktop's own menus open at the press. Not while
the first button is down - a press on its way to a click or a band is not interrupted,
and its release would change the selection under the menu - and not through anything
that lies over the photos, the open menu itself among it (above). On a Mac the first
button with Control held asks for it too (`asks_for_menu`): that is how a mouse with one
button asks, and the system reports it as the first button. A photo
outside the selection is selected first, alone: what a menu acts on is always what is
ringed. Opening takes the keyboard off whatever had it, as a click on a photo does.
**A menu does not outlive the photos it is of.** Its items are built again in every
frame from the selection, so a grid that loses one of several is simply a shorter
subject. It is closed when nothing is selected any more (a grid of other results; a grid
that lost its photos and had none to land on), and when `follow` lands the lead: the
photo the keys were on has left - which is the photo the menu was opened on, unless it
was opened inside a selection of several - and its items could act on the neighbour the
selection landed on, a photo nobody asked about. It closes more often than it must,
never less.

**The sidebar's rows and the grid's headers have menus, and the one menu is whose it
is** (`side_menu.rs` for what an album's, a saved search's and a folder's hold;
`App::menu` is where it was opened and a `MenuOf`). The items are built again in every
frame from the selection and the lists as they are then, so a menu whose album, search
or folder is no longer listed has no items, and closes - the same rule that closes the
tile's menu when nothing is selected. A folder's menu is one list on its row in the
sidebar, on its header in the grid and on the copy of that header pinned over the photos
(`header_at`, in `grid/layout.rs`, says which header a point is on; a period's names no
folder and has no menu). "Delete…" asks before it acts, through the dialog layer
(below); "Reveal in file manager" is in its place and takes no press. The other button on a row is read from its own press (`menu_press`), as on a
photo, and not through what lies over the list.

**A name is typed in the row itself, in the one field the sidebar has**
(`rename.rs`, `createAlbumEditor`; drawn by `sidebar/view.rs`'s `field`). One field keyed
by what it stands in for (`rename::Of`), where the Svelte list has three editors because
its rows' ids collide - album 3, search 3, folder 3. Storing a name is a write on the
queue of writes, whose answer comes frames later: meanwhile the field stays and takes
no typing, and the answer closes it only if it is still the field that was sent
(`Rename::landed`, by the number the field was sent under): a second rename begun
meanwhile is another field. A blank name is a cancel, but a folder's, which is how its
directory's own name is asked back. A name the library refuses leaves the field open,
under a message. **A field whose row is not drawn cannot hold the keyboard**: scrolled
out of sight its name is stored, as leaving it stores it, and with no entry left in the
list it closes; hiding the sidebar stores it too. The list does not follow the grid
while a name is typed, and the field's row is the entry the list is held by. "Rename in
photon…" on a header shows a hidden sidebar and brings the folder's row into sight
(`SidebarView::reveal`). Nothing here writes outside `library.db`: a folder's name in
photon is the library's `alias`, and the directory keeps its own.

**What a review of the field found, each with its test.** *While its name is stored a
field does not ask for the keyboard back.* Stored by Enter it still has it, so what is
typed next does not reach the grid, where `h` hides a photo. Stored by being left it has
given the keyboard to what the user went to: asked back in every frame, Ctrl+F out of a
name left the search box without it, the sort's list closed under the click that opened
it, and when the write landed nothing had the keyboard - the search typed next was the
grid's. *A name out of sight is sent once* (`SidebarView`'s `unseen`): a name the library refuses leaves the field open, still out of sight, and
sent in every frame it was a write and a message in every frame. *A field that was not
drawn in the last frame is opened* when it is drawn - its row scrolled back, the sidebar
shown again (`SidebarView::not_drawn`, which the shell calls for a frame it does not
draw the sidebar in) - and takes the keyboard: a refused name's field came back into sight
without it, and what was typed "into" it was the grid's. *The other button on the row a
name is typed in opens no menu*: it took the keyboard, which stored the name half typed.
*Enter is answered when it is no repeat, and every Enter is taken out of the frame*: the
Enter that picked "Rename…" in the menu is still down when the field opens, and its
repeat stored and closed the field before anything was typed. *A field opens with a
history of its own*: one field under one id serves every row, egui keeps a field's undo
by id, and Ctrl+Z in a folder's field brought back the name typed for an album. *What
was typed is said before what stores it*, by the field (a letter and Enter in one long
frame) and by the shell (a letter and Ctrl+B, which is read before the sidebar is
drawn). *The folder list is read again by a folder's write itself*: the engine rebuilds
nothing when no photo changed, and "Hide folder" on a folder whose photos were hidden
one by one left its menu offering to hide it. *A folder's menu is of a folder the view
lists*, and closes when it no longer is; a menu of the selection closes when the lead
lands on another photo, and a folder's does not. *On a Mac the click of a press that
asked for a menu is not the row's*, whenever Control is let go of: that is kept from the
press (`menu_button`), and can only fail on a Mac - checked by building as one.

**egui's text field, twice now.** What the search box learnt holds for this field, and
one thing the search box had wrong until the second field was built: **a filter built
from `EventFilter::default()` takes the arrow keys away from the field.** The default
leaves them to egui, which walks the keyboard to a neighbouring widget with them
(`TextEdit`'s own filter claims them; replacing it to claim Escape dropped them). In the
search box an arrow key moved the caret *and* handed the keyboard to the button beside
the box, and what was typed next was typed nowhere - since sub-project 2, unseen
because its frame tests draw the box alone, with no neighbour to walk to
(`the_arrow_keys_move_the_caret_in_the_box_and_the_keyboard_stays`, through the whole
application, is the test). Also: Enter and Escape are read and taken out of the frame
*before* the field is drawn (egui's field gives the keyboard up at Enter; an Escape left
in the frame is the grid's next); a press elsewhere stores at the press, by the view's
own rule; an empty field draws its placeholder in the frame it is first typed in.

**A dialog is `dialogs.rs`, `dialog_view.rs` and the application that holds it**
(`App::dialog`): one at a time, over the whole window - the question asked before an
album or a saved search is deleted (`Confirm`), and the keyword picker (`tag_picker.rs`,
`createTagPicker`); the export's and the person dialog have their own paragraphs below. **What a dialog acts on is taken when it opens**: the album or search
of the question, under the name it had; the photos of the picker. A menu's rule is the
opposite, and on purpose - a menu's items are built again in every frame and say what
they would act on now, while a dialog's words were read once, and what the user was told
it would act on is what it acts on (a scan can take a photo out of the selection behind
an open dialog; the keyword is written to the ids its title counted). **By number, which
is not for ever the same thing**: an id of the library's is handed out again once it is
free. For an album and a saved search the window closes that itself (below). For a photo
it cannot - a photo that is hidden has left the grid as well, and is to be written to -
so a picker left open while its photo is purged and another indexed writes to the other
one: improbable, and to be closed only by the library never reusing an id, which is a
migration. The picker closes
*before* its write, which is the user's last word on it, and the message afterwards is
the library's own count and the name it stored, which a rename rule can change.

It is drawn in egui's `Modal`, read from the pinned source: an area in front of
everything with a ground over the whole window, and from the frame after it is first
shown nothing under it is pressed, hovered or given the keyboard - Tab stays inside it -
and `layer_id_at` answers the dialog's layer wherever it is asked, which is the question
the grid and the sidebar ask of a press. **What `Modal` does not keep from what lies
behind is the keys**: the grid, the search box and the shell read them from the frame's
events and not through a widget. So the dialog is shown before anything else in the
frame, as a menu is, and when it has read its keys every key press and everything typed
is taken out of the frame's input. Escape closes it (read first, and taken), and so does
a press on the scrim - the press, as a menu closes. The question opens with the keyboard
on "Cancel", so an Enter that came with the dialog deletes nothing. `Modal`'s area fades
in like any new area, each frame of it asked for: `fade_in(false)`.

**A letter and the Enter after it can arrive in one frame**, when a frame is long: the
field's Enter is read before the field is drawn (egui's field gives the keyboard up at
Enter), which is before the field has taken the letter. The picker's submit carries what
the field holds after the frame's typing (`Done::Submit::typed`), and the sidebar's field
says what was typed before it says the name is to be stored - without it "Porto" and
Enter in one frame renamed an album to what it was already called.

**Three more things about a dialog's keys, each found by asking of the dialogs what a
review had asked of the sidebar's field.** *A key held when a dialog opens presses
nothing in it*: egui presses the button the keyboard is on at every Enter and Space,
repeats among them, so the Enter that picked "Delete…" in a menu, held a moment longer,
pressed "Cancel" on the question it had just asked; the dialog takes repeats of the two
out of the frame before it draws. *A key in the frame of the Escape that closes a dialog
is taken out with it*: the dialog returned at Escape before it had cleared the frame,
and an `h` behind the Escape was the grid's. *The picker's field opens with a history of
its own* (a fresh `TextEditState`): it has one id for every picker, egui keeps a field's
undo by id, and Ctrl+Z in a new picker brought back the keyword typed in the last.

**What a review of the dialogs found, each with its test.** *A delete on its way takes
its row's menu away* (`App::deleting`, `going`): an album's number is handed out again
once it is free, the row stays until the lists are read after the delete's answer, and
"Delete…" asked of it a second time waited behind an album made meanwhile under that
number - and deleted it. Refused, the row is a row again. *A question whose album or
search is no longer listed under the name it was asked under closes* (`App::listed`):
deleted from elsewhere under the question and another made at once, "Delete the album
“Porto”?" answered with yes deleted Paris. *The second press of a double click does
nothing in a dialog* (`DialogView::deaf`): a double click on a menu's item opened the
picker at its first click and landed its second on a keyword row, which writes at a
click - a button that goes down within egui's double-click time of the dialog's first
frame is that press, and nothing is done until it has come up. A test that presses in a
dialog lets that time pass first (`pick_item`, the fixture's `open`). *Enter writes the
keyword the library has under another case* (`TagPicker::submit`): the list's first row
is "Lisbon" for "lisbon" typed, and Enter wrote "lisbon" beside it - where the Svelte
picker still does. *Nothing under a dialog acts*: egui keeps the pointer from what lies
under a modal through the modal's own ground, which it finds under the pointer only from
the frame after the one the dialog is first drawn in (it tests a press against the
widgets of the frame before; the "modal layer" in its memory is about the keyboard). So
in a frame drawn under a dialog the dialog takes a button going down out of the frame's
events, which is where the grid and the sidebar's menus read it, and the application
drops whatever the shell answers - a click that was over within that first frame was
still a click on the sidebar's row. *The cross in the picker's corner shows the keyboard
on it*, as every button does. Not closed, and known: a screen reader's press goes
through a modal (egui's own, and not reachable until AccessKit is switched on); the
picker lays out every keyword in every frame, which is slow in a library of thousands;
and a keyword's removal rule is dropped by adding that keyword again, as the engine has
it.

**A folder dropped on the window is watched** (`folder_drop.rs`, `createFolderDrop`;
`drop_card.rs` for the card). The application takes what the system says is held over
the window and dropped on it out of each frame's input before egui reads it
(`App::raw_input_hook`; `RawInput::hovered_files`, `dropped_files`: the system's own
events, a file each) and hands a drop's paths to the engine as one write
(`Write::Watch`), in order, each offered after the one before has been answered - two
folders of one drop can overlap, and the engine can refuse the second only once it has
finished the first. The engine is what knows a folder from a file and a folder already
watched; the drop is reported once (`Watched::said`, `refusal`: the first refusal with a
count of the rest, so that forty photos dropped are not forty messages). **Whether a drop
can arrive at all is asked of the window, once** (`folder_drop::can_arrive`, from the
window's handle in `App::new`): winit 0.30 has dropped files on X11, Windows and macOS
and none on Wayland, so a Wayland window offers none - no card, no line in the empty
library's panel - and a test, which has no window, feeds the drop itself
(`common::hold_over`, `drop_on`; `App::without_drops` for the window that takes none). A
folder just watched is in the list on hand at once and counts as being scanned until its
scan has been heard from (`Scans::added`, into the launch's own set): the list read
again arrives a moment later, and an empty library must not say meanwhile that no folder
is watched. That call has its tests in `scans.rs` and none through the application: the
answer that says a folder is watched and its scan's first report arrive in either order,
with no seam to hold one behind the other. One folder twice in a drop - by its path, and
by a link to it - is one folder, by its id. **Under the export's dialog the first path
dropped is its destination, and nothing is watched** (the first path, not the first
folder: a photo dropped before a folder is said to be no folder, as a file typed there
is); under anything else that is open a drop is the window's, as ever. The card is drawn
over everything, a dialog too (`Order::Tooltip`), takes no press, and tells AccessKit its
words, which is how a test of the whole application sees it.

**egui is not told what is held over the window.** For as long as it is, it asks for the
next frame at once (`InputState::wants_repaint_after`): a folder held still over the
window drew a frame at every refresh, and the card's own frame test, which draws the
card without the input that puts it up, could not see it - a review counted the frames.
The system's reports each draw a frame without that (`egui-winit` asks for one at each),
which is all the card needs. `egui_kittest` does not call `raw_input_hook`, so the tests'
`Driver` does before every frame (`common::hand_over`) and keeps what is held from frame
to frame, as `egui-winit` does (`Driver::held`). **And the system does not always say
that a drag has left**: winit on X11 can report a file held after it has reported the
drag gone, when the two cross - read in its source, not reproduced - and what it goes on
calling held is then held for good. So a pointer that moves or is pressed in the window
ends the drag, and so does a drop, whatever the system still says
(`FolderDrop::frame`, its `over`); the next drag is one more path than was left over.
That a window hears nothing of the pointer while another program's drag is over it is
how a drag is made on X11 and Windows and is assumed of macOS: were it not so there, the
card would not show and the drop would still be taken.

**The export is a third dialog and a thread of its own** (`export_dialog.rs`,
`createExportDialog`; `export_view.rs` for its body; `App::exports`). Its photos are
taken as it opens, as the picker's are. **The destination is a path typed into a field**
until a later sub-project brings the folder picker, whose button stands beside the field
and takes no press: asked about as it changes, off the UI thread, by a task that answers
only the latest question (`App::checking`, `check_dest`), and the answer counts only for
the text the field still holds (`ExportDialog::checked`). It passes when it is the whole
path of a folder that is there (`typed_path`: a `~` at its start is the home folder, and
a path that is not absolute is refused - it would be read from wherever photon was
started) and the engine will write into it; a file passes the engine's own check, which
asks only where a path lies, so the task asks `is_dir` first. **The text as it stands is
asked first, and without the spaces at its ends only when no folder is at that very
path**: a folder's name can end in a space, and a dropped folder's path is put in the
field as the system gave it - read trimmed, "out " dropped passed as the folder "out"
beside it, and the copies were written into a folder nobody had named. What is said of a path
that does not pass stands under the field, the engine's words for a folder inside the
library among it, **and stands there until the next answer** (`ExportDialog::problem`):
a path is asked about at every letter, and with the line taken away at each letter and
put back at each answer it came and went as a path was typed, the dialog in the middle
of the window moving up and down with it (seen in a picture whose click missed a button
that had just moved, and in no test until then). The room of one line is kept under the
field whether or not anything is said there, so the reasons of one line move nothing;
the engine's two lines make the dialog taller. **And every dialog is placed by the
height it has in the frame it is drawn in** (`dialog_view.rs`: an `Area` of its own
with `Modal`'s settings, since `Modal`'s is anchored, and an anchored area is placed by
the size it had the frame before): in the frame the export's dialog changed height its
top stayed where it had been, a press on "Export" where the button had stood was a
press beside the dialog - which closes it - and it was put in the middle a frame later.
"Apply edits" is read when the dialog opens (ticked until then, and
not over a tick made meanwhile), stored on the queue of writes as it is ticked, and put
back if the library will not store it. The size is five buttons in a row where the
Svelte dialog has a dropdown, and is never remembered. Enter in the field exports once a
destination has passed; what was typed in the frame of that Enter is asked about first.
**The dialog closes before the export runs, and the export is not on the queue of
writes**: it takes minutes, and a star pressed meanwhile must not wait for it. One at a
time (`App::exporting`): "Export…" takes no press while one runs, and the status bar's
line is the engine's own reports - the first when a photo is done, so nothing but the
dimmed item says that an export stands at its first photo. The window does not wait for an export when it closes,
as the Tauri photon does not. Nothing here writes a file itself: the copies are the
engine's (`Engine::export_items`, which refuses a destination inside a watched folder
and creates every file with `create_new`), and a test's destination is a folder under
its own temporary directory. A test holds an export for as long as it needs by holding
the engine's `RENDERING` lock, which a turned photo's copy waits for.

**The viewer takes the whole window** (spec `2026-10-12-photon-native-viewer-design.md`;
`viewer.rs` for which photo is open, `viewer_view.rs` for what is drawn, `App::viewer`
while it is open). The shell is not drawn under it, so the grid's keys cannot be answered
twice and the grid keeps its place, which is a number of its own. Enter on the photo the
keys are on and a double click open it - **a double click is read from two plain clicks
on one photo** (`GridView::clicked`), not from what egui calls one, which is of a widget,
and the photos are one widget. Escape, Backspace, the mouse's back button and the cross
close it, and the photo it was on is handed back selected and brought into sight; one the
view has lost hands back none, and the grid is brought to where it stood. **Enter opens
nothing in a frame with a mouse button in it, nor while a press on the photos is held**:
opened over a press, the button came up where the grid did not hear it and the first move
of the mouse after the viewer drew a band with no button down; opened in the frame of the
other button, it stood under the menu that button had asked for. **Other results that do
not hold the viewer's photo close it** (`adopt`: a search typed, and a photo opened before
it was sent): its offset is a number of another list, and the step back from the ninth of
all photos went to the third of three. Where they hold it too, it stays, at its place in
them. And a search typed is sent on time under the viewer, which asks for its frame as
the shell does. **Most of the Svelte viewer's guards have no successor**:
this window holds the whole `GridIndex`, so the photo is found again by its id in the
frame a new grid arrives in (`Viewer::follow`), and a picture is asked for by the photo
and its key (`Want`), never by an offset - `createLoadSlot`, `rebind`, `renumbered` and
`detailsSeq` are not ported. A photo the new grid lacks has left the view or the library,
which only the engine can say: `viewer_item` is asked again, an answer makes it
`orphaned` (it stays on screen, with no place in its caption, and the next step goes on
from where it stood) and the refusal of a photo that is not there (`notFound`) `gone`;
any other refusal leaves what is on screen, which nothing tests - there is no way to
make the library fail a read on demand. `picture_changed` is ported whole, with the
faces left out of what it compares. **What the library says of the photo is a round trip
behind the grid**, and is what `.` and `H` go by: a test that presses one waits for the
facts first, as a user's eye does.

**No control of the viewer is a stop of the Tab key** (`Sense::CLICK`, not
`Sense::click()`): a control that had the keyboard would take the arrows from the viewer,
egui walking the focus on with them, and every control has a key. **And Tab is called
off in the viewer** (`move_focus`, as under a menu; not under a dialog, whose own walk it
is then - below, with the faces): a message drawn over it has a button
that is a stop, and with the keyboard on that the arrows stepped nothing and the grid's
keys were dead once the viewer had closed. The wheel steps a
photo a notch whatever the system's lines a notch are - nothing scrolls here - and a
trackpad's stream adds up to one (`viewer::wheel_step`); what it has turned is the
viewer's it was turned in (a view made anew at every opening).

**The picture is the preview, then the full picture, and photon scales the full picture
itself.** The preview is the cache's 1600-pixel thumbnail (`Thumbs::previews`, a second
store over `EngineThumbs::preview`). The full picture is `commands::viewer_picture`, a
command the Tauri photon does not call: `render_picture` behind `RENDERING`, as pixels.
It is asked for on a `Latest` - one at a time, the last one wins, since a decode cannot
be taken back - and **after a rest of 130 ms on a photo stepped to** (`Viewer::rested`):
a held arrow key passes photos faster than they decode. **egui's wgpu renderer gives a
texture one level** (`mip_level_count: 1`, whatever `TextureOptions::mipmap_mode` says),
so a camera's photo fitted into a window is sampled from a few of its pixels in every
ten. Two things answer that, and the second was added when a picture showed the first
was not enough: the picture's halves (`viewer_levels.rs`, each the mean of four pixels,
down to 512 pixels - at 2,048 an upright photo in a low window was drawn from a level
three times its size; the level drawn is the smallest at least as large as what is
drawn), which are what a zoom draws from; and **a copy scaled to the window with a
Lanczos filter** (`edit::fitted_copy`, made by the engine for the box the viewer names,
drawn pixel for pixel on whole pixels while the photo is fitted at that size). On a zone
plate - `native-viewer-quality.png` beside its reference, which `native-shot` writes -
the halves alone drew rings the photo does not have; with the copy the two pictures are
the same. A window of another size is another copy, asked for once the size has stood
for 200 ms, and the levels are drawn meanwhile. A level larger than the device's limit
is not made into a texture (`max_texture_side`; **`egui_kittest` says nothing of it and
egui then takes 2,048**, so a test that wants a large level says
`harness.input_mut().max_texture_side`). The picture is handed over as `ColorImage`s
made on the worker: the frame that uploads a 10 MP photo and its half took 8 ms in an
unoptimised build, so it is one frame's work and is not spread over several (that is
egui's pass and the write of the textures, off screen; what the GPU does with them
after is not in the number, and has not been measured in a window). **A full
picture that cannot be made leaves the preview**, with nothing said: a photo on a drive
that is not plugged in is looked at from photon's own cache, as the web view left it;
why is said only where there is no preview either. A picture made for a photo the viewer
has left is dropped as it arrives. A test holds a render for as long as it needs by
holding `RENDERING`.

**Whether the full picture is asked for is decided from what is held and what is on its
way, never from what was once asked** (`show_viewer`: `none`, `other_size`). "Asked for
once" left a photo at its preview for good in three ways a review found: one photo on
and straight back inside the rest (the picture is let go of at the step), a photo that
was gone and came back, and a picture the engine made from another state of the photo
than the one named (`Made::Stale`) when the photo is back under the key it had - turned
and turned back between two frames, the window never hears it was another picture, and
no grid asks again. So a stale answer has the library read again, and the picture is
asked for after that answer, under the key the engine has by then. What stops the
asking, each with its test: a photo that is gone (asked for then, the refusal would be
what is remembered when it is back), one the library says cannot be shown (its picture
is asked for at the opening, before the library has said anything, and is not held when
it arrives), and a picture that could not be made - until the photo is come back to. The
question on its way is kept beside the task and not with the viewer (`App::asking`),
which can close and open again on the same photo meanwhile. **A viewer closed holds no
texture**: its previews are let go of with it (`Thumbs::clear`), which nothing else did
but their store's own limit of ninety-six megabytes.

**The zoom and the pan are the viewer's state, and the arithmetic is `nav.ts`'s**
(`viewer_zoom.rs`: `zoom_at`, `clamp_pan`, `actual_size_zoom`, `wheel_zoom_factor`, with
that file's tests' cases; `Viewer::zoom`). A point `q` of the fitted photo, measured from
the middle of the photo's area, is drawn at `pan + zoom * q`: everything is measured from
that middle, not the window's, which is another place once the info panel stands beside
the photo. The fit again at a step and at another picture of the same photo
(`Viewer::land`, `took`), in the state module, so that nothing in the application has to
remember to; kept at a star, a new grid, a photo leaving the view. The view is handed
the zoom to change (`ViewerView::show(.., &mut Zoom)`, a copy the application writes
back before it acts on the frame's steps). **What zooms**: `+` `=` `-` `0` about the
middle; the wheel with Ctrl or Cmd, a fifth a notch, at the pointer - read from the
wheel's own event, a line taken for a hundred of a web view's pixels, because egui's
`zoom_delta` is the same turn scaled by its scroll speed, which is the grid's and
another on every system; a pinch where the system reports one (`Event::Zoom`, a Mac's
trackpad); the slider, which leaves the pan where it may stay; a double click, to the
photo's own pixels and back. **"Its own pixels" are what the library says the photo
has**, turned as it is seen, whether or not the full picture has landed - a double click
a moment after a step goes where one a second later does (the first version went by
the largest level on hand, and to twice until there was one); until the library has
answered, the largest level. At that zoom the level stands on whole pixels (`picture`'s
`own`): half a pixel off them is the linear filter's blur over the whole photo, at the
one zoom a photo is checked for sharpness at. The fitted copy is drawn at fit and at no
zoom, which is said in so many words (`zoom.is_fit()`): a zoom can be the copy's size
too, a hair over the fit or twice in a window half as wide as the copy was made for.
**The fit is asked as a question, and a float's error over it is the fit**
(`clamp_zoom`, `FIT_WITHIN`): a fifth out and a fifth back a few times over ends at
1.0000001 in single precision, where `nav.ts`'s doubles end at one, and everything that
asks "is it zoomed" then said yes - a hand over a photo that cannot move, and a double
click "back to the whole photo" with nothing seen to happen. **A press on the photo is
read from the button's own events**, as the grid's band is (`Press`): a click when it
comes up within a click's distance of where it went down and never went further; a pan
while it is held, at once and not after a threshold, **if the photo was zoomed when the
button went down** - a press on a fitted photo is no drag and does not become one when
the photo is zoomed under it, or the photo is thrown by all the pointer moved while
there was nothing to move; and a double click from two such clicks in egui's time and
distance. Its endings are the band's: the button up, the window left
(`WindowFocused(false)`), a touch cancelled; the pan stays where the drag had brought
it, and the slider's drag ends with it (`let_go`: it goes through egui's press, which
knows no such ending). Where nothing zooms any more - a video stepped to under a held
button - the press and the first click of a double click are over. A zoom made during
a drag moves the drag's origin along (`Zoom::to` answers how far the pan moved), or the
pointer's next move puts the pan back. **The photo has a surface widget under the
controls** (`viewer-photo`): with something there to be hit egui hands no near miss to
a button, a press is the photo's when it is on no control's rectangle - the photo's own
caption among them - and under nothing that lies over the viewer (`layer_id_at`: a
message's button), and one place decides. No zoom for a video, nor where something is
said in the picture's place; the slider is not drawn for a video, and answers the first
button alone. The hand is closed while the photo is held, wherever the pointer goes
with it. **A zoom's key pressed after an arrow in one frame is lost**: the view changes
the zoom of the photo the frame began on, and the step, acted on after the frame, puts
the next one back to the fit - a frame long enough to hold both costs a second press.
**Ctrl with `+`, `-` and `0` scales nothing** (`zoom_with_keyboard` off): egui would
zoom the whole window by them, as a web view zooms its page, which the Tauri photon has
switched off.

**The info panel is the viewer's while it is open, and the photo is fitted beside it**
(`viewer_info.rs` and `viewer_histogram.rs` for what it says - `exif.ts` and `histogram.ts`,
with their tests' cases; `viewer_layout.rs` for where it stands - `info_room`, `photo_area`
and `info_place`, from one set of numbers; `viewer_info_view.rs` draws it and answers what
was pressed in it). Closed at every opening, kept across a step. The photo's area is the
window less the panel's room, and everything that was measured in the window is measured in
it: the fit, the copy the engine is asked to scale (`show_viewer`'s `within`), the zoom's
bounds and the point it holds still, the pan, a message in the picture's place, the button
for the next photo. The picture is cut off at the area's edge, so a zoomed photo stops at
the panel. The photo's own caption is not drawn beside the open panel, which has it whole.
**Nothing is shown before the library has said it**: a keyword taken off is gone when the
library says so, a round trip later, where the Svelte panel shows its own guess at once and
puts it back on a refusal. Here a refusal is a message - and the keyword that was typed is
not put back into the field, as the Svelte panel puts it. A keyword is `Write::TagAdd` or
`TagRemove` on the queue of writes, for which the engine rebuilds the grid, and a grid's
arrival is what has the viewer read its photo again; one the photo has already, in whatever
case, is not asked for (the engine would rebuild the grid for nothing). An album's tick is
the menu's `AlbumAdd` or `AlbumRemove` with the one photo, and **the write's own answer has
the photo read again** with the lists: the engine announces nothing for an album that is not
on screen. **What the panel writes is written before the frame's steps are acted on, to the
photo the panel was drawn for** (`show_viewer`'s first pass over what the frame did):
written in their turn and by the viewer's photo, a tick in the frame of an arrow key - one
that is held repeats thirty times a second - went to the photo stepped to, whose panel
nobody had seen, and a tick in the frame of the Escape that closes was not written at all.
It is the grid's rule for a key that writes (`Asked`), met again. What the People section
says of a photo with no face is the engine's switch (`Engine::face_detection`, a flag in
memory) and whether a pass is running (`Scans::looking_for_faces`).

**A link of the panel that leaves the viewer shuts it first**, and goes where a row of the
sidebar would, through `App::go`, which tells the search box: a camera, a lens and "Photos
nearby" are searches, a person is their view. The photo is not handed back to the grid where
the grid is being left too; **where the link leads to where the user already is** - a
camera's link in that camera's own search, a person's in their view: `go` asks for no step
there - the viewer closes as it does by its cross, onto the photo it was on. **A copy is
found in All photos** (`App::locate`): by the folder jump's own means, `Nav::jump` with a
`Target::Photo` - looked up in the grid on screen when no step is on its way, else in the
grid of the step to All photos - and there selected, alone, and brought into sight
(`take_photo`'s `locating`, by its id in the grid the next frame draws). All photos is asked
for whatever is on screen: `go` asks for no step to where the user already is, and drops a
search half typed, which would land after the jump. From Hidden too, where a folder asked
for is looked up in the grid on screen: a copy the panel lists is never a hidden photo, and
a photo unhidden in the viewer stays on screen in Hidden with its copies listed - looked up
there, the press closed the viewer and found nothing. "Open in OpenStreetMap" and "Show N
duplicates in the grid" are in their places and take no press; no text of the panel can be
selected. Known and left: a double click on a copy's link lands its second click on the grid
that is by then under the pointer, and selects that photo in the copy's place.

**The panel is one column painted by hand** (`Page`: where the next thing goes; `Flow` for
what stands side by side and goes on to the next line), scrolled by a number of its own,
through a child cut off at the panel's edge - an icon paints through the `Ui` it is given
and a widget is pressed only inside its clip. The cut is what keeps a thing that is scrolled
out from being drawn or pressed; **`Page::sees` is what keeps it from AccessKit**, by not
handing it to egui at all - without it a line that was not on screen was there for a test of
the whole application to find, and will be for a screen reader. (A probe that took one such
test away passed, because the row it looked at was guarded twice: the test now asks for a
name, a row, a link, a heading, a keyword with its cross and an album's tick. Not for a
Picasa album's row, nor for the words between two links.) **But the keyword's field is drawn
in sight or out of it**: egui lets go of a field that is not drawn, and scrolled out of
sight it gave the keyboard to the viewer, where the next letter typed is a key. It then has
the keyboard with no caret to be seen, and nothing scrolls it back when it is typed into.
**The panel's ground is a widget that does nothing, and is needed**: where a press hits
nothing egui hands it to a widget within five points, so a press in the panel's own margin,
just above a link the edge cuts off, pressed the link. In the viewer the photo's surface
lies under the panel and would do as much, which is why a probe that took the ground away
passed until a test pressed there in the panel alone. The wheel over the panel is the
panel's and steps to no photo: **forty points a notch, whatever the system's**
(`PANEL_NOTCH`), and a trackpad's points as they come. Not egui's `line_scroll_speed`, which
the application sets to the system's notch in every frame for the grid: at a Windows set to
scroll no lines a notch the panel, which has no other way to be scrolled, could not be
brought to its foot. A press on the panel is not the photo's (`Controls::panel`). A panel
that has got shorter than it is scrolled - a keyword taken off - is held to its new end in
the next frame, which it asks for: what it holds is measured as it is drawn. Another photo
begins at the top, with nothing typed. **Its words break at a word's end**, and inside a
word only where the word is longer than its line, which egui does by itself: told to break
anywhere it never looks for a word's end, and a camera was "Canon EOS 5D Mark IV Special E"
over "dition". They are laid out by egui and do not go through `text::paint_line`: a name in
another script is drawn in the order it is written - turned round, in the same window in
which the bar has it right.

**The keyword's field is egui's text field a third time**, with the filter spelled out
(above, "egui's text field, twice now") and a history of its own for each photo - one field
under one name serves them all, and Ctrl+Z in the next photo's field brought back the
keyword typed for the last. Enter is read and taken out of the frame before the field is
drawn - egui's field gives the keyboard up at an Enter it sees, and the next keyword is
typed straight on - and adds what the field holds once it has taken the frame's letters.
**Whether the keyboard is the field's is the panel's own word** (`InfoView::held`,
`typing`), not egui's: egui lets go of a field in any frame that does not draw it, and at a
step there are such frames, as many as the library takes to say what the next photo is. By
egui's word the field kept the keyboard across a step of the wheel when the answer was quick
and lost it when it was not, and "hills" typed next was a keyword or five of the viewer's
keys - `h` hides the photo - by a matter of milliseconds. It keeps it now, for the next
photo's keyword, and asks egui for it back when it is drawn again; letters typed in the
frames between are dropped. **While it has the keyboard the viewer's keys are the field's**:
the viewer reads its keys from the frame's events and not through a widget. Escape leaves
the field and closes nothing, by that rule and not by being taken out of the frame, which
was written first and did nothing: the viewer has read the frame's keys before the panel is
drawn. The next Escape closes the viewer. A press of any button anywhere but on the field
leaves it at the press - egui gives the keyboard up at a click elsewhere, which a press that
becomes a pan is not - and one on the field itself does not, button down or up. Known and
left: a letter in the very frame of the click that gives the field the keyboard is the
viewer's too (the question is asked before the panel is drawn), and a keyword the library
has on another photo under another case is written as it is typed, where the picker writes
the library's. **The field goes with the viewer, in the first frame that finds the viewer
shut** (`App::ui`, before the shell is drawn): egui lets go of a field that is no longer
drawn only at the end of the frame that did not draw it, so that frame's keys were not the
grid's. In one place, because the viewer is shut in two - by its own frame, and by `adopt`,
when a grid of other results does not hold its photo. Each has a test with the key in that
very frame - for the first with the mouse's back button still held, since the click of a
button let go of has egui take the keyboard off the field by itself; with the key a frame
later, as the first version had it, the test passed with the rule gone.

**egui paints a field's placeholder in the theme's "weak" text colour, whatever colour its
words are given** (`hint_text` with a `RichText` of a colour: the colour is replaced). Two
things follow. `theme::install` sets that colour to the palette's dim text, which every
field here had asked for in vain and got egui's greyed-out text instead. And **the viewer's
keyword field is drawn in the dark theme's visuals, whatever the window's** (`set_visuals`
on its own `Ui`): the viewer's glass is dark in both themes, and in a light window "Add a
keyword" was all but invisible - seen in a picture, enlarged, after every test had passed -
and the caret was that theme's dark blue on the dark glass. A placeholder's colour and a
caret's are things a frame test can ask (the fixture's `tints` and `lines`).

**The bar keeps its width while the caption is not known** (`bar_place`'s `was`): at a step
the caption is empty for the round trip the library's answer takes, the bar shrank to its
tools, and its buttons jumped twice at every step - found by a test that pressed the panel's
button a frame after a step and missed it. A test that presses in the bar after a step waits
for the photo's facts. **A fixture's photos have no camera and no copy**, and no call of the
library's gives them one: a test writes the columns itself before the launch (`doctored`,
`copies` in `tests/viewer.rs` - the engine's own pass does not read a drive that is not
there, so the hash stays). And a library where photon looks for faces is still recognising
them for a moment after the launch: a test of the People section's "No faces found." waits
for the pass's line to go.

**The viewer's full picture is converted to sRGB, and nothing else is**
(`photon_core::colour`; `commands::viewer_picture` calls `in_srgb`, and nothing else does).
A decoder hands over numbers, and what colours they are is said beside them: a phone's photo
is Display P3 and a camera's often Adobe RGB, and their numbers read as sRGB are duller
colours than were taken. A web view converts them; the native viewer draws the pixels
itself. The conversion is moxcms: pure Rust, and in the build already as `image`'s own
dependency. `embedded_profile` reads what the file says - the ICC profile of a JPEG, a PNG,
a TIFF or a WebP, through each format's own decoder, and an AVIF's `colr` box, which is a
profile or four numbers (`nclx`). It is a read of the file of its own, after the decode:
every full decode goes through `decode_image`, and its other callers want no profile. **A
PNG's profile is read under a limit** (`PROFILE_LIMIT`): it is deflated, `PngDecoder::new`
inflates it with no bound, and a file of four megabytes took 1.3 GB to say it had no
profile. **Only a profile that says what a picture's numbers are is applied**: RGB, and of a
device or a colour space - not a link between devices, an abstract profile or named colours;
not a CMYK one, whose numbers the decoder has made RGB of by a rule of its own; and not a
grey one, which a web view does apply (the spec's "RGB profiles only": a grey photo with a
profile of its own keeps its numbers, 128 where a browser shows 146). **A profile's curves
are its curve tags, whatever its `cicp` tag says** (`allow_use_cicp_transfer` off, where
moxcms's default is on): by the tag, a profile with sRGB's curves and a tag saying PQ turned
mid grey to near black, and one saying BT.709 was read by BT.709's own formula. **An AVIF's
numbers** are converted where they name a standard-range space: the primaries by moxcms's
table, BT.709's where none are named (each of the two has a default of its own), and the
curve taken for sRGB's own where it is BT.709's family or not said at all, as a web view
takes it - by its own formula a photo that says "BT.709" comes out lighter than every
browser shows it, 128 as 140. PQ and HLG are left as they are: high dynamic range is not
shown on this surface by a conversion of numbers. **A profile that is sRGB is told by what
it does**, not by its name: every step of grey and of each channel alone is converted first,
and where none moves by more than a step the picture is left alone - which is nearly every
profile there is. (Six steps of each were tried at first: a curve that is a table, sRGB's at
those six and lifted between, passed for sRGB with its greys seventeen steps out.) The
picture is converted in place, a chunk at a time, so no second picture of 96 MB is held (30
ms for 24 million pixels in a release build, 27 in one pass over two pictures; about three
seconds unoptimised, which only a debug build of the application pays - moxcms is not among
the crates the dev profile optimises), and **before the fitted copy is scaled**, so that the
two pictures of one photo are of one colour.

What is *not* converted, each on purpose or for want of a decision: the preview (so a
wide-gamut photo changes colour a little when its full picture lands, as it does in the
Tauri viewer), the grid's thumbnails, an export and the copy for the clipboard - whether
those should be is a decision about photon-core and the Tauri photon, not this viewer's. (It
is not a small one: a re-encoded export of a Display P3 photo carries Display P3's numbers
and no profile, so every other program shows it duller. That is older than the native
viewer.) And "converted" means converted to sRGB's numbers: whether the system takes the
window's surface for sRGB on a wide-gamut display is not something a test here can see (the
smoke checklist, for a Mac). **Known, and left**: an AVIF with no `colr` box whose AV1
stream names its colours is not converted (a browser reads the stream's own header; photon's
decoder reads it for the matrix and the range only, and writers put the box in); an AVIF's
profile is passed over when a box of numbers stands before it (the parser hands over the
first of the two; libavif writes the profile first); a PNG's `gAMA`, `cHRM` and `cICP`
chunks are not read, so one that says its colours that way and carries no profile is taken
for sRGB; and a picture of sixteen bits is made eight before it is converted, which leaves a
wide-gamut ramp a few shadow levels short.

Things about moxcms, `image` and the tests, each learnt by a test that failed or a review
that measured. moxcms's `update_rgb_colorimetry_from_cicp` answers false whatever it did
(0.8.1), so the profile is made by `new_from_cicp` and asked whether it has primaries and
curves (`is_matrix_shaper`). **A profile moxcms writes carries a `cicp` tag**, so a test
built on one alone never takes the road a phone's profile takes; the tests of the tag write
their profiles a byte at a time (`colour::by_hand`, a reviewer's writer). A profile moxcms
writes for any class but a screen's or a camera's cannot be read back (it counts a tag it
then does not write), so the tests of "a device link is not applied" patch the four letters
of a good profile's header - made by the writer, such a fixture was refused for being broken
and proved nothing. `image`'s generic reader loses the profile of a *small* TIFF (it has the
tiff crate take no tag with more values than the picture has bytes), which this module's
16-pixel fixtures are and no photo is. And the values the tests expect are worked out by
hand - primaries to XYZ and back through sRGB's, each space's own curve - not asked of
moxcms; they agree with it, and with lcms2, to the digit. A profile that makes moxcms panic
costs that photo its full picture and no more: the task catches it and the preview stays -
none was found in half a million mutated profiles.

**The faces of the photo in the viewer are outlined in the picture as it is drawn**
(`viewer_faces.rs`, which is `faces.ts` with its tests' cases; drawn by `viewer_view.rs`'s
`faces`). While the info panel is open each face has its outline at the rectangle `picture`
answers, zoom and pan in it, cut off at the photo's area, a named one with its name on a
plate under its left corner; with no picture drawn there is no outline, and no face under a
press. The line, its ink and the plate's ground are the photo's own colours in both themes
(`PHOTO_LINE`, `SHADOW_INK`, `SCRIM`). **An unnamed face photon found is a button**
(`viewer-face`, "Name this face"), as much of it as is in the photo's area, and to the
photo's own press a control (`Controls::hold`): no pan begins on it, and it is no half of a
double click. A named face's outline, and one that is Picasa's alone, take no press - a
press there is the photo's. One that a zoom has taken out of the area is not handed to egui
at all, nor is its plate, so it is not there for whoever reads the window without seeing it.

**Which outline a click is on is the view's to say, not egui's** (`faces`' `which`). egui
gives a click to the last widget made under the pointer that takes one, whatever lies over
it that takes none: the bar's glass is no widget, its caption senses no click, and a button
that leads nowhere - "Next photo" on the last photo, where a hand that has clicked through
a folder clicks once more - senses none either. Each of those named the face under it. And
of two outlines one inside the other egui's click is the later one's, not the smaller's,
where the menu is the smaller one's (`face_at`). So a click names a face only on no control
(`Controls::over`), and the smallest outline that holds it. An outline's widget is named by
its number among the faces and not by its id: two outlines Picasa drew over one detection
carry one id, and under one name both were one widget at the first one's place. **And a
press is asked of the faces as the last frame drew them** (`ViewerView::faces`, `Aimed`),
not of where this frame is about to: in the frame after the panel is toggled, the window
resized or another picture landed the faces are elsewhere, and a press on one face was
taken for another's.

**The other button asks for the viewer's menu where it goes down** (`ViewerAction::Menu`;
`viewer_menu.rs` for its items, in `Viewer.svelte`'s words and order), read from the
button's own event as on a photo of the grid: on the photo's surface and its faces, not on
a control, not under anything that lies over the viewer, and not while the first button is
down - on the photo, or on the slider, which would go on following the pointer under the
menu. (Not once the window has been left with that button down, `let_go`: its release may
never be heard of, egui goes on calling it down, and no menu could be asked for until the
first button was pressed again.) Which face it went down on is asked of every face that is
in the photo's area, whether or not the panel shows the outlines, and **kept by what the
face is, not by its number in the list** (`viewer_faces::On`): a menu's items are built
again in every frame from the photo as the library last said it is, and the faces can be
another list by then. A face that is no longer what the menu was opened on - named since,
or gone - offers nothing: not the photo's own "Not …" items in its place, which were not
what was asked for. **The menu is of the photo on screen, as the tile's is of the
selection** (`MenuOf::Viewer`): its picks act on the photo its items were just built from,
and with nothing said of the photo - one that is gone, a viewer that is shut - it has no
items and closes. "Not Anna" is `Write::PersonRemove` with the one photo, the grid's
Remove; "Locate in photon" shuts the viewer and finds the photo in All photos, or **in
Hidden when the library says it is hidden** (`App::locate`'s `hidden`), the one view that
holds it then; "Reveal in file manager" and "Open in default app" are in their places and
take no press. The menu is dark in both themes, as the viewer is (`MenuView::show_in`) -
seen in a picture, where it was light in a light window; the frame test holds `show_in`,
and nothing holds that the application calls it so but the picture.

**A menu or a dialog asked for in a frame that stepped is not opened** (`show_viewer`'s
end: only while the library has said what the photo on screen is, which after a step it
has not). A held arrow key repeats thirty times a second and a wheel is turned while a
button goes down. Opened, the menu stood over the next photo and - by a race between the
library's answer and the next frame - closed or became that photo's; and "Who is this?"
stood over the next photo asking about a face of the one that was left, with a face of the
next one's own in the same place: the name typed was written to a photo nobody was looking
at. It is the rule the panel's writes have ("to the photo the panel was drawn for"), met a
third time. The same test keeps both from opening over a viewer the frame has closed.

**The viewer is deaf under a dialog and under its own menu** (`ViewerData::paused`). It
reads the wheel and the buttons from the frame's events, and a dialog takes only the keys,
what is typed and a button going down out of the frame, a menu only the keys: under either
the wheel steps to no photo, a held press is over, and what the wheel had turned is not
kept for after. A press beside the menu closes it before the viewer is drawn, and is then
the viewer's, as on the grid. What the viewer answers in a frame drawn under a dialog is
dropped, as the shell's is, and by the same word (`under_dialog`, read before the dialog
is shown, or a dialog opened since): **the frame a dialog opens in and the frame it closes
in are the dialog's** - a notch of the wheel in the frame the menu's "Name this face…" is
picked would step under the dialog it opens. Each of the two - deaf, and dropped - has its
own test, and needed one: the first test of "nothing reaches the viewer under its dialog"
passed with either taken out, each hiding that the other was gone - the wheel's step is
dropped with the answers, a click on the star in the dialog's first frame is not read by a
viewer that is deaf. What tells them apart is the wheel with Ctrl, which zooms and answers
nothing, and that click, which egui hands to the star whatever the viewer reads. **And
egui's walk of the keyboard is not called off in the viewer under a dialog**: the viewer
calls it off for itself after the dialog is drawn. Tab egui has settled by then, among the
dialog's own widgets; what was called off was an arrow's walk from one of the dialog's
buttons to the next - a person's row, reached by Tab - which egui makes at the end of the
frame. (The comment first said it was Shift+Tab wrapping round the dialog's end, and a
probe passed: the test of that passed with the rule gone.)

**The person dialog is the fourth dialog, and it is the keyword picker's drawing**
(`person_picker.rs`, `createPersonPicker` and the lines of `people.ts` it says;
`dialog_view.rs`'s `Picker`: a title, a field with its button, a list narrowed by what is
typed - and for this one a line under the field that says what Enter will do). What it
names is taken as it opens: a face by its id - the outline pressed, or the menu's "Name
this face…" - or the photos selected, for the grid's "Add N photos to a person…", which
works now. The people it lists are `named_people`, read by a task as it opens (`App::named`,
forgotten at each opening): **until they are read none is listed, nothing is said in the
list's place, and the line under the field says what it says at rest** - "No one is named
yet." would be said, for the moment the read takes, of a library with fifty people in it,
and by an empty list every name typed is "a new person". A read that fails leaves them not
known, for the same reason; a name can be typed all the same. **A dialog that is a field
over a list stands where it would at its tallest and grows downwards** (`Picker::tallest`):
it is as tall as its list, which is another list at every letter and - the people's - is
not there at all until a moment after the dialog opens, and in the middle of the window by
its own height its field moved a hundred points under a name being typed. That is the
export dialog's lesson (above) met in the keyword picker, which had moved as its list
narrowed since it was built. A name is written as it is typed, on the queue of writes
(`Write::Name`): the library is what knows whose it is, whatever its case. **What is said
afterwards is the library's**: "This is Anna." by the name the person has, which the
write's worker reads after the write (where the Svelte picker goes by the list it read as
it opened); "That face is no longer there." when `name_faces` answers no one, and no
person is made of the name; and for photos `named_items_message`, `people.ts`'s word for
word. Nothing here tells the viewer or the lists: the rebuild the engine makes after a
write of the people is what has the photo read again, and a name written reloads no
picture. **The dialog goes with the viewer** (`shut_viewer`): shut from under it by a grid
of other results that does not hold the photo, the viewer left "Who is this?" standing
over the grid. And **an outline's click a moment after a dialog closed or a menu's item
was picked asks nothing** (`App::overlay_closed`, egui's double-click time): a double
click on a person's row names at its first click, and its second landed on whatever the
dialog had covered - another face's outline - and asked about that; a double click on
"Hide photo" over an outline did the same. (`DialogView::deaf` is the same double click
the other way round, and the Svelte viewer's `menuClosedAt` the same rule for its zoom.)

Three things about the tests. A dialog's title and a face's plate tell AccessKit their
words, which is how a test of the whole application reads them - so a test that presses a
person's link in the panel asks for the *link* of that name
(`get_by_role_and_label(Role::Link, ..)`): the plate over the photo says it too. The
faces of a test's library are written through the library's own calls before the launch
(`looked` in `tests/faces.rs`: `write_face_batch` for every photo, so the launch's pass
finds none to detect); a picture's are written after it, through a second connection
(`shot_over`), because the pictures' library is one the application has scanned itself -
rectangles placed for the picture, as `mock.js`'s are, one of them on the skyline. And
**a read can be made to fail**, which the plan had said it could not: a second connection
renames the table out from under it (`tests/faces_edges.rs`, which holds what a review
found). Not tested: a Mac's Control with the first button on an outline, which asks for
the menu and whose click is the outline's as well - the view answers a menu asked for in
the frame of an outline's click alone, and the click of a later frame finds the viewer
deaf under the menu, but that the two cover a Mac's press has not been run on one; and that
the people's list is forgotten at each opening (no seam holds the read back).

**A turn and a crop are the engine's edit, written from the viewer and never drawn by it**
(spec "Turns and the crop"; `crop.rs`, `crop_tool.rs` and `crop_view.rs` - `crop.ts`,
`createCropTool` and the Svelte viewer's crop markup). `R` and Shift+R are
`commands::rotate_item` and "Original" is `set_item_edit` with no turns and no crop,
`Write::Turn` and `Write::Edit` on the queue of writes; the engine rebuilds the grid, whose
key for the photo is another, `Viewer::follow` takes it, and the picture is asked for under
it at once. **A round trip later `took` hears the same from the library, and asks again
only for a picture changed under the key it had** (turned and turned back between two
frames), compared with the `Shown` it replaces and not with the viewer's key, which
`follow` has already moved: compared with that, every edit made its picture twice
(`a_turn_in_the_viewer_has_its_picture_made_once`, in `tests/edits_edges.rs` with the rest
of what the review found).
No photo is written: `tests/edits.rs` reads the watched folder back byte for byte after
every write it makes. **A turn is of the photo on screen**, read in the frame's action loop
like the star - so an arrow and `R` in one frame turn nothing (nothing is known of the photo
stepped to), and `R` then an arrow turns the photo that was seen. Each key at its press, not
its repeat: a held `R` would be a write and a render thirty times a second. **Whether the
photo can be edited** is the Svelte viewer's `editable`: the library has said what it is
(nothing is known of a photo that is gone), it is not a video, orphaned, unshowable, or a
photo whose full picture could not be made.
The three tools stay in their places, fainter and deaf, where it cannot; a video's bar has no
tools for it at all, as in the Svelte bar. "Original" is there only while the photo has an
edit, between the crop's tool and the info panel's, and widens the bar - and stays while
nothing is known of a photo stepped to (`ViewerView::edited`), or the buttons after it
jumped at every step through edited photos, the jump `bar_place`'s `was` ends for the bar.

**The crop tool is the viewer's while it is open** (`Viewing::crop`, a `Cropping`), drawn by
`ViewerView::cropping` in place of everything but the button that closes: no buttons at the
sides, no slider, no caption, no faces, no panel. It opens on the whole window with the info
panel shut and the zoom put back, on the crop the photo has, free. **It closes when its
photo can no longer be edited or is another picture**: `if !editable` in `show_viewer` -
gone, left the view, stepped from, picture refused, which covers a step made in the frame
the tool opened in - or the grid's key for the photo is no longer the one it opened under
(`Cropping::key`: turned or rewritten from elsewhere, its own "Apply" landing), which is a
round trip before the library says so, and in the frames between it asked for a picture of
the new state that nobody would draw; and when the library says the picture changed under
the key it had (`took`). **A press within a double click's time of the tool's first frame
presses nothing on its bar until it has come up** (`crop_deaf`, as `DialogView::deaf`): the
tool's bar stands where the viewer's did, and the second click of a double click on the crop
button landed on "Whole photo". **The keyboard is the
tool's**: Enter applies and Escape cancels, each at its press, and no other key is answered;
the wheel does nothing, the other button asks for no menu, the mouse's back button still
closes the viewer, as the cross does. The tool's bar is drawn **before** its keys are read,
because its dropdown (`select_view`, `above: true`, `palette: Some(&DARK)` - the viewer's
glass is dark in both themes) takes out of the frame the Enter that chose a ratio and the
Escape that shut the list, which are not the tool's as well. **The rectangle's box is the
size the library gives** (`uncropped_width`, `uncropped_height`) fitted to the window, never
a texture's: for a photo that has a crop, the picture on hand is of the part the crop keeps,
and is drawn in that part of the box (`Cropped::part`) until the picture turned and not
cropped has been made for the tool (`PictureAsk::uncropped`, `Cropping::uncropped`) - the
Svelte tool draws nothing until its image loads. That picture is the one question asked
while the tool is open on such a photo, after the viewer's rest like any other; a photo
with no crop has it already and none is asked. Made, it is the tool's and nobody's once the tool closes; refused, why is
said and it is not asked again, and the tool stands on the stand-in. **"Apply" closes the
tool when the library has stored the rectangle** (with the turns the photo has), as the
Svelte tool does: until then the tool takes no drag, no ratio and no second "Apply" (`sent`;
"Cancel" works, and the write lands all the same); refused,
it stands to be put right and takes "Apply" again; and the answer closes only the tool that
sent it (`Cropping::number`) - one cancelled and opened again while the write was on its
way is another (`an_answer_closes_only_the_tool_that_sent_it`, which writes the crop the
photo has so that nothing else could close it). A ratio chosen on a small rectangle is
grown back to `MIN_SIDE` (`fit_aspect`): on a panorama held to 1:1 it shrank a side below the
least crop the library takes, and "Apply" was refused - as it is in `crop.ts`. Known and
left, as in the Svelte viewer: a
turn on its way when "Apply" is pressed is overwritten by the turns the tool read - which
needs `R`, `C` and Enter inside one round trip.

**The slideshow is `slideshow.rs` and `slideshow_order.rs`, and its countdown is the
photo's** (`createSlideshow`, `slideshow-order.ts`, with their tests' cases). It starts
when the photo's *full picture* is shown, or cannot be (refused, unshowable, gone) -
never at the preview, and never at the frame's word for a photo an arrow in the same frame
stepped from (`start_show`) - and a step forgets it (`App::left`, the one place every way
of going on to another photo goes through). `Slideshow::shown` counts once per photo,
because the frame asks it in every frame, and `due` leaves the step to the caller: a view
of one photo has nowhere to go and the show then stands still, as the Svelte show does.
The show's own steps, by its countdown or by hand, follow its order (`show_step`: the next
photo that is not a video, by the stride, wrapping), and do not wait for the rest a step by
hand waits for. **From a photo the view has lost - hidden under the show, gone - the walk
begins a place back**: whatever stands at its offset is the photo that came after it, as
`Viewer::step` has it; from the offset itself the show skipped that photo, and in a view of
two stood on the hidden one for good. A shuffled show's stride is drawn by the standard library's seeded hasher
(`random`), and kept across a view that changes length wherever it still fits. The
interval and the shuffle are read once at each start, off the UI thread
(`App::show_settings`; a failed read plays four seconds in order). **A show between two
photos is a still window**: frames are asked for at the countdown's end, when the controls
go (`Slideshow::next`), while the photo left fades, and once at the end of a step's rest
(which falls inside the fade), at no other time - which a test
holds over a library on a drive that is not there, a watched folder's own work asking for
frames of its own every few seconds. The photo left is the picture the last frame drew at
fit (`ViewerView::drawn`), kept as a texture and its rectangle and drawn over the one
arriving, fading over 600 ms from when that one is shown. **The window goes fullscreen as
the show starts and is given back as it stops, only if the show took it there**: `start`
and `stop` answer whether to ask (`ViewportCommand::Fullscreen`), and whether the window is
fullscreen is what it was last asked until it has done it, then what it says
(`App::window_fullscreen`, `fullscreen_pending`) - egui reports what the window last said,
a compositor says it a configure later, and a show stopped and started again at once took
the window for one the user had made fullscreen, and played in a window. Every way the viewer shuts goes through `shut_viewer`,
which stops a show first. **F11 is the window's own fullscreen everywhere**, read at the
very start of the frame, before a menu, a dialog or a held band takes the keys, and at its
press only: held, it flickered the window at every repeat. Tests play the window: `fullscreen_asked` is what the application last asked,
and `tests/slideshow.rs`'s `obey` sets the viewport's `fullscreen` to it, which
`egui_kittest` keeps from frame to frame. Escape stops a show before it closes the viewer;
`S` starts and stops, Space pauses; the show is held while a dialog or the viewer's menu
is over it and while a keyword is typed in the info panel - asked before the countdown is,
so a menu asked for in the frame before its end holds it - and the crop tool stops it. **The
controls at rest are woken by the pointer moving and nothing else**, as in the Svelte
viewer: woken by a press, the second click of a double click on the photo where the cross
had been pressed the cross it woke.

**Compare is `compare.rs` and `compare_view.rs`, and its zoom is the viewer's own**
(`createCompare`, `differingFacts`, `Compare.svelte`): one `Zoom` for every pane, measured in a
pane's size, so the point a zoom holds still and the pan's bounds are `viewer_zoom.rs`'s and
tested there; the wheel, a pinch (`Event::Zoom`) and a drag move them all. It opens from `C`
in the grid (`Ask::Compare` - not under a held press, as Enter is not: the button came up
where the grid never heard it, and a band was drawn with no button down) and the tile's
"Compare N photos", for two to four photos, **once the library has said what each is**
(`App::comparing`, off the UI thread): half a comparison is worse than none. **It goes up
at the end of the frame its answer arrives in, and only over the grid** (`compare_landed`):
a key pressed before compare was ever drawn is the grid's, and a viewer, a menu, a dialog or
a held band come since it was asked for keep the window - the answer is dropped, `C` being
there to press again. It takes the whole window as the viewer does - drawn instead of the
shell, Tab called off (a message's button over the panes would take the keyboard), its keys
read from the frame's events, and a press on anything lying over a pane (a message) not the
pane's. Panes stand in two columns, three leaving the fourth place empty so every photo is
one size. Fitted, a pane is drawn from its preview - twice or three times a pane in a window
of ordinary size, from one level; **once the zoom has left the fit every pane's full picture
is made, one after another, the pane with the keys first** (`compare_pictures`, no fitted
copy: a box of nought makes none). **A pane follows its photo**: its star and its key from
the grid on screen where that holds it - a photo turned elsewhere has its full picture made
again under its new key - and its star from what was written (`Wrote::Stars`' ids), since in
Starred a photo unstarred from compare leaves the grid and its pane would say "starred" for
good, and `S` would unstar it again. Enter opens the pane with the keys in the viewer when
the grid on screen holds the photo (the viewer goes through the grid's photos), and
otherwise does nothing. While a drag is held the keys are its: Escape lets go of it and
closes nothing. What a pane says beneath it - and to AccessKit, with its star - is only what
the panes disagree about, on every pane.

**A video is played by the system's own framework, one for each system** (spec
`2026-10-14-photon-native-video-design.md`; `video/mod.rs`, a state module, and behind it
`gst.rs`, `avf.rs` and `mf.rs`, of which one is built): GStreamer on Linux, AVFoundation on
a Mac, Media Foundation on Windows - each a poster at a time (`poster`), a player with its
sound (`Player`) whose frames come out as RGBA, and `can_play`. No fallback decoder: a
spike on all three runners (2026-10-14) had each play H.264 and seek to the frame, and
HEVC everywhere but a Windows without Microsoft's extension, which answers
`Failure::Unsupported` - as does a Linux without GStreamer's plugins, which are looked for
when a pipeline is made: the build links GStreamer's core alone, and photon starts on a
system without the rest. **A frame is handed over turned as the video is seen, and each
framework turns a different half**: GStreamer both (`videoflip video-direction=auto`);
AVFoundation the poster (`appliesPreferredTrackTransform`) and not what it plays, which is
turned here by the track's `preferredTransform`; Media Foundation what it plays (the Media
Engine) and not the poster (the source reader), which is turned here by the reader's
`MF_MT_VIDEO_ROTATION` - a backend that turned both would turn twice. The Media Engine's
first frame is black and stamped before nought, and is dropped. Every backend lets go of
what it holds when it is dropped (GStreamer to `Null`, the Media Engine `Shutdown`), and
opens the file to read it alone. **`tests/video` is a binary of its own, with no libtest
harness** (`harness = false`, `libtest-mimic` on one thread): AVFoundation's player item
is made on the main thread, and libtest runs each test on a thread of its own. Its
fixtures are four seconds of red, green, blue and white, a second each, so a frame's colour
says which second it is from (GStreamer's times run two frames behind the picture in a file
with an edit list, so a tenth of a second past each edge is let off), and a file turned a
quarter. A test whose decoder the system has not got says so and passes over itself - which
is every test of them on a Linux without the plugins, this machine's included (installing
them is the user's to decide): **only CI runs them**, and a change to a backend is checked
there, on all three.

**A picture test makes its GPU device alone** (`tests/screenshots.rs`, `DEVICES`): every
picture is drawn on a thread of its own, and with two dozen devices made in the same
moment `native-shot` crashed inside the Vulkan loader about once in four runs
(`vkEnumerateInstanceExtensionProperties`, read from the core dump). Making them one at
a time ended it; the pictures are still drawn side by side.

**A star is tested in a library of real files** (`tests/menu.rs`, `pictures`,
`over_files`): the folder's `.picasa.ini` is read back, and every write goes into a
temporary directory the test made. A fixture's folders are on a drive that is not there,
so a star there is the engine refusing - which is the test of a refusal's message. A
person to take photos off is made through the library's own calls before the launch
(`write_face_batch` for every photo, so the launch's face pass finds nothing to detect,
then `name_items`).

**Tests of the whole application drive it through `tests/common`'s `Driver`**: `until`
draws the frames that are asked for, `settle` waits in real time for the other threads,
`frames_in` counts what a stretch of the clock draws. **A sidebar row is clicked once it
is drawn** (`common::click`, `Driver::seen`): Hidden and Starred are there only while
the library holds something of their kind, which the counts say a moment after the
grid, and a test that clicked "Hidden" straight after the grid found no such row on
Windows' CI. **A wait asks for what is said,
never for how many messages stand**: when nothing has answered for a quarter of a second
of real time `until` lets the clock go on to the next frame asked for, which is the end
of a message's time, and a test that waited for "two messages" behind a slow write then
waited for ever - on Windows' CI once, after passing everywhere a dozen times. A row is clicked by its name
(`harness.get_by_label("Starred").click()`), which is why every row and button tells
AccessKit what it is (`widget_info`) although AccessKit itself is switched on only in
sub-project 7. Two things about egui's input that a frame test meets: egui works out for
itself whether a key going down is a repeat, whatever the event says, and it shows a new
`Area` - a toast - only in the frame after the one it measures it in.

Three smaller things the same review found. The list of messages a frame draws comes from
`Toasts::at`, which lets go of those whose time is over first: drawn first and let go of
after, the frame asked for at a message's end drew it once more, and on a still window
that picture stayed. `text::paint_line` gives its text its own colour: the application sets
one colour for all of egui's text, egui puts that in place of a painter's fallback, and
every dimmed line - a header's count, the line of an empty view - had come out in the full
colour since sub-project 1. And the splitter is lit under the pointer, while it is held and
when the keyboard brought the focus to it, not while it merely still has the focus a
press gave it (`Shell::pointer_focus`): egui has no `:focus-visible` of its own.

**`cargo run -p xtask -- grid-gate` is the one command that launches the application**, and
it does nothing unless told which: `--go --refresh-hz <hz>` opens fullscreen windows that
close by themselves, for about two minutes, on a desktop that has to stay awake and
untouched; `--dry-run` builds everything and launches nothing. It is run by a person, or on
their word at the time, because the interval between two frames does not exist without a
compositor. It measures `photon-native --probe` (the scroll programme, `probe.rs`, a state
module) against the Svelte grid, which is given the same programme by
`crates/xtask/gate/svelte-probe.patch` in a throwaway worktree (`target/gate-svelte`) and
never in `ui/`; `the_svelte_probe_still_applies` fails when `Grid.svelte` or `ipc.rs` has
moved from under the patch. Both run over `target/gate-fixture` (`fixture-library`: 300,000
rows on a drive recorded as unplugged, every thumbnail cached, so the engine has nothing to
do at launch). The gate asks the builder whether the fixture there is finished and its own
(`fixture --check`, `fixture::BUILDER`) and builds it again when it is not: **move
`BUILDER` with any change to what a fixture is.**

**The gate is only worth its table if the two columns are of the same thing**, and its
first version was not, in ways a review found by reading the two programmes side by side.
Kept so, each with a test: the programme moves the native grid through `GridView::move_to`,
taken with the next frame's input - set directly, the grid never knew it had moved, while
the Svelte grid's scroll event always tells it, so the native one skipped the hold after a
jump that the Svelte one paid; every jump is made from rest (`PAUSE_SECS`), or it is the
last of a stream and what is timed is each grid's own hold; the sweep ends short of the end
(`SWEEP_SHARE`), where no jump has been; each application is launched once and thrown away
before the launch that is measured, since the Svelte photon runs under directories of its
own that a first launch has to fill; and `judge` fails a table whose runs differ in photos
or window, whose given refresh rate neither grid keeps, or in which any tile was a mark
(a library whose thumbnails are not where the application looks "shows its pictures" at
once). A new step is written into both programmes, `probe.rs` and the patch's `probe.ts`.

**A test that counts frames drives the application as an event loop does**
(`the_programme_runs_over_a_fixture_to_its_report`): a frame only when egui's repaint
callback asked for one, on a clock that jumps over the time nothing did. Stepping
`egui_kittest` draws a frame at every step, asked for or not, and hid what the gate's first
count of idle frames got wrong: egui draws a second frame after every frame asked for at
once, a frame asked for after a delay comes one frame early, a scroll leaves two frames of
its own behind (`SCROLL_SETTLE_MS`, `VISIBLE_DEBOUNCE_MS`), and a nudged mouse is three
frames, not one (`ECHO_MS`). Each would have failed the gate on a perfectly still grid.
Anything that asserts a still interface draws nothing is tested through that loop.

**Two checkouts do not share a `CARGO_TARGET_DIR`.** Cargo names a workspace member's
build by its place in the workspace, not by where the checkout is, and decides whether it
is fresh from the sources of whichever checkout built it last: a second checkout then runs
the first one's binary without a word. It happened twice while the gate was built (a
reviewer's copy, and the gate's own worktree running this tree's xtask). The gate builds
its worktree into `target/gate-svelte-target` for that reason.

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
