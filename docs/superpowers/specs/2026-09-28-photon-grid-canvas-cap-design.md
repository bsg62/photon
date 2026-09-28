# Grid canvas cap: every photo reachable in a library taller than the browser allows

Date: 2026-09-28. Item 6 of `docs/superpowers/plans/2026-09-28-performance-audit-open-items.md`.

## The problem

The grid is one absolutely positioned canvas (`.canvas` in `Grid.svelte`) whose height is the
whole library's layout (`totalHeight(rows)`), with only the rows near the viewport mounted.
Layout engines cap a box at (2^31−1)/64 = 33,554,428 px. Past the cap `.canvas` stops growing,
every row whose `top` lies beyond it is laid out on the cap, and `scrollTop` stops there:
the rest of the library cannot be reached by scrolling, End, a folder jump or the timeline.
The viewer, being offset-based, still reaches every photo.

Chromium (so WebView2) applies the cap in *device* pixels: 22,369,620 CSS px at 150% display
scaling, 16,777,214 at 200%. WebKitGTK measured 33,554,428 at scale 1.

Measured on the built UI in headless Chromium with a 300k-photo / 20k-folder mock: at an
800 px window with large tiles (2 columns) End stops at offset 263,244; with 1 column
(the minimum window with the sidebar at its widest) only offsets up to 142,341 are reachable.
The threshold, at ~15 photos per folder, is ~142k photos at large tiles and 1 column at 100%
scaling, ~71k at 200%; one photo per folder crosses it at ~116k folders at any width. The
full table is in the plan's section 6.

## Decisions

- **Past the cap, the wheel stays 1:1 and the scrollbar thumb is approximate.** A DOM
  scroll range shorter than the layout cannot be both: 1:1 input and a proportional thumb.
  Wheel, trackpad, keys and touch keep their speed; the thumb drifts from the true position
  while scrolling and settles on it when scrolling stops. Dragging the thumb maps
  proportionally. The timeline strip, drawn from the virtual position, stays exact.
  Rejected: purely proportional mapping (the wheel scrolls up to ~5× too fast at the
  largest sizes) and hiding the native scrollbar for a custom one (a custom scrollbar to
  build and keep accessible, for a case most libraries never reach).
- **The cap is measured at runtime**, not a constant. A constant safe at 300% scaling
  (~8M px) would put Linux and macOS users into the approximate-thumb mode at ~34k photos,
  four times earlier than their engine needs.
- **Under the cap nothing changes.** The mapping is the identity, `shift` is 0, and no
  scroll position is ever written that is not written today.

## Design

### Two positions and one offset

- `total` — the layout's height (`totalHeight(rows)`), in *virtual* px, unchanged.
- `domMax` — 90% of the measured cap (below). `domHeight = min(total, domMax)` is what
  `.canvas` gets as `style:height`.
- The grid's existing `scrollTop` state becomes the **virtual** position. Everything that
  reads it today keeps reading it: `renderRange`, `fetchSpan`, `visibleRange`,
  `topFolderId`, the timeline, the speed sampler (`speed.sample`).
- `shift = virtual − domTop`. Every mounted row, header and the band rectangle is drawn at
  `style:top = row.top − shift`. Only the mounted rows (5–30) are affected; their `{#each}`
  keys stay virtual. Not a `translateY` on a wrapper: the children's untranslated `top`
  would still hit the cap.

### Reading a scroll event

Each `onscroll` takes the DOM delta `d = domTop − lastDomTop` and classifies it:

- **Relative** (the default): `virtual += d`, `shift` unchanged. Wheel, trackpad momentum,
  keyboard scrolling, touch pan, band autoscroll, and the browser's clamp when the canvas
  shrinks.
- **Proportional**: `virtual = domTop × (total − viewport) / (domHeight − viewport)`, `shift`
  recomputed. When a pointer went down in the scrollbar gutter and has not come up (the
  grid already tells a press on its scrollbar apart, `bandDown`'s `offsetX > clientWidth`
  test) or when `|d|` exceeds two viewports in one event (a track click, a thumb drag on an
  overlay scrollbar the gutter test cannot see).

`virtual` is clamped to `[0, total − viewport]` either way. A proportional move maps the
DOM's ends to the virtual ends exactly, so dragging the thumb to the bottom always lands on
the last row.

### Re-anchoring

Relative scrolling makes the thumb drift. When scrolling goes still (`Motion` becomes
`STILL`, `SCROLL_SETTLE_MS` after the last event) and `shift` is not the proportional
shift for the current `virtual`, the grid writes `domTop = proportional⁻¹(virtual)` and
sets `shift` to match. Nothing on screen moves - `virtual` is unchanged - only the thumb
jumps to where it belongs.

If a relative move reaches a DOM edge (`domTop` at 0 or at `domHeight − viewport`) while
`virtual` is not at the matching virtual edge, the grid re-anchors at once. That can cut a
momentum flick short; it takes a continuous scroll of millions of px without a pause.

### Writes from code

End, Home, a folder jump, a timeline scrub (`onscrub`), `scrollToOffset` and the tile-size
pin restore set `virtual` and write the matching proportional `domTop`, remembering the
value written. The scroll event that write causes, whose `domTop` equals it, is taken as
already applied rather than classified. Band autoscroll (`Grid.svelte`, `viewport.scrollTop
+= whole`) is a relative move and goes through the same path as the wheel.

