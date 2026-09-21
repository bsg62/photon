# Grid thumbnail size

2026-09-21

The grid draws 160 px tiles because `layout.ts` says `export const TILE = 160`. Picasa had a
slider, and the reason it did is that the right size is not a property of the software: it is
a property of the screen, the library and what you are doing — scanning a year for one photo
wants small tiles, choosing between four portraits wants large ones.

## Decisions taken before the design

- **Three or four discrete steps, not a continuous slider.** Discrete widths keep the layout
  arithmetic exact and keep the number of render targets finite. The control appears both in
  Settings and in the top bar, because it is a thing you change while looking at the grid.

## The sizes

| Step | Tile | Notes |
|---|---|---|
| Small | 120 | roughly half again as many photos per row |
| Medium | 160 | today's grid, and the default |
| Large | 224 | |

**Every step stays at or below 256 px, and that is the constraint, not a coincidence.**
`ThumbSize::Grid` renders a 256 px maximum edge, and its cache directory is named `grid`. A
fourth, larger step would mean raising `Grid::max_edge()` — and because the cache key is the
photo's fingerprint while the *directory* name is what separates the sizes, every already
cached 256 px file would be silently reused at the new size. Photos indexed before the change
would stay soft forever, with nothing to indicate why.

So: a "Huge" step is possible, but it is a **new `ThumbSize` variant with its own directory**,
rendered lazily, not an edit to the existing one. It is deliberately not in this design.

Worth recording while we are here: on a 2× HiDPI screen today's 160 px tile already asks for
320 device pixels from a 256 px thumbnail, so grid tiles are *already* marginally soft there.
This design does not change that and does not fix it; it is noted so the next reader does not
attribute it to the size control.

## What changes in the layout

`TILE` stops being a module constant and becomes an argument. The pure functions in
`layout.ts` that read it or `TILE_ROW` — `columnsFor`, `buildRows`, `itemsInRect` — each take
the tile size, and `TILE_ROW` becomes `tileRow(tile)`. `HEADER` and `GAP` do not change:
section headers are text and the gutter is a gutter.

`itemsInRect` is the one to be careful with. Its comment explains that a tile spans
`top..top + TILE` rather than the full row, so a band drawn in the gap below a row selects
nothing, and that `firstColumn` measures from tile *right* edges while `lastColumn` measures
from left edges. All of that is expressed in terms of `TILE` and `TILE_ROW`; threading a
parameter through must not disturb the relationship, and the existing band tests are what
prove it did not — they run at the default size, so **each one gains a Small and a Large
case**, or the parameterisation is pinned by nothing. That is the lesson from the band's six
green probes: a probe only proves the rule for the inputs the tests actually use.

## Keeping your place across a size change

Changing the tile size changes every row's `top`. The grid index does not move, so this is
not the "an offset is only meaningful against one index version" hazard — but the *visual*
consequence is the same one a person notices: the scroll position now points somewhere else,
and a grid that jumps to a different year when you make the tiles bigger is worse than no
control at all.

Before the change the grid reads the offset of the first item in the top visible row; after
it, it re-derives that row's `top` from the new layout and scrolls there. `rowOfItem` already
answers exactly this question. The pinned photo is the same one `topFolderId` follows, so
what photon remembers across a restart stays consistent with what you were looking at.

## Where the choice lives

The `settings` table, key `grid_tile`, holding the step name — not the pixel width, so a
future change to what "Large" means does not have to migrate anyone's setting. This is the
same reasoning that puts `last_folder`, `theme` and `slideshow_interval_s` there rather than
in `localStorage`: the webview's storage sits in the profile directory, dies with a cache
clear, and cannot be reached from a test.

No schema change: `settings` is key/value TEXT and already exists.

IPC follows `slideshowInterval` exactly — `grid_tile`/`set_grid_tile` in `commands.rs`, the
delegating wrappers in `ipc.rs`, two entries in `generate_handler!`, the hand-written mirror
in `api.ts` (nothing validates it; a Rust field without its TS counterpart is silently
`undefined`), and an answer in `screenshots/mock.js` or the test in `screenshots.rs` fails.

Unlike the theme, this needs no `localStorage` mirror and no boot script. A grid at the wrong
tile size for one frame is a reflow; a UI in the wrong theme before first paint is a flash of
the wrong colours, which is why only the theme pays for that machinery.

## The control

A three-way segmented control in the top bar, and the same choice in Settings under
**Appearance**, beside the theme. Both write through one `createGridSize` store so they cannot
disagree — the same generation-counted pattern `createTheme` uses, because a singleton
outlives an App remount.

The segmented control's items carry `tabindex="-1"` as a roving-tabindex widget, which means
the global `[tabindex='-1']:focus-visible { outline: none }` rule would strip their focus
ring. That rule exists for script-focused containers. **Scope it before adding this control**,
or the keyboard user gets no indication of where they are — CLAUDE.md names this trap
specifically and this is the first widget to walk into it.

## Tests

- `columns_for_each_size` — the column count at a fixed width differs per step. Fails if the
  parameter is threaded but ignored, which is the plausible bug.
- `rubber_band_at_small_and_large` — the existing `itemsInRect` cases re-run at 120 and 224,
  including the range-merge case. Fails if `firstColumn`/`lastColumn` keep a hardcoded 160.
- `row_of_item_survives_a_size_change` — the pinned offset lands in a tile row at the new
  size. Fails if the re-pin is dropped.
- `the_setting_round_trips` — Rust-side, a stored step reads back, and an unknown value falls
  back to Medium rather than erroring, the way `ThemeChoice::from_db` treats an unknown theme.

Whether 120 and 224 are the right numbers cannot be tested and is a looking question: it goes
on the README's smoke checklist, and `xtask screenshots` gains a `grid-small` and a
`grid-large` shot so the answer can be seen without launching the app.

## Not in this design

A continuous slider. A fourth size above 256 px (see above — it needs its own cache
directory). Any per-view memory of size; the choice is one setting for the whole app.
