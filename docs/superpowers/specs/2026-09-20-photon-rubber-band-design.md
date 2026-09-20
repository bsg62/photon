# Rubber-band selection

## Why

Multi-select (v0.18.0) gave the grid Ctrl/Cmd+click, Shift+click and Ctrl/Cmd+A. The gesture
it never got is the one people reach for first in a grid of pictures: press on the background
and drag a box over the photos you want.

## What the user does

Press the left button anywhere in the grid - on a tile or on the space between them - and
drag. A rectangle follows the pointer, every tile it touches rings, and releasing keeps them
selected. Holding **Ctrl/Cmd or Shift** adds to what was already selected instead of replacing
it. **Escape** during the drag abandons it and puts the previous selection back.

A press that does not move is still a click: the band only starts after the pointer has moved
past a small threshold, and the click it would otherwise have been is swallowed so a drag
ending on a tile does not also select that one tile.

The wheel still scrolls during a drag, and the band keeps its grip on the photos it was
started over, because the rectangle is held in the canvas's coordinates rather than the
window's.

## What it does not do

**No autoscroll.** Dragging to the edge does not scroll the grid, so a band covers what is on
screen (the wheel is the way to reach more). This is what keeps the feature honest about ids:
see below.

**No drag and drop.** Nothing in photon moves a photo, so a press on a tile has no other
meaning to compete with; that is why the band may start there rather than only on the
background.

## The id problem, and why the band is answered twice

A selection is a set of photo **ids**, not offsets (v0.18.0's lesson: an offset means "this
photo" only against one index version). The band, though, is geometry: it names *offsets*.

- **While dragging**, the covered offsets are resolved through the loaded pages -
  synchronous, instant, and correct for everything on screen. A tile whose page has not
  arrived yet resolves to nothing and is skipped, the same silent rule `toggleSelected`
  already follows for a Ctrl+click on a placeholder.
- **On release**, the same ranges are re-resolved through `fetchIds`, which asks the backend
  and is the authority. It is what makes a band that crossed a placeholder come out right.

Two paths, deliberately: the preview is for the eye and the release is for the truth. If
autoscroll is ever added, the preview's "everything is on screen" assumption goes with it and
the preview would need the backend too.

## The geometry is pure

`itemsInRect(rows, columns, rect)` in `lib/layout.ts` turns a rectangle in canvas coordinates
into **contiguous offset ranges, one per row it crosses**, merging rows that join up. Ranges
rather than a set of offsets because that is what `fetchIds` already takes, and because a band
over a full row is one range whatever the column count.

Tiles are `TILE` wide at `GAP` intervals inside a row of `TILE_ROW`, so a row's vertical band
is `top..top + TILE` (not `+ TILE_ROW`: the gap below a row belongs to no tile, and a band
that only grazes the gap must select nothing). Headers are skipped.

This is where the tests are: the component holds the pointer wiring, which `svelte-check` and
the smoke checklist cover, as everywhere else in this project.

## Store

`beginBand(additive)` captures the selection to build on, `bandTo(ranges)` previews,
`endBand(ranges)` resolves through `fetchIds` and is the authority, `cancelBand()` puts the
captured selection back. The lead and the anchor land on the band's first covered offset, so
Enter opens something inside the band and a later Shift+click extends from it.

## Tests

- `layout.test.ts`: a rectangle inside one row; a band spanning rows merging into one range;
  a band that touches only the gap between two rows; a band past the last tile of a short row;
  a band over a header; a zero-size rectangle; a band wider than the grid.
- `library.test.ts`: a preview replaces or adds according to `additive`; `cancelBand` restores;
  `endBand` overrides a preview that missed a placeholder; a band whose grid version changed
  mid-flight writes nothing.