`scrollToOffset`'s "is this row already visible" tests, the pin read
(`firstVisibleOffset`), and `atCanvas` (pointer y to canvas y) use `virtual`, not
`viewport.scrollTop`. Band rectangles therefore stay virtual and `itemsInRect` is
unchanged. Keyboard navigation (`nav.ts`) is row-based and ends in `scrollToOffset`.

### When sizes change

- **A rebuild changes `total`** (a scan, a view or sort switch, a resize changing columns).
  While `total` stays above `domMax` before and after, `domHeight` and `shift` are
  unchanged and the DOM position is not written: a flick under a scan continues. When
  `total` crosses `domMax` either way, the grid re-anchors. The pin restore re-finds its
  photo by offset and ends in a write from code.
- **The canvas shrinks under the viewport.** The browser clamps `scrollTop` synchronously
  (CLAUDE.md, "A shrinking scroll container clamps `scrollTop` for you"); the clamp arrives
  as a relative delta, and `virtual` is clamped. Under the cap that is today's behaviour.
- **`devicePixelRatio` changes** (another monitor, zoom): the cap is measured again through a
  `matchMedia('(resolution: …dppx)')` listener; if `domMax` falls below the current DOM
  position the grid re-anchors.

### Measuring the cap

On mount, an absolutely positioned, `visibility: hidden` box of `height: 1e9px` inside the
viewport; its `getBoundingClientRect().height` is the engine's cap in CSS px, and it is
removed at once. `domMax = floor(0.9 × measured)`. A reading of 0 or below 1M px (a window
not yet laid out) falls back to 8,000,000 and is measured again on the next resize. A
reading of 1e9 (no cap) is used as is: the mapping never leaves the identity.

### Where the code lives

- **`ui/src/lib/scroll-map.ts`** (new, pure): `capFrom(measured)`, and a map holding
  `{ total, viewport, domMax, shift, lastDomTop, expected }` with `onScroll(domTop,
  pointerInGutter) → virtual`, `setVirtual(v) → domTop to write`, `moveBy(d)`,
  `resize(total, viewport)`, `settle() → domTop to write | null`. No DOM access.
- **`Grid.svelte`**: the measure, the gutter pointer flag, and wiring `onscroll`,
  `scrollToOffset`, the pin restore, `atCanvas`, band autoscroll, `onscrub` and the settle
  effect through the map; `style:top` minus `shift`; `.canvas` height `domHeight`.
- Unchanged: `layout.ts` (`buildRows`, `renderRange`, `itemsInRect`, …), `Timeline.svelte`,
  `timeline.ts`, `nav.ts`, the backend.

## Testing

- **`scroll-map.test.ts`** (vitest, pure):
  - identity under the cap: every function returns its input, `shift` stays 0, `settle`
    returns null;
  - relative moves keep `shift` and clamp;
  - proportional moves map DOM ends to virtual ends exactly;
  - the classifier: gutter pointer → proportional, a jump over two viewports →
    proportional, anything else → relative;
  - re-anchoring on settle, and at a DOM edge reached before the virtual one;
  - `total` crossing `domMax` in both directions; a rebuild above it keeps `shift`;
  - the echo of a write from code is not re-applied;
  - `capFrom`: the 90% margin, the fallback below 1M px, 1e9 left as is.

  Each rule gets a revert probe (CLAUDE.md).
- **`cargo run -p xtask -- scroll-probe`** (new; like `screenshots`, needs Chromium, not in
  CI): serves the built UI with `mock.js` answering a 300k-photo / 20k-folder library, and
  asserts, reading the DOM through `--dump-dom` and a load script:
  - 1 column, large tiles: End mounts the last offset, and so does a scrollbar-sized jump
    to the bottom of the DOM range (a timeline scrub goes through the same write-from-code
    path as End, and is on the smoke checklist);
  - a library under the cap: `shift` is 0 throughout and `.canvas` height equals `total`;
  - `--force-device-scale-factor=2`: the measured cap is ~16.8M px and End still reaches
    the last offset.
- **README smoke checklist**, per webview (WebKitGTK, WKWebView, WebView2; on Windows at
  100%, 150% and 200% scaling):
  - a normal library feels unchanged;
  - a library over the cap in a narrow window: wheel and trackpad at normal speed;
    dragging the thumb moves proportionally and reaches both ends; the thumb settles after
    a pause without the photos moving; End, Home, a folder jump and a timeline scrub land
    where they should; a rubber band autoscrolled across a re-anchor selects what it drew.

## Out of scope

- The sidebar's folder list: a separate list, not measured near the cap.
- Horizontal scrolling: the grid has none.
- Recording the cap anywhere: it is measured per session and per scale.

## Risks

- **Telling a thumb drag from kinetic scrolling** rests on the gutter press and the
  two-viewport jump. An engine that scrolls a thumb drag in small steps on an overlay
  scrollbar (no gutter press) would treat it as relative: the content follows the thumb
  1:1, slower than the thumb suggests, and the settle re-anchor puts the thumb right
  afterwards. Degraded, not broken; the smoke checklist covers each webview.
- **Writing `scrollTop` on settle** could interrupt an engine's own smooth-scroll animation
  if `STILL` arrives while one is still running. `SCROLL_SETTLE_MS` is measured from the
  last scroll *event*, which an animation keeps sending, so it should not; the smoke
  checklist watches for it.
