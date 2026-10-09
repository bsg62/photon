# A native Rust UI in place of the webview

Date: 2026-10-09. Approved in conversation the same day, section by section: the three
reasons, the toolkit, the video decoders and their fallback, the long-lived branch, the
architecture, the sub-projects and their order, and the risks.

This is the umbrella design. It fixes the architecture, the order of work and what "done"
means. Each sub-project below gets its own spec and plan when it starts; only sub-projects 0
and 1 are specified next, because the gate at the end of 1 may change the ones after it.

## The goal

Replace the Svelte 5 UI and the Tauri 2 shell with a UI written in Rust and drawn on the GPU,
for three reasons the user gave, all three wanted:

- **Performance.** Thumbnails as GPU textures instead of `<img>` over `photon://`, no IPC
  round trip per page of the grid, no webview process.
- **No webview.** One renderer on three platforms instead of WebKitGTK, WebView2 and
  WKWebView, and no GStreamer plugin story inherited through WebKitGTK.
- **One language.** No hand-written TypeScript mirror, no three-file IPC rule, and UI
  behaviour under `cargo test`.

`photon-core` and `Engine` are not part of the change. What is replaced: 28 `.svelte` files
(8,917 lines), 9,132 lines of TypeScript logic, 68 vitest files (12,624 lines), and in
`photon-app` the files that exist only because of the webview (`app.rs`, `ipc.rs`,
`protocol.rs`, `media_server.rs`, `webkit.rs`).

## What was read first

`pch/rawmakase` at `4d89aaa` (0.2.2, 2026-10-09), a RAW developer for the same three
platforms. What it does, as far as it bears on photon:

- **Toolkit.** `eframe`/`egui` 0.36 with the `wgpu` renderer (wgpu 30), pinned to an exact
  patch version "because egui releases change behavior between patch versions". No toolkit
  of its own.
- **Acceleration.** egui draws the whole UI as textured triangles through Metal, Vulkan or
  DirectX 12. The app shares eframe's wgpu device for its own compute shaders and registers
  a rendered preview as a native texture, so those pixels never reach the UI thread.
- **The grid** (`src/app/library/grid.rs`, `textures.rs`). `ScrollArea::show_rows` over rows
  of one height, so only visible rows are laid out. Workers decode, send an `RgbImage` over a
  channel, and the UI uploads it with `ctx.load_texture`; 192 textures are kept, oldest
  dropped first, and a ticket per request drops a stale result.
- **No IPC.** UI code calls the domain crates. Boundaries are held by scripts in CI
  (`scripts/deps.py`, `crate-closures.sh`), not by a process boundary.
- **UI tests in `cargo test`.** A test builds `egui::Context::default()`, feeds it a
  `RawInput` through `ctx.run_ui`, and asserts on state; no window, no GPU.
- **Background work.** A single-slot mailbox per kind of job (`worker/latest.rs`: a new job
  replaces the pending one), a generation per load, and a written shutdown contract.
