# Performance audit: open items (handoff, 2026-09-28)

The performance audit of 2026-09-27 shipped in five PRs:

| PR | Contents | Release |
|---|---|---|
| #113 | query plans, header reads, post-scan passes, lock scope, watcher cache | 0.36.0 |
| #114 | refresh chain, paced scan rebuilds, faster search | 0.36.0 |
| #115 | lost watcher events, per-photo star busy flag | 0.36.0 |
| #116 | thumbnails, viewer, grid, startup, scan | 0.36.0 |
| #117 | fast-scroll pop-in and the follow-ups | 0.36.1 |

This file lists what is still open, roughly in priority order. Each entry names where to start. The PR descriptions and commit messages hold the measurements and the reasoning behind each change.

## 1. Manual smoke checks nobody has run yet

These can't be tested in CI. The component wiring has no harness, and the headless measurements ran in Chromium only. They are on the README's `## Manual smoke checklist`. Run them on each platform's real webview: WebKitGTK on Linux, WKWebView on macOS, WebView2 on Windows.

- **Fast scroll fill-in (#117).** Wheel and trackpad flicks at several speeds should show no blank tiles popping in. End, Home, a scrollbar-thumb drag, the year strip and a folder jump should all stay smooth.
- **Folder list (#116).** Drag the sidebar splitter with thousands of folders. Scroll the list. Tab through it: the focus rings on the first and last rows of each year group must not be clipped.
- **Viewer stepping (#116).** Hold → and let go. The full-size photo appears about 130 ms after you stop; a slideshow is unaffected. View photos during an import scan: the photo must not get stuck on the blurry preview, and a slideshow must not stall.
- **`/image` revalidation (#116).** Open an edited photo, go away and come back. It should not re-render. Then click Original and come back: it must show the original. Whether each webview sends `If-None-Match` for the custom scheme is unknown. If one doesn't, the ETag does nothing there, but it is still harmless.
- **Startup (#116).** The window appears before the grid is built. The status bar shows no count until the grid arrives, and "No photos yet" never flashes on a non-empty library.

## 2. Fast-scroll pop-in at extreme speed

- **Measured result.** In the headless-Chromium harness, blank on-screen tile-frames are 0% at 4 px/ms and 0.3% at 8 px/ms, but about 58% at 16 px/ms.
- **Why.** At 16 px/ms a frame running 50–117 ms moves a whole viewport. `createScrollSpeed` then classifies that frame as a jump, and a jump drops overscan.
- **Where to start.** `ui/src/lib/scroll-speed.svelte.ts` (`Motion`) and `ui/src/lib/layout.ts` (`renderOverscan`, `renderRange`, `defersThumbs`).
- **Idea.** Classify a jump by distance relative to the elapsed time, not per event. Or keep the lead during a sustained stream of events even when one frame is long.
- **Untested.** Real trackpad momentum curves and 120 Hz screens.
- **Reproducing the measurements.** The harness from #117 lived in a scratchpad and was not committed. Rebuild it as follows:
  - serve `ui/dist` with an enlarged `mock.js` (20,000 photos in 400 folders);
  - answer thumbnails through a service worker with a configurable delay, so Chromium's six-connections-per-host limit doesn't skew the numbers;
  - drive `scrollTop` from `requestAnimationFrame` at a constant px/ms;
  - count on-screen tiles with no `<img>`, still loading, or still fading.

## 3. Hide and selection edge cases (`ui/src/lib/library.svelte.ts`)

- **A band released while a hide is in flight** can put the hidden photos back in the selection. `endBand` keeps local copies of `bandBase` and `bandPrevious` from before its `await`. `setHidden` strips the hidden ids from the store's copies but not from those locals. Fix: re-read the store's sets after the await, or strip them in `endBand` against a "recently hidden" set.
- **A selection that reaches above its anchor**, and is wider than the loaded pages, lands at the lead's offset after a hide rather than just after the selection. `landingOf` computes the selection's start as `min(anchor, lead, first unloaded offset below the lead)`. The true minimum of the selection can be lower (for example Ctrl+click added a photo far above). Fix: carry the selection's minimum offset when it is known, or fetch it.

## 4. Writers that report a failed rebuild as a failed write

`set_items_hidden` was fixed in #117. It commits the write, then logs a failed `refresh_grid` instead of returning it. The same write-then-`refresh_grid()?` pattern is still in these functions in `crates/photon-app/src/engine.rs`:

- `set_stars`
- `add_item_tag`, `add_items_tag`, `remove_items_tag`
- `set_folder_hidden`, `set_folder_alias`
- `albums_changed`
- `write_edit`
- `remove_folder_inner`

In each, the change is saved but the UI shows an error, and may keep a selection or state for a retry that has nothing left to do. Decide per writer whether logging is right. Where the UI acts on the error, keep it.

## 5. Smaller ideas the audit raised but nothing has picked up

- **Keyword order in search text.** In the search haystack, keywords now follow the query's row order (`search_tags` in `library/items.rs`). This only matters for a quoted phrase spanning two keywords. `ORDER BY e.item_id, e.tag` would make it deterministic again, at the cost of a sort over the keyword rows.
- **The JPEG decode is now the largest stage of a thumbnail,** about 130 ms of about 140 ms at 24 MP. zune-jpeg has no DCT scaling. `jpeg-decoder`'s `scale()` could decode at 1/2 or 1/4 for previews. Benchmark it first: its entropy decoder is slower, so 1/2 may be a wash.
- **Duplicate candidates are chosen by byte size alone,** so roughly 5–10% of a large library shares a size by coincidence and gets read in full once. A first-64-KiB hash as a pre-filter would cut that read. It needs a schema column, which makes it a minor release.
- **Thumbnail workers** stay capped at 8 (`MAX_WORKERS`, `thumbs/service.rs`). After #116 each worker needs about half the memory, so a RAM-derived cap is possible. The cap was also about the disk, so measure before raising it.

## 6. A correctness risk found by the audit, never verified

**The grid canvas may exceed the browser's layout-height limit.** The UI audit worked out a case where it would: 300k photos, large tiles, and a narrow window of about 800 px wide with 2 columns. There `.canvas` in `Grid.svelte` reaches roughly 35M px. WebKit and Blink both clamp layout at about 33.5M px, so the end of the library would become unreachable by scrolling.

Nothing has checked this. Measure the canvas height at the widest tile setting and the narrowest window. If it does exceed the limit, the fix is to scale scroll positions: keep the DOM canvas under the limit and map `scrollTop` to rows proportionally.

## 7. Audit findings nobody has picked up

These came out of the audit on 2026-09-27 and are not in any PR. They are ordered roughly by how often the cost is paid.

**`grid_info` sends every section and folder on every call** (`commands.rs` `grid_info`). That is about 1 MB of JSON at 5k folders, and `grid_info` runs on every grid version. It also runs four COUNT queries.
- Idea: a `layout_gen` counter bumped only when sections or folders actually change. `grid_info(known_gen)` then omits both when unchanged.
- Cache the counts under a data generation too.
- The TS mirror and `GridInfo` literals in the tests change with it.

**Scanner** (`scanner.rs`, `watcher/policy.rs`, `watch.rs`):
- **photon's own INI writes echo back as subtree scans.** Starring across more than 8 folders overflows the pending queue, and `insert_pending` then collapses it into a full rescan of the root. The same happens when Picasa renames an album, which rewrites every member folder's INI.
- Ideas for that:
  - suppress watcher events for an INI photon just wrote, while its size, mtime and inode still match what photon wrote;
  - collapse to the lowest common ancestor instead of the root.
- **`describe()` runs serially.** On a network share even header reads are latency-bound. 2–4 describe threads per batch would help there, but on a single HDD they would not.
- **Degraded roots are rescanned every 5 minutes** (`DEGRADED_RESCAN`) however long a full scan of that root takes. Wait at least 10× the root's last scan duration.
- **On Linux, notify's inotify registration follows symlinks and stats every entry.** That walk costs minutes on CIFS or NFS. `with_follow_symlinks(false)` would match the scanner.
- **`keywords::read_embedded` opens the file a second time** to read a 256 KiB prefix, and `xmp.rs` lossily decodes that whole prefix before searching it. Read one prefix and share it with `read_header`. Find `</x:xmpmeta>` in the raw bytes before decoding.

**Engine:**
- **Two IPC commands can hold an async IPC worker for a long time.**
  - `export_items` holds one for the whole export.
  - `check_export_dest` calls `paths::canonicalize`, which can hang on a dead mount.

  Both should run inside `spawn_blocking`, as `add_folder` already does.
- **The watcher starts late.** `startup` runs `wait_for_scans()` before `start_watcher()`, and a scan keeps its slot through `hash_after_scan`. So the watcher waits for the first look-alike pass of the session. Files changed in that window aren't seen until something rescans their directory. Running `hash_after_scan` on its own thread after the scan slot is released would fix it.

**SQL** (a schema bump, so a minor release; see CLAUDE.md on version tripwires):
- **Starred and Videos walk the whole library twice** to return 2–5% of it: about 135 ms at 300k, against 25 ms for Hidden. Reshape `items_starred` to `(folder_id, taken_at) WHERE rating >= 1 AND missing_since IS NULL`, and add the same for videos.
- **The All view's grid driver reads every row from the table.** A covering partial index on `(folder_id, taken_at) WHERE missing_since IS NULL AND hidden = 0` measured 65 → 19 ms for the driver.
- **`map_grid_row` allocates a path `String` and a Vec per row** to compute the fingerprint. The thumbnail key must stay byte-identical, since it names cached files.

**UI:**
- **`itemsInRect` (`layout.ts`) walks every row** on each band pointermove and autoscroll frame. Start it at `rowIndexAt(rows, top)` and stop once past `bottom`.
- **Tile badges use `filter: drop-shadow`** (`Tile.svelte`). On WebKit that may give each badge its own compositing layer. This is low confidence; check it with `WEBKIT_SHOW_COMPOSITING_DEBUG_VISUALS=1`.

**Thumbnails:**
- **Every photo gets a 1600 px preview cached.** At 150–300 KB each, that is 45–90 GB for 300k photos. One design option is grid thumbnails first on first launch, with previews made lazily when the viewer asks.

## 8. Measured and rejected: don't redo these

| Idea | Result | Where |
|---|---|---|
| `[profile.dev.package."*"] opt-level = 2` | photon-core tests 11.1 s → 10.5 s. The fixtures are tiny by design. | PR #115 description |
| Release LTO and `codegen-units = 1` | No gain. | commit 8857216 |
| `ANALYZE`, `PRAGMA optimize`, bigger `cache_size`/`mmap_size` | Under 5% once the plans were fixed. Statistics would also re-plan every query, and the plan tests run without them. | `library/mod.rs` module doc |
| Lazy-loading the Viewer, Settings or Compare bundles | About 199 KB of JS served locally; parse savings are a few ms. | |
| Parallel directory listing (jwalk) | `skip_mark_purge` depends on walkdir's error behaviour for errors with no path. | |
| Skipping unchanged directories by their mtime | An in-place edit doesn't change the directory's mtime. | |
| Caching parsed INIs by size and mtime | FAT's 2-second mtime resolution makes it unsafe. | |
| `DynamicImage::thumbnail` for the preview shrink | 77–106 ms, no better than the resize it would replace. `fast_image_resize` is 7 ms. | commit for #116 item 11 |

## 9. Release and workflow notes

- **Release tags are unsigned lightweight tags,** as every earlier tag is. The development machine's global git config sets `tag.gpgsign = true`, which makes a plain `git tag` fail with "no tag message". Use `git tag --no-sign vX.Y.Z`.
- **`malformed_requests_leave_the_server_serving` was flaky on macOS CI** (connection reset). It was fixed in #116 by accepting a reset as the server closing the connection. If macOS CI fails there again, that's where to look.
- **Measurement fixtures lived in a scratchpad and are gone:**
  - a 300k-photo synthetic `library.db` built from `MIGRATIONS`, used for `EXPLAIN QUERY PLAN` and timings;
  - the headless-Chromium scroll harness described in section 2.

  Rebuild them from those descriptions. The grid benchmark (`benches/grid.rs`) now uses varied file sizes and includes `search_100k`. `benches/render.rs` times a thumbnail's decode, resize and edit render.
