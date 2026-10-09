# The native UI's foundation, and the grid as its first slice

Date: 2026-10-09. Sub-project 1 of `2026-10-09-photon-native-ui-design.md`. It is built on
`native-ui`, after sub-project 0 (`2026-10-09-photon-engine-crate-design.md`) is on main and
merged in.

**Changed since approval**, each found while the plan was written against a scratch build
(egui 0.36.2, the real engine): the fonts come from `fastframe-fonts` 0.4.1, which does
offer the platform's own face; text needs a module of its own, because egui draws a line
that mixes writing directions wrong ("Text in other scripts", below); a thumbnail that was
not to be had is asked for again after five seconds, not on its next frame in view; dates
are written in English; the workspace's Rust version becomes 1.98; and the sub-project has
two plans, the grid first and the gate second.

**Changed by the review of the built slice** (2026-10-09): the stored theme and tile size
are read in `App::new`, before the first frame, and not by the task layer, whose one user
is the folder list; a thumbnail that is not there yet marks its tile and counts as settled,
and a frame is requested for its retry; a built thumbnail is handed back only under its own
key; a right-to-left run is cut at everything that is not a letter, with brackets mirrored.

**Changed by the gate's plan** (2026-10-09), each found while the gate was built and driven
without a window: the Svelte grid measured is the one on `native-ui` itself, patched in a
throwaway worktree, not the commit the branch began from (main is merged in as it moves, so
it is the newer of the two); "time to picture" is the time from a jump, or from the end of
the sweep, to the first frame in which every tile in view has its picture, not a time per
tile; the screen's refresh rate is given by whoever runs the gate (`--refresh-hz`) and not
read; the native grid is run first and the library is read into the page cache before each
run, so that what is cold counts against the grid that has to pass; the pass lines are
numbers this spec did not give - a scroll passes with its 95th-percentile interval at or
under one and a half refreshes (a missed refresh is two), and "no worse" is within 5%, plus
one refresh on a time; and step 6 counts the frames the grid draws *by itself* - there is a
second of rest before it, the frame that ends it is not counted, and neither is a frame the
engine's own report asked for (it looks for the fixture's unplugged drive twice a minute).
The fixture's builder lives in `photon-ui` (`fixture.rs`, run as an example), so xtask goes
on depending on neither crate.

## What this is for

Two things, and the second is why it comes first.

1. The foundation every later sub-project stands on: the crate, the window, the theme, the
   wiring to the engine, the task layer and the texture cache.
