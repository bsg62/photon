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
