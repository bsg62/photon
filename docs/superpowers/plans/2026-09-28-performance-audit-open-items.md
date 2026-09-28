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

## 3. Hide and selection edge cases — done

Fixed in the PR after #118 (`ui/src/lib/library.svelte.ts`): a band released while a hide is in flight strips what the hide took (`bandsResolving`), and a hide lands after the selected *run* the lead is in, from offset runs the store keeps beside the selection against one index version. Not the selection's lowest offset, which the earlier note suggested: a photo Ctrl+clicked in far above is a run of its own.

## 4. Writers that report a failed rebuild as a failed write — done

Fixed in the same PR: every committed write rebuilds through `Engine::refresh_after_write`, which logs a failed rebuild. That covers the nine writers listed here plus `set_star` and `remove_item_tag`, which ended in a bare `refresh_grid()` and were missed by a search for `refresh_grid()?`.

## 5. Smaller ideas the audit raised but nothing has picked up

- **Keyword order in search text.** In the search haystack, keywords now follow the query's row order (`search_tags` in `library/items.rs`). This only matters for a quoted phrase spanning two keywords. `ORDER BY e.item_id, e.tag` would make it deterministic again, at the cost of a sort over the keyword rows.
- **The JPEG decode is now the largest stage of a thumbnail,** about 130 ms of about 140 ms at 24 MP. zune-jpeg has no DCT scaling. `jpeg-decoder`'s `scale()` could decode at 1/2 or 1/4 for previews. Benchmark it first: its entropy decoder is slower, so 1/2 may be a wash.
- **Duplicate candidates are chosen by byte size alone,** so roughly 5–10% of a large library shares a size by coincidence and gets read in full once. A first-64-KiB hash as a pre-filter would cut that read. It needs a schema column, which makes it a minor release.
- **Thumbnail workers** stay capped at 8 (`MAX_WORKERS`, `thumbs/service.rs`). After #116 each worker needs about half the memory, so a RAM-derived cap is possible. The cap was also about the disk, so measure before raising it.

## 6. The grid canvas exceeds the browser's layout-height limit — confirmed, not fixed

Measured on 2026-09-28. Engines cap a box at (2^31−1)/64 = 33,554,428 px; `.canvas` is capped there, every row whose `top` lies beyond lands on the cap, and `scrollTop` stops at it. Everything past it is unreachable by scrolling, End, a folder jump or the timeline; the viewer still works.

- **Chromium (WebView2) applies the cap in device pixels**: 22,369,620 CSS px at 150% scaling, 16,777,214 at 200%. WebKitGTK measured 33,554,428 (its behaviour under scaling is likely, not proven, to be the same).
- **The worst width** is the 800 px minimum window with the sidebar at its maximum (half the window): `clientWidth` 336, one column at large and medium tiles.
- **Photos needed to cross the cap**, at ~15 photos per folder:

| Case | 33.55M | 22.37M (150%) | 16.78M (200%) |
|---|---|---|---|
| Large, 1 column | 142k | 95k | 71k |
| Medium, 1 column | 195k | 130k | 98k |
| Large, 2 columns (800 window, default sidebar) | 263k | 176k | 132k |
| Large, 4 columns (1280 window) | 512k | 341k | 256k |
| One photo per folder, any width | 116.5k folders | 77.7k | 58k |

- **Confirmed on the built UI** in headless Chromium with a 300k-photo / 20k-folder mock: at 800 px, large, 2 columns, End stops at offset 263,244; at 1 column only offsets up to 142,341 are reachable.

**Fix sketch.** Keep row geometry virtual; the DOM canvas is `min(total, MAX_CANVAS)` with `MAX_CANVAS` under the smallest cap across scales (a constant of 8–10M, or measured at runtime from a 1e9 px probe and re-measured on a resolution change). Libraries under it are unchanged. Scrolling is hybrid, not proportional: relative input (wheel, keys, touch, band autoscroll) moves a `virtualTop` 1:1, scrollbar drags and jumps map proportionally, and the ends snap. Rows and headers are drawn at `row.top − shift` (not a wrapper `translateY`, whose children's `top` would still hit the cap). All reads and writes of `scrollTop` are in `Grid.svelte` (`onscroll`, `scrollToOffset`, the tile-size pin restore, `atCanvas`, band autoscroll, the timeline's `onscrub`); `renderRange`, `itemsInRect` and `nav.ts` are unchanged. The hard part is telling a scrollbar drag from wheel and kinetic scrolling on three webviews, which no test here can cover: design it before building it.

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
- ~~**Degraded roots are rescanned every 5 minutes** (`DEGRADED_RESCAN`) however long a full scan of that root takes.~~ Done: a degraded root now waits `max(DEGRADED_RESCAN, 10 × its last full scan)` from the later of its last walk and the ticker's last ask (`watch.rs`, `degraded_rescan_due`; the time is `Engine::last_full_scan`, in memory, timed again by each session's startup scan). Registration is still retried every 5 minutes, and a root that registers is rescanned at once.
- **On Linux, notify's inotify registration follows symlinks and stats every entry.** That walk costs minutes on CIFS or NFS. — symlinks: done, `with_follow_symlinks(false)` (`watcher/fs.rs`), matching the scanner, so a linked tree is no longer walked or given watch descriptors. The stat is not: notify's `filter_dir` calls `metadata()` on every entry, files included, to find the directories, whatever the flag says (an `lstat` each instead of a `stat` on a link). Removing it needs notify to use walkdir's `file_type()`, which is free from `readdir` - an upstream change, or a fork.
- **`keywords::read_embedded` opens the file a second time** to read a 256 KiB prefix, and `xmp.rs` lossily decodes that whole prefix before searching it. Read one prefix and share it with `read_header`. Find `</x:xmpmeta>` in the raw bytes before decoding.

**Engine:** — done. `export_items`, `check_export_dest`, the five opener commands and `set_star`/`set_stars` run on the blocking pool (`ipc.rs`, tested through Tauri's mock runtime by parking one call per worker). A scan's duplicate and look-alike pass is now requested on its own thread after the scan slot is released, so `startup` starts the watcher without waiting for it; `shutdown` waits for passes by count (`similar_passes`). Starting the watcher *before* the startup scans was rejected: the watcher registers only roots marked online, which those scans settle. Still on the async workers, bounded but slow at 300k: the view setters' rebuilds, `list_tags`, `watched_folder_stats`, and bulk tag/hide/edit writes.

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