2. An answer, in numbers, to whether egui can draw photon's grid well enough on a
   300,000-photo library. If it cannot, the grid gets its own renderer before anything is
   built on it (the umbrella design's first rejected alternative).

The slice is the grid in whatever view and arrangement the engine opens with, read-only: it
scrolls, and it shows photos. It is not a product and is not released.

## What is in it

### The crate

`crates/photon-ui`, with a binary named `photon-native` (the name `photon` belongs to
`photon-app`'s binary until the switch-over). It depends on `photon-engine` and
`photon-core`, on `eframe` with the `wgpu` renderer pinned to an exact version, and on
nothing of Tauri's.

The version to pin is the one `pch/rawmakase` runs (0.36.2) unless a newer release exists
when the plan is written. The plan reads that version's source for every API it names:
nothing here is written from memory of an older egui.

```
src/main.rs        logging, the window, run
src/args.rs        the command line
src/app.rs         the eframe App: owns the Engine and the state, orders a frame
src/events.rs      the Events implementation
src/tasks.rs       latest-wins mailboxes and generations
src/dirs.rs        where the library and the cache are
src/text.rs        one line in any mix of scripts
src/theme/         tokens.rs (the colours and scales), apply.rs (onto egui), fonts.rs
src/icons.rs       the Lucide paths the slice draws
src/grid/
  layout.rs        rows, sections, tiles: the port of layout.ts
  motion.rs        still, scrolling or jumping: the port of scroll-speed
  scroll.rs        the grid's position and its scrollbar
  visible.rs       telling the engine what is on screen
  labels.rs        what a header and a badge say
  view.rs          draws the grid and turns input into calls on the above
  header.rs        a section's header, in its row and pinned
  tile.rs          one tile
src/thumbs/
  loader.rs        reading, waiting for and decoding thumbnails, off the UI thread
  textures.rs      which textures are held: bookkeeping, no GPU in it
  shown.rs         the loader's results uploaded as textures
  source.rs        the engine as the place thumbnails come from
src/probe.rs       the scroll programme the gate measures (the second plan)
```

**State modules name no egui type.** `args.rs`, `tasks.rs`, `dirs.rs`, `theme/tokens.rs`,
`grid/layout.rs`, `grid/motion.rs`, `grid/scroll.rs`, `grid/visible.rs`, `grid/labels.rs`,
`thumbs/loader.rs` and `thumbs/textures.rs` are plain Rust. A test reads their source and
fails on `egui`.

### The window and the engine

- One window, 1280x800 at first launch with a minimum of 800x500, no stored state. Window
  state, the themed title bar and fullscreen are sub-project 7's; `--fullscreen` exists for
  the probe.
- **The library opened is the one the Svelte UI opens.** `dirs.rs` resolves the directories
  Tauri resolves for the identifier `io.github.bsg62.photon` (`library.db` in the app data
  directory, `thumbs` in the app cache directory), verified against Tauri's own source and
  pinned by a test per platform. `--data-dir` and `--cache-dir` override them, which is how
  the probe opens its fixture.
- `Engine::open`, then `startup` on a thread as `app.rs` does today, `shutdown` when the
  window closes. photon has no single-instance guard today and the slice adds none: running
  it beside the Svelte photon on one library is as unsupported as running two photons is.
- **Events.** The `Events` implementation sends each event down a channel and calls
  `ctx.request_repaint()`. The slice acts on `library_changed` only; the other four are
  received and dropped, so the channel never fills.

### The task layer

`tasks.rs`: a mailbox that holds one pending job per kind (a new one replaces it), a worker
thread, and a generation per kind so an answer to a question no longer asked is dropped. A
job that panics fails its own answer and leaves the worker running. The slice has two
users: the folder list for the headers (`commands::list_folders`, read again on
`data_changed`) and the stored theme and tile size (`commands::theme`, `grid_tile`).

What the grid reads per frame is not a task: `Engine::published()` and
`GridIndex::rows`/`sections` are in memory.

### Theme, fonts and icons

- **Tokens.** `theme/tokens.rs` holds the light and dark colours and the scales of
  `ui/src/tokens.css` as constants. Until the switch-over `tokens.css` stays the source: a
  test reads it and fails when a value differs. `apply.rs` maps the tokens onto egui's
  visuals and spacing. The stored `theme` setting chooses; System follows the desktop.
- **Fonts.** The Svelte UI uses the platform's interface font (`system-ui`). The slice does
  the same, with installed fonts for scripts that font lacks, through `fastframe-fonts`
  0.4.1 (`Primary::System`, without its `inter` feature, so nothing is bundled). egui's own
  faces are the last fallback, not the look.
- **Text in other scripts.** egui shapes text - Arabic letters join - but runs no
  bidirectional algorithm: it sets stretches of one font face down left to right, each in
  the direction of its first strong letter. Read out of its glyph positions on 2026-10-09,
  a year inside a Hebrew name came out reversed, a two-word Arabic name with its first word
  on the left, and a Latin word after Hebrew backwards. `text.rs` cuts a line into the runs
  the algorithm finds (`unicode-bidi`), and a right-to-left run into its words, and lays
  each piece out by itself in the place it is read at. That covers every line photon
  paints. It does not cover a text field, whose caret and typing in mixed-direction text
  are a finding for the sub-project that brings the search box. Emoji are drawn in one
  colour, by egui's own emoji font.
- **Icons.** The four the slice draws (star, play, copy, triangle-alert), from the same
  Lucide path data as `lib/icons.ts`, rasterised through egui's SVG loader and tinted with a
  token.

### The grid's geometry

`grid/layout.rs` is `layout.ts` in Rust, constant for constant: `TILE_WIDTH`, `GAP`,
`HEADER`, `SECTION_GAP`, `TILE_MAX`, `columnsFor`, `tileFor`, `hasHeader`, `buildRows`,
`totalHeight`, `rowIndexAt`, `visibleRange`, `headerRows`, `pinnedHeader`, `pinAt`,
`pinTop`, and the overscan rules below. Their vitest cases are ported with them.

Not ported in this slice, because nothing in it calls them: `pageMove`, `rowOfItem` beyond
what `pinTop` needs, `itemsInRect`, `edgeScrollSpeed`, `scrollIntoGrid`, `scrollToStart`,
`topFolderId`, `showsTimeline`, `layoutHeight`.

Rows are rebuilt when the published `layout_gen`, the column count or the drawn tile
changes, and not otherwise.

### What immediate mode changes

Several of the Svelte grid's rules exist because the DOM is laid out after the script that
changed it, and have no successor here. Saying so is part of the design: a reader who
ports them anyway adds code that does nothing.

- **No scroll map.** `scroll-map.ts` exists because a browser box stops at 33,554,428 px.
  The native grid does not put its rows in a scrolling container at all. `grid/scroll.rs`
  holds the position as an `f64` in the layout's own coordinates, and the view draws each
  visible row at `row.top - position`. egui's `ScrollArea` is not used for the grid: its
  offset is an `f32`, which is exact only up to 16,777,216, and CLAUDE.md records libraries
  taller than twice that.
- **Its own scrollbar.** Drawn by the grid, always taking its room (the successor of
  `scrollbar-gutter: stable`), with a thumb of a minimum length. Its mapping is
  proportional and exact at both ends; a drag is a stream of jumps to `motion.rs`.
- **No mounting.** `renderOverscan` and `renderRange` decided which tiles exist in the DOM.
  Here every visible row is drawn every frame and nothing else is. The same numbers
  (`RENDER_OVERSCAN`, `LEAD_MS`, `LEAD_OVERSCAN_MAX`, `TRAIL_OVERSCAN`) survive as the
  **wanted range**: the rows whose thumbnails are asked for ahead of the scroll.
- **No `placeIn`, `restoredTo` or `viewTop`.** They bridge the render where the tiles change
  and the effect that moves the viewport after it. Here one function reads the pin from the
  old rows, builds the new ones and sets the position, before anything is drawn.
- **No pages.** The index is in the process. `fetchSpan`, `FETCH_OVERSCAN` and
  `library.ensure` have no successor.

What does not change: the `Pin`. Every row's height still moves with every pixel of a
resize, so the place is kept as the row at the top and the share of it scrolled past, and
restored on any change of the tile or the column count.

Across a rebuild of the index the position is kept as it is and held to the new end, which
is what the Svelte grid does.

### Drawing

- **A header** shows what the `heading` snippet shows: a folder's label, its summary
  (`folderSummary`) and its path, or a period's label (`periodLabel`, from the section's own
  numbers) and its count. Its dates are in English: the browser wrote them in the system's
  locale, photon takes no ICU, and what to do about locales is decided with the sidebar,
  which reads the same instants. The same function draws it in its row and pinned over the top of
  the grid, pushed out by the next header as `pinnedHeader` says.
- **A tile** is a square. The picture covers it, centre-cropped by the texture's own
  proportions. Over it: the star, the video badge with its running time, the copies mark,
  and for a thumbnail that failed the warning icon (the play icon for a video). No ring, no
  hover, no click.
- **Input.** The wheel and the touchpad move the position by egui's smoothed delta. Home
  and End go to the ends, Page Up and Page Down move by a viewport less one row, the arrows
  by 40 px. A press or drag on the scrollbar moves through its mapping. Nothing else is
  answered.

### Thumbnails

The path of a picture: wanted range, loader, upload, texture cache, tile.

- **Asking.** Each frame the view names the wanted range's photos by `(id, thumb_key)`.
  `motion.rs` and `defersThumbs` decide, as today, whether a tile asks at once or after
  `TILE_SETTLE_MS`: a tile that will be gone before then (a scrollbar drag, a scroll faster
  than the mounted window) does not cost a render.
- **A cached thumbnail** is read and decoded on a small pool: `ThumbCache::read`, made
  public, which decodes with libwebp in about a tenth of a millisecond. Nothing outside
  `photon-core` names libwebp.
- **A thumbnail not built yet is awaited, never blocked on.** All such waits share one
  thread that polls `ThumbService::request_async` futures, each bounded by the 30 seconds of
  today's `THUMB_TIMEOUT`. A tile that leaves the wanted range drops its future, which is
  what gives up the wait. `protocol.rs` records why: run on blocking threads, a fast scroll
  parked hundreds of them and cached thumbnails queued behind.
- **What is on screen is reported** to the engine's own queue with `set_visible`, debounced
  as `VISIBLE_DEBOUNCE_MS` does today.
- **Results** come back over a channel with the key they were asked under. One whose photo
  now has another key, or that is no longer wanted, is dropped.
- **Upload.** The UI thread uploads at most a fixed number of textures per frame and asks
  for another frame while any wait.
- **The cache** is keyed by `(thumb_key, size)` and bounded by bytes, least recently drawn
  first, and never evicts a texture in the wanted range. It starts at 256 MiB, 32 uploads a
  frame and two decode threads; all three are tuned against the gate and the values kept
  are recorded with their measurements.
- **A failure** (`ThumbFailed`) is remembered per key and drawn as the icon. A thumbnail
  that was not to be had - not built in time, its photo gone - is left alone for five
  seconds and then asked for again, while it is still wanted: asked for on its next frame,
  a photo that answers at once would be asked for sixty times a second.

Each tile's texture is its own, so egui issues one draw call per tile. If the gate shows
that to be the cost, the first answer is to pack thumbnails into a few large textures and
draw them by their rectangles, still inside egui. An own renderer is the answer after that.

### Seeing it without a display

`cargo run -p xtask -- native-shot` writes `target/screenshots/native-grid-light.png` and
`native-grid-dark.png`: the real grid over the CC0 photos in
`crates/xtask/screenshots/photos`, rendered off screen. The plan tries `egui_kittest`'s
wgpu rendering first and renders through `egui-wgpu` directly if that does not serve. Like
`screenshots`, it is not run in CI.

One folder of that fixture is named in Japanese, one in Arabic and one with an emoji. The
PNGs are read, and what they show of those three names is written into the pull request.

## What is not in it

Selection, clicks, the context menu, the rubber band and the keyboard lead; the tile's
fade-in and the slow background retry of a broken tile; the restore of the last folder and
remembering the folder at the top; the year strip; the empty-library panel; every view
switch, the sort and size controls, the sidebar, the status bar; the viewer. Each is named
in the umbrella design's sub-project that owns it.

AccessKit, the GPU fallback and window state are sub-project 7's. The slice logs which
adapter it started on.

## Tests

Each is shown to fail with its rule broken, as Conventions requires.

- **`layout.rs`**: the ported cases of `layout.test.ts` for every function ported.
- **`motion.rs`**: the ported cases of the scroll-speed tests.
- **`scroll.rs`**: at a layout 40,000,000 px tall, a step of one pixel moves the position by
  one pixel anywhere in it; the thumb at its end is the last row and at its start the first;
  a position past the end is held to the end. This is `scroll-probe`'s successor.
- **`textures.rs`**: the byte bound holds; a wanted texture is never evicted; a result under
  a key the photo no longer has is dropped; no more than the budget is uploaded in a frame.
- **`loader.rs`**: with more unbuilt thumbnails waited for than there are decode threads, a
  cached one is still delivered at once.
- **`tasks.rs`**: a newer job replaces the pending one; an answer from an older generation
  is dropped; a panicking job fails its own answer and the next job runs.
- **The view, headless** (`Context::run_ui`): a wheel delta moves the position by that much;
  End shows the last row; narrowing the window keeps the photo at the top (the `Pin`); deep
  in a folder the pinned header names it, and the next header pushes it out; a
  `library_changed` with a new `layout_gen` rebuilds the rows and one without does not.
- **Tripwires**: the tokens equal `tokens.css`; the directories are Tauri's, per platform;
  the state modules name no egui type.

## The gate

The second plan of this sub-project, written once the grid has landed: its native half
hooks the frame loop built here, and its Svelte half is a patch against `Grid.svelte`.

### The fixture

`cargo run -p xtask -- fixture-library --photos 300000 --out <dir>` writes a library and a
thumbnail cache: rows through `Library`'s own writers, modelled on the grid bench's
`synthetic_library`, under one watched folder whose directory does not exist, which is an
unplugged drive (nothing is scanned, marked or purged, and no Pictures folder is added). A
few dozen distinct grid thumbnails, made from the CC0 photos, are hard-linked under every
row's key.

### The programme

Both applications run it fullscreen on the same monitor, one after the other, on that
fixture:

1. launch, until the first frame that shows pictures;
2. a steady scroll, 3,000 px a second for ten seconds;
3. a fast scroll, 30,000 px a second for five seconds;
4. a jump to the end, then to the middle, each until every tile on screen has its picture;
5. a sweep from top to bottom in five seconds, by position, as a scrollbar drag is;
6. five seconds idle.

It records, per step, the intervals between frames (median, 95th and 99th percentile,
longest), the time from a tile entering the viewport to the frame that shows its picture,
and at the end the memory of the whole process tree by `memory::usage`, which already
counts a webview's processes. Also the window's size, the scale, the refresh rate and the
adapter.

- **Native**: `photon-native --probe <out.json>`, which drives `scroll.rs` directly.
- **Svelte**: a release build of the commit `native-ui` branched from, with a probe script
  that drives the viewport the same way and reads `requestAnimationFrame`. The script is a
  patch file kept beside the harness and applied for the run; it is not merged into `ui/`.
  On Linux the fixture is reached through `XDG_DATA_HOME` and `XDG_CACHE_HOME`.

`cargo run -p xtask -- grid-gate` runs both and prints the two columns. **It opens two
fullscreen windows on the desktop for about two minutes.** It is therefore run by the user,
or on their word at the time, and never as part of a gate; this is the one sanctioned
exception to "never launch the GUI", because a frame interval does not exist without a
compositor. It is measured on Linux, on this machine. macOS and Windows are not part of the
gate; they are first run when sub-project 7 produces installers.

### Passing

As the umbrella design sets it: in steps 2 and 3 the native grid's frame interval is within
one refresh interval at the 95th percentile, and it is no worse than the Svelte grid in
time to picture (steps 4 and 5), in memory, and in launch. The pull request carries the
table, the fixture's command and the tuned values.

Two findings weigh as much as a failed number and are reported with it: a name in another
script that is not drawn correctly, and any frame drawn during step 6, since an idle grid
that repaints is a bug in the wiring.

## Risks particular to the slice

- **The comparison's two clocks.** A frame interval read in `update` and one read in
  `requestAnimationFrame` are both the cadence the compositor allowed, but they are not the
  same instrument. A result within a few percent either way is a tie, and is reported as
  one.
- **A tiling compositor decides a window's size.** Hence fullscreen for both.
- **Hard-linked thumbnails are cheaper to read than 300,000 distinct files**, for both
  applications alike: the page cache holds a few dozen files. Time to picture on a cold
  cache of a real library is larger than the gate will show.
- **CI on Linux** may need packages for winit to build. Found in the plan, and written into
  CLAUDE.md's Commands when known.
