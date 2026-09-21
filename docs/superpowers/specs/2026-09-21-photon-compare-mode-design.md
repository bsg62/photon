# Compare mode

2026-09-21

Picking the keeper out of a burst is the one thing the viewer cannot help with. You can step
between three frames of the same shot, but you cannot *see* them together, and the difference
that decides it — which one is sharp, whose eyes are open — is exactly the difference that
does not survive a second of memory between two keypresses.

Compare mode puts two to four photos side by side, at the same zoom, over the same point.

## Decisions taken before the design

- **Entered from the grid selection**, 2 to 4 photos. Multi-select already exists and is the
  natural way a person says "these ones". Not the viewer's pin-and-hold model, which can only
  ever compare two.

## What a person sees

With 2–4 tiles selected, **`C`** in the grid — or **Compare** in the tile right-click menu —
opens a dark full-window surface holding one pane per selected photo: two side by side,
three or four as a 2×2. Each pane shows its photo fitted, with the file name and, where they
differ between the panes, the fact that decides it: pixel dimensions, and capture time when
the photos were taken at different moments.

**Zoom and pan are shared.** Scrolling zooms every pane about the same relative point, and
dragging pans them together. That is the whole feature: two photos at 200% on the same eye is
the comparison, and two photos each at their own zoom is not.

One pane is focused (a ring, `1`–`4` or Tab to move it). On the focused pane: `S` stars, `Enter`
opens it in the viewer and closes compare, `Delete` — nothing. photon does not delete photos.
`Escape` closes compare and returns to the grid with the selection intact.

A photo that carries an edit is shown **as edited**, like everywhere else.

## Where it lives, and why that is the whole risk

Compare mode is an overlay, so it renders in `App.svelte` beside `Viewer` and `Settings`, and
it counts towards `covered`. This is not a style preference. The grid is inside `<main>`, and
`covered` makes `<main>` inert: an overlay mounted inside the grid component would be made
inert **by its own opening** — Tab would walk out of it into the tiles, and Settings could be
opened on top of it.

Three consequences, each already paid for elsewhere in this app and each easy to forget:

- `covered` gains `compareAt !== null`.
- Closing hands focus back to the grid's viewport **after `await tick()`**, because `<main>`
  is inert until the DOM catches up and focusing an inert element silently does nothing. The
  grid's keys live on its viewport; a close that lands on `<body>` leaves the arrow keys,
  Enter and Escape dead until the user clicks.
- The pan drag has **three endings**, not two: `pointerup` finishes it, `Escape` abandons it,
  and a touchscreen pan sends **`pointercancel`** and nothing else. One `endPan` reached by
  all three, wired the way `App.svelte`'s splitter and the grid's rubber band are.

## What each pane loads

Not `/image/<id>`. Full-size renders run one at a time behind `RENDERING` in `protocol.rs`,
so four panes would be four serial full renders before anything appeared — and at fit, a
24 MP render is thrown away to draw 600 px of it.

A pane loads the **1600 px preview thumbnail** by `thumbKey`, which is already cached for any
photo whose thumbnail has been rendered, and which already accounts for the photo's edit. It
upgrades to `/image/<id>` only for the focused pane and only once zoom passes 100%, where
preview pixels would actually show. The upgrade is per pane, so the serialised renderer is
asked for one image at a time, which is what it is for.

`neighbours` is not involved and should not be: compare shows exactly what was selected.

## The state, and how it gets tested

There is no component test harness here, so the logic goes in `ui/src/lib/compare.svelte.ts`
as `createCompare`, the same shape as `createSlideshow` and `createCropTool`, and the
component keeps only effect wiring:

```
createCompare({ ids, onclose })
  panes: { id, src, loaded }[]      focus: number
  zoom: number                      pan: { x, y }
  zoomAt(factor, originX, originY)  // shared origin, clamped
  panBy(dx, dy)                     // clamped per current zoom
  focusPane(i) / nextPane()
  needsFullImage(i): boolean        // focused && zoom > 1
```

Pure and synchronous, so vitest covers it with fake timers. The tests that discriminate:

- `zoom_is_shared_across_panes` — zooming reads one value for every pane. Fails the moment
  zoom is stored per pane, which is the shape the feature would drift into.
- `zoom_about_a_point_keeps_that_point_still` — the arithmetic that makes shared zoom useful.
- `only_the_focused_pane_asks_for_a_full_render` — `needsFullImage` is true for at most one
  pane. Fails if the upgrade rule is dropped, which is the change that would quietly serialise
  four 24 MP renders.
- `escape_and_pointercancel_reach_the_same_teardown` — both leave no live pointer id. This is
  the bug class CLAUDE.md records from the rubber band, written down before it happens again.
- `opening_with_one_or_five_is_refused` — the grid offers Compare only for 2–4.

Pane geometry (2 across, 2×2, the gutters) is measurable without the GUI by the headless
Chromium layout probe: a static page holding the component's CSS, `--dump-dom`, and a load
script writing `getBoundingClientRect()` into `document.title`. Everything else — that it
*looks* right, that the shared pan feels shared — goes on the README's smoke checklist, and a
`compare-light`/`compare-dark` pair joins `SHOTS` in `xtask screenshots` with an action in
`mock.js` to select three tiles and press `C`.

## What it touches

No Rust, no schema, no new IPC. The backend already serves every pixel this needs. That is
the argument for building it: it is a new way of looking at what photon already has.

`no-literals.test.ts` applies — the panes' ground is the viewer's black, which is the single
colour literal that test allows **once, only in the viewer**. Compare therefore takes its
ground from the same token the viewer's root sets via `data-theme="dark"`, not a second
literal. A new `--compare-*` token must be declared in `tokens.css` or the test fails on it.

## Not in this design

Difference blending or an onion-skin overlay (a different feature, and one that needs the two
photos aligned first). Comparing more than four. Any "keeper" concept — photon has stars and
albums and does not need a third kind of mark. Rating or deleting from compare.