- **Platform glue by hand.** Its own winit event loop around eframe, `rfd` for file dialogs,
  `objc2`, `windows-sys` and `zbus` for per-OS pieces, the `fastframe-*` crates (fonts, the
  desktop's text settings, icons, themes, self-update), and its own packaging scripts
  (Inno Setup, DMG, deb/rpm), there being no Tauri bundler.
- **What it does not have.** eframe's `accesskit` feature is off. A working graphics driver
  is required. It has no video.

## Decision

Build photon's UI on `eframe` + `egui` + `wgpu`, calling `Engine` directly, in rawmakase's
shape.

Rejected:

- **egui for the chrome and photon's own wgpu renderer for the grid and viewer.** Most
  headroom at small tiles on large screens, but hit-testing, focus and accessibility for
  tiles become hand-written and pixel output cannot be tested without a GPU. Kept as the
  fallback for the grid alone, chosen on the numbers of sub-project 1 and not before.
- **A retained-mode toolkit (iced, Slint).** Closer to how the Svelte code is laid out and
  better at text, but there is no reference application to build from, which was the premise
  of the request, and Slint's own markup language works against "one language".
- **Keeping a webview for video only.** It keeps every cost the second reason is about.

## Architecture

### Crates

- **`photon-core`**: unchanged.
- **`photon-engine`** (new): `engine.rs`, `commands.rs`, `watch.rs`, `events.rs`,
  `memory.rs`, `error.rs` and `testutil.rs`, moved out of `photon-app`. None of them names
  Tauri today (checked: no mention in any of the seven); three references into the files
  that stay are settled in sub-project 0's spec. The move lands on
  **main**, before the branch exists, so that later fixes to the engine on main merge into
  the branch without conflict.
- **`photon-ui`** (new, on the branch): the egui application and the `photon` binary. It
  depends on `photon-engine` and never on Tauri.
- **`photon-app`**, `ui/` and the `mock.js` screenshot harness stay untouched on the branch
  until the last sub-project deletes them. Until then the branch builds both applications.

### The UI and the engine

- **Events in.** `Engine::open` takes an `Arc<dyn Events>` (five methods:
  `library_changed`, `scan_progress`, `folder_status`, `export_progress`, `face_progress`).
  The UI's implementation pushes each onto a channel and calls `ctx.request_repaint()`.
  `scanning_folders` is still asked once after the listener exists, for the reason CLAUDE.md
  gives: the startup scans begin before anything can hear them.
- **Calls out.** The UI calls `commands::*`. A missing field is a compile error; the
  three-file rule, `capabilities/default.json` and `api.ts` have no successor.
- **The threading rule.** Nothing that reads SQLite or the filesystem runs on the UI thread.
  A command that returns `CmdResult` or reads `engine.lib` goes through a task layer: a
  latest-wins mailbox per kind of request and a generation, so an answer that arrives after
  the question changed is dropped. This is the job `LibraryStore`'s generation counters do
  today. Commands that read only the published `GridIndex` may be called inline:
  `grid_rows`, `grid_folder_ids_at`, `grid_offset_of_folder` and `grid_offset_of_item` are
  such (read: each is `engine.grid()` and a lookup). `neighbours` is not; it reads an item
  per id. Sub-project 1's spec checks every command it calls against the rule.
- **A consequence worth stating.** With the index in the same process, the grid has no
  pages to load. The rules that exist for an unloaded page (`adoptLead`, a range taking its
  lead's photo from fetched ids) may have no successor. "An offset is only meaningful against
  one index version" does not go away: the viewer and the selection still re-find their photo
  by id after a rebuild.

### The UI's own structure

- **State modules without egui**, one per concern, plain Rust with tests. The `.svelte.ts`
  factories and the pure modules port one to one (`layout.ts`, `scroll-map.ts`,
  `timeline.ts`, `crop.ts`, `picture.ts`, `createSlideshow`, `createSearchBox`,
  `createCropTool`, the selection store, the People page's state), and their vitest cases
  with them.
- **View functions** that draw one state module and turn input into calls on it. They get
  headless `Context::run_ui` tests for keys and clicks. Today that layer is covered only by
  `svelte-check` and the smoke checklist.

### Pictures

- **Thumbnails.** A worker pool reads the cached WebP from `ThumbCache`, decodes it to RGBA
  and sends it over a channel. The UI thread uploads a bounded number per frame into a
  texture cache keyed by `(thumb_key, size)` and bounded by bytes. The thumbnail queue and
  its priorities are reused for a photo not yet rendered.
- **The viewer's photo.** The decode and the edit rendering now inside `protocol.rs`'s
  `image()` move into an engine function that returns pixels, still one at a time under
  `RENDERING`. An edited photo no longer makes a JPEG round trip on its way to the screen.
- **Face crops.** `ThumbCache::face_crop` from the same pool, under the same bound of one
  permit a core.
- **Video.** See below.

## Video

Decided: the operating system's decoders. One `video` module with one trait (open, play,
pause, seek, the current frame as RGBA, a poster frame at a given time) and three
implementations: AVFoundation, Media Foundation, GStreamer. Each player plays its own audio.
The loopback media server and the webview thumbnailer are deleted.

**This is a spec-level decision under CLAUDE.md's "no system library dependencies" and
"nothing wrapping a C/C++ SDK", and it is taken here.** AVFoundation and Media Foundation
are part of the OS. On Linux the backend binds GStreamer, a system library photon already
needs for video today, through WebKitGTK; the `.deb` keeps its dependency on the plugins and
the AppImage still bundles none.

**If video makes trouble, it is dropped for now** (the user's ruling, 2026-10-09). Video
gates nothing. Dropped means, per platform:

- videos stay in the library and in the Videos view;
- a poster frame already in the cache is shown, and a video without one has a placeholder
  tile;
- opening a video hands it to the system's player, through the open-in-app path photon has;
- the parity checklist leaves video out, and the release notes say so.

A backend that works ships; a platform whose backend does not falls back. The fallback is
built either way.

## Sub-projects

Each has its own spec, plan and reviewed pull request into the branch.

**0. Extract `photon-engine`**, on main. A move with no change of behaviour.

**1. Foundation and the grid slice.** The gate for everything after it.
- The window, the light and dark tokens of `tokens.css` as egui styles, fonts, the Lucide
  icons, the `Events` wiring, the task layer, the texture cache.
- The grid in All photos, read-only: rows, sections, headers, the pinned header, tiles
  widened to fill the row (`tileFor`), the place kept across a resize (`Pin`), and a scroll
  position that reaches the end of a library taller than the toolkit's own scrolling can
  address.
- File names in CJK, Arabic and emoji drawn correctly.
- The measurements of "The gate", below.

**2. Sidebar, views and search.** The folder tree with its year groups, albums, people,
tags, saved searches and their counts; view switching; the search box and its help; the sort
and grouping control; the status bar; the empty-library panel; the year strip; the splitter
and the hidden sidebar.

**3. Grid interaction.** Selection by id, Shift ranges, the rubber band with autoscroll, the
keyboard lead and the page keys; the shared context menu; star, hide, tags, albums and
captions; the folder menu and rename; copy photo; folder drop; export and its progress.

**4. Viewer.** The photo, zoom and pan, the info panel beside it, the histogram, face
outlines and naming, turns and the crop tool, the neighbours' preload, the slideshow,
compare, and the conversion to sRGB of risk 1.

**5. Video.** A spike first (throwaway: can each OS player hand frames to a wgpu texture and
give a poster frame, for an H.264 and an HEVC file; can the hosted macOS and Windows runners
run that as a test, since a poster frame needs no window and no audio). Then the backends,
the poster-frame worker in place of the webview thumbnailer, and the controls - or the
fallback above.

**6. Remaining pages.** The People page, Settings, Statistics, the shortcut sheet,
duplicates and copies.

**7. Platform and packaging.** Window state, the themed title bar, fullscreen, file dialogs,
the clipboard, reveal and open-in-app, AccessKit switched on, the fallback for a machine
without a usable GPU, and the installers built without Tauri's bundler (two `.dmg`, `.deb`,
AppImage, `.msi`; the MSI's opt-in desktop shortcut is a copied Tauri WiX template today and
has to be rebuilt). The version's authority moves from `tauri.conf.json` to `Cargo.toml`,
and `xtask versions` with it.

**8. Switch-over.** A new screenshot harness and the website's images, the smoke checklist
and CLAUDE.md rewritten, `ui/`, `photon-app` and npm deleted, the whole-branch review, the
merge. The release that carries it is a minor at least.

## The branch

One long-lived branch, `native-ui` (the user's choice over building beside the Svelte UI on
main). Main is merged into it after every release on main. Sub-project 0 is on main for that
reason. Nothing is said here about freezing the Svelte UI: a feature added on main during
the rewrite is one more row in the parity checklist.

## Parity

The branch is done when the native UI answers all of:

- every key in `lib/shortcuts.ts`;
- every item of `docs/smoke-checklist.md` (382 today);
- every surface in the screenshot harness's `SHOTS`;
- every public function of `commands.rs`: a test fails on one `photon-ui` never calls,
  unless the function is deleted on purpose. It is the successor of the test that holds
  `mock.js` to `api.ts`.

Less video, where it was dropped.

## Verification

- **The Rust gate covers the UI.** The npm gate ends at the switch-over; CI then installs
  neither webkit2gtk nor Node.
- **A ported test is a new test.** The code under it is new, so it is shown to fail with its
  rule broken, as Conventions requires of any new test.
- **Layout is asserted in tests.** egui lays out without a GPU, so a test reads a widget's
  rectangle. That replaces the headless-Chromium layout probe.
- **`scroll-probe` becomes a unit test** of the scroll map and the layout state.
- **The look still needs eyes.** A screenshot harness renders the real UI to PNG through a
  software adapter; like today's it is not run in CI and replaces no item of the smoke
  checklist. Sub-project 1 confirms it runs headlessly.
- **"Never launch the GUI to verify a change" stands.**

## The gate

At the end of sub-project 1, on the 300,000-photo library, on one machine, the native grid
against the Svelte grid:

- frame time while scrolling within one refresh interval at the 95th percentile;
- no worse in time until a tile shows its picture, in memory, and in launch to first paint.

Sub-project 1's spec records the numbers and how each was taken. If the first line fails,
the grid gets its own renderer (the first rejected alternative) before anything is built on
it. If text in other scripts cannot be drawn, that is a finding of the same weight.

## Risks

1. **Colour management.** photon converts no ICC profile itself (no code in `thumbs`,
   `decode.rs` or `turbo.rs` reads one). Thumbnails and edited renders already ignore the
   profile; an unedited photo in the viewer is the original file, and the webview applies its
   embedded profile. Natively an Adobe RGB or Display P3 photo would look flat. Sub-project 4
   converts to sRGB on decode in pure Rust; which crate is that spec's decision.
2. **Text.** File names, captions and people's names are in any script, and egui's text
   layout and input method support are weaker than a browser's. Tested in sub-project 1.
3. **A GPU becomes a requirement.** A virtual machine, a remote desktop or an old driver
   works today and may not after. wgpu's GL backend and the software adapters are the
   fallbacks; a plain error dialog when none starts.
4. **Accessibility gets thinner.** AccessKit covers standard widgets. Tiles, the viewer and
   the menus are custom-drawn and need hand-written nodes; `inert` and today's ARIA
   structure have no direct equivalent.
5. **Very large photos.** A texture's edge is limited by the device (8192 or 16384 px). The
   viewer downscales to the limit, so the deepest zoom into a large panorama is lower than
   today on some machines.
6. **Toolkit churn.** egui is pinned to an exact version. `fastframe` crates are taken only
   where sub-project 1 shows a need (fonts and the desktop's text settings are the likely
   ones), and its forks of egui and winit are not adopted unless forced.
7. **Branch drift.** Answered by "The branch", above.

## Not decided here

- Whether the Svelte UI on main is frozen during the rewrite.
- Everything inside sub-projects 2 to 8 beyond their scope.
- The crate for colour conversion, the installer tooling, and the screenshot harness's
  mechanism: each in its sub-project's spec.
