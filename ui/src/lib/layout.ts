/** Grid geometry. Square tiles in fixed-height rows, with one header row per section that
 *  has one - a folder's or a period's. Everything here is pure, so 100k items lay out in microseconds. */

import type { Period, Section } from './api';
import type { Motion } from './scroll-speed.svelte';
import { TILE_SETTLE_MS } from './thumb-request.svelte';

export type TileSize = 'small' | 'medium' | 'large';

/** How wide a tile is at each step, in CSS pixels, before it is widened to fill its row
 *  (`tileFor`, which stops at `TILE_MAX`).
 *
 *  Every step is at or below 256, which is `ThumbSize::Grid`'s maximum edge. The cache key
 *  is the photo's fingerprint while the cache *directory* is what separates the sizes, so
 *  raising that maximum without renaming the directory would silently serve already-cached
 *  256px files at the new size - soft forever, with nothing to say why. A step above 256
 *  needs its own `ThumbSize` variant, not a larger `Grid`. */
export const TILE_WIDTH: Record<TileSize, number> = { small: 120, medium: 160, large: 224 };

export const GAP = 8;
export const HEADER = 32;
/** Extra space above every section's header but the first, on top of the gutter under the
 *  last row before it, so one folder reads as ending before the next begins. Space between
 *  rows rather than part of the header row: a jump to a folder puts its header, not this
 *  gap, at the top of the grid. */
export const SECTION_GAP = 24;

/** A tile row's full height: the tile plus the gutter under it. */
export function tileRow(tile: number): number {
  return tile + GAP;
}

export interface SectionLike {
  /** Null for a run drawn under no folder: a period's, or a flat view's. */
  folderId: number | null;
  /** The day, month or year a run is drawn under, when the grid is grouped by one. */
  period?: Period | null;
  offset: number;
  count: number;
}

export interface Row {
  kind: 'header' | 'tiles';
  /** Index into the sections array. */
  section: number;
  /** Grid offset of the first item (the section offset for headers). */
  first: number;
  /** Items in this row (0 for headers). */
  count: number;
  top: number;
  height: number;
}

export function columnsFor(width: number, tile: number): number {
  return Math.max(1, Math.floor((width + GAP) / tileRow(tile)));
}

/** The widest a tile is drawn: an eighth past `ThumbSize::Grid`'s 256px edge, which is how
 *  much of an enlargement goes unseen. Only Large reaches it, with three columns or fewer;
 *  there the row keeps a gutter. */
export const TILE_MAX = 288;

/** How wide a tile is drawn in a row `width` pixels wide: the size chosen (`TILE_WIDTH`),
 *  widened so the columns that fit fill the row instead of leaving a gutter at its end.
 *
 *  The column count is the chosen size's, so a tile only ever grows, by less than one
 *  column's share. A whole number of pixels: every row's `top` is a multiple of the tile's
 *  height, and a fractional one would put each row's tiles on a different part of a pixel.
 *  Never narrower than the size chosen, which a row too narrow for one tile overflows as
 *  it always has. */
export function tileFor(width: number, nominal: number): number {
  const columns = columnsFor(width, nominal);
  const share = Math.floor((width - (columns - 1) * GAP) / columns);
  return Math.max(nominal, Math.min(share, TILE_MAX));
}

/** A section gets a header row when it names a folder or a period; a flat view's run names
 *  neither. */
export function hasHeader(section: SectionLike): boolean {
  return section.folderId !== null || (section.period ?? null) !== null;
}

export function buildRows(sections: SectionLike[], columns: number, tile: number): Row[] {
  const rows: Row[] = [];
  let top = 0;
  sections.forEach((s, section) => {
    if (hasHeader(s)) {
      if (rows.length > 0) top += SECTION_GAP;
      rows.push({ kind: 'header', section, first: s.offset, count: 0, top, height: HEADER });
      top += HEADER;
    }
    const end = s.offset + s.count;
    for (let first = s.offset; first < end; first += columns) {
      rows.push({ kind: 'tiles', section, first, count: Math.min(columns, end - first), top, height: tileRow(tile) });
      top += tileRow(tile);
    }
  });
  return rows;
}

/** How tall `buildRows` lays these sections out, without building a row: a sum over the
 *  sections, for a question asked of a layout that is not the one on screen. */
export function layoutHeight(sections: SectionLike[], columns: number, tile: number): number {
  let top = 0;
  let any = false;
  for (const s of sections) {
    if (hasHeader(s)) {
      if (any) top += SECTION_GAP;
      top += HEADER;
      any = true;
    }
    const rows = Math.ceil(s.count / columns);
    if (rows > 0) any = true;
    top += rows * tileRow(tile);
  }
  return top;
}

/** The width of a row of tiles in a viewport `viewport` wide: what is left between the
 *  gutter down either side. */
export function rowWidth(viewport: number): number {
  return Math.max(0, viewport - 2 * GAP);
}

/** The year strip's width beside the grid (`Timeline.svelte`). */
export const TIMELINE_WIDTH = 44;

/** Whether the year strip is drawn beside the grid: with more than one year to choose
 *  between, and something to scroll.
 *
 *  "Something to scroll" is asked of the grid *as it would be beside the strip* - `outer`
 *  is the width the grid and the strip share, `gutter` what a scrollbar takes of it - and
 *  not of the grid as it stands. The strip takes width from the tiles, and tiles that fill
 *  the row are shorter for it, so there is a band of heights where the grid fits beside the
 *  strip and overflows without it: asked of the grid as it stood, the answer changed the
 *  grid, which changed the answer, and the strip came and went on every frame. In that band
 *  there is no strip, and a grid that scrolls by a few pixels without one. */
export function showsTimeline(
  sections: SectionLike[],
  years: number,
  outer: number,
  gutter: number,
  viewport: number,
  nominal: number,
): boolean {
  if (years < 2) return false;
  const row = rowWidth(outer - TIMELINE_WIDTH - gutter);
  return layoutHeight(sections, columnsFor(row, nominal), tileFor(row, nominal)) > viewport;
}

export function totalHeight(rows: Row[]): number {
  const last = rows[rows.length - 1];
  return last ? last.top + last.height : 0;
}

/** Index of the last item whose `key` is at or below `value`, or 0 if there is none.
 *
 *  Shared with `nav.ts`'s section lookup, which is the same search over a different field:
 *  an upper-bound binary search is easy to get subtly wrong, and only one of the two copies
 *  was covered by tests. */
export function lastIndexAtOrBefore<T>(items: T[], value: number, key: (item: T) => number): number {
  let lo = 0;
  let hi = items.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (key(items[mid]) <= value) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** Index of the last row whose top is at or below `y` (the row containing `y`). */
export function rowIndexAt(rows: Row[], y: number): number {
  return lastIndexAtOrBefore(rows, y, (r) => r.top);
}

/** Rows intersecting `[scrollTop - overscan, scrollTop + viewport + overscan)`, as `[start, end)`. */
export function visibleRange(rows: Row[], scrollTop: number, viewport: number, overscan: number): [number, number] {
  if (rows.length === 0) return [0, 0];
  const start = rowIndexAt(rows, scrollTop - overscan);
  const end = rowIndexAt(rows, scrollTop + viewport + overscan) + 1;
  return [start, Math.min(end, rows.length)];
}

/** How far past the viewport tiles are mounted while the grid is still, in viewports, either
 *  side: enough that a wheel notch or an arrow key lands on rows already in the DOM, no
 *  more. Every mounted tile carries its own deriveds and effects, and rows are keyed by
 *  position, so a jump (End, a folder jump, a scrollbar drag, a timeline scrub) mounts the
 *  whole window again: at two viewports either side that was five screens of tiles per
 *  jump. */
export const RENDER_OVERSCAN = 0.5;
/** While scrolling, how far ahead of the direction of travel tiles are mounted, as time: a
 *  tile mounted this long before it comes into view has had a round trip for its page, a
 *  request and a decode before anyone sees it. Scaled by the speed, so the lead covers the
 *  same time at any speed rather than the same distance, which a flick covers in a frame
 *  or two. */
export const LEAD_MS = 300;
/** The most the lead grows to, in viewports. Mounting it costs once, when the scroll starts
 *  or speeds up; after that a continuous scroll mounts only the rows it moves by, which at
 *  any speed short of a jump is a row or two a frame. */
export const LEAD_OVERSCAN_MAX = 1.5;
/** What stays mounted behind a scroll, in viewports: a small reversal lands on rows still in
 *  the DOM, and a real one grows its own lead. */
export const TRAIL_OVERSCAN = 0.25;
/** How far past the viewport pages are fetched, in viewports. Wider than what is rendered
 *  on purpose: a page is cheap to hold and costs a round trip to miss, and it is also the
 *  range `LibraryStore.refresh` prefetches before it swaps a rebuilt grid in. It has to
 *  reach past `LEAD_OVERSCAN_MAX`, or the lead mounts tiles with no photo to show yet. */
export const FETCH_OVERSCAN = 2;

/** How far past the viewport to mount tiles, in pixels, as `[above, below]`.
 *
 *  A jump mounts only what is on screen: it shares no rows with the render before it, so
 *  every row of overscan would be mounted fresh - and during a drag unmounted again a frame
 *  later. That is the only time the overscan goes. A continuous scroll at any speed mounts
 *  only the rows it moves by, so it can afford a lead, and it needs one: without it every
 *  row reaches the screen before its tiles have asked for anything, which is the pop-in a
 *  fast wheel or trackpad scroll showed while the overscan was dropped for speed alone. */
export function renderOverscan(motion: Motion, viewport: number): [number, number] {
  switch (motion.kind) {
    case 'still':
      return [viewport * RENDER_OVERSCAN, viewport * RENDER_OVERSCAN];
    case 'jump':
      return [0, 0];
    case 'scroll': {
      const lead = Math.min(
        Math.max(motion.peak * LEAD_MS, viewport * RENDER_OVERSCAN),
        viewport * LEAD_OVERSCAN_MAX,
      );
      const trail = viewport * TRAIL_OVERSCAN;
      return motion.direction > 0 ? [trail, lead] : [lead, trail];
    }
  }
}

/** The rows to mount, as `[start, end)`; see `renderOverscan`. */
export function renderRange(rows: Row[], scrollTop: number, viewport: number, motion: Motion): [number, number] {
  if (rows.length === 0) return [0, 0];
  const [above, below] = renderOverscan(motion, viewport);
  const start = rowIndexAt(rows, scrollTop - above);
  const end = rowIndexAt(rows, scrollTop + viewport + below) + 1;
  return [start, Math.min(end, rows.length)];
}

/** Whether a tile given its photo now waits `TILE_SETTLE_MS` before asking for its thumbnail
 *  (`createThumbRequest`'s `defer`).
 *
 *  Only when it will most likely be gone by then: a tile never seen settled costs the
 *  backend a render for nothing - on a fresh import, a blocking one each. That is a jump in
 *  a stream (a scrollbar or timeline drag), where the next frame replaces every tile, and a
 *  scroll so fast that a tile crosses the whole mounted window, lead and trail included, in
 *  less than the settle. Anything slower asks at once: a tile the lead mounts is on screen
 *  within a few hundred milliseconds, and deferring it spent a third of that waiting. A
 *  jump on its own (End, a folder click) lands where the user stops, so it asks at once. */
export function defersThumbs(motion: Motion, viewport: number): boolean {
  switch (motion.kind) {
    case 'still':
      return false;
    case 'jump':
      return motion.stream;
    case 'scroll': {
      const [above, below] = renderOverscan(motion, viewport);
      return motion.speed * TILE_SETTLE_MS > viewport + above + below;
    }
  }
}

/** The photos whose pages should be loaded, as `[start, end)` grid offsets, or null for a
 *  grid with no tiles in reach. */
export function fetchSpan(rows: Row[], scrollTop: number, viewport: number): [number, number] | null {
  const [start, end] = visibleRange(rows, scrollTop, viewport, viewport * FETCH_OVERSCAN);
  return itemSpan(rows.slice(start, end));
}

/** The folder whose section is at the top of the viewport, or null when the grid is empty.
 *
 *  This is what photon remembers across a restart. It follows the eye rather than the last
 *  sidebar click, so scrolling into a folder counts as being in it — and it returns null
 *  rather than a guess for an empty grid, so a launch whose first scan has not produced
 *  anything yet cannot overwrite what the previous session recorded. */
export function topFolderId(rows: Row[], sections: SectionLike[], scrollTop: number): number | null {
  if (rows.length === 0) return null;
  const row = rows[rowIndexAt(rows, scrollTop)];
  return sections[row.section]?.folderId ?? null;
}

/** Index of the tile row containing grid offset `offset`, or -1. */
export function rowOfItem(rows: Row[], offset: number): number {
  let lo = 0;
  let hi = rows.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid].first <= offset) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  if (found < 0) return -1;
  const row = rows[found];
  return row.kind === 'tiles' && offset < row.first + row.count ? found : -1;
}

/** Grid offsets covered by the tile rows in `rows`, as `[start, end)`, or null if none. */
export function itemSpan(rows: Row[]): [number, number] | null {
  let start = Infinity;
  let end = -Infinity;
  for (const r of rows) {
    if (r.kind !== 'tiles') continue;
    start = Math.min(start, r.first);
    end = Math.max(end, r.first + r.count);
  }
  return start === Infinity ? null : [start, end];
}

/** A rectangle in the canvas's own coordinates: the same space `Row.top` is in, so a band
 *  keeps its grip on the photos it was started over while the wheel scrolls under it. */
export interface Rect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** The grid offsets a rubber band covers, as contiguous ranges - one per row it crosses,
 *  with rows that join up merged into a single range.
 *
 *  Ranges rather than a list of offsets because that is what the backend fetch takes
 *  (`fetchIds`), and because a band over a whole row is one range whatever the column count.
 *
 *  A row's tiles occupy `top..top + tile`, not `top + tileRow(tile)`: the gap below a row
 *  belongs to no tile, so a band drawn entirely inside it selects nothing rather than both
 *  neighbours. Headers are not photos and are skipped.
 *
 *  `rect` may be given in any corner order; it is normalised here. */
export function itemsInRect(rows: Row[], rect: Rect, tile: number): [number, number][] {
  const left = Math.min(rect.x0, rect.x1);
  const right = Math.max(rect.x0, rect.x1);
  const top = Math.min(rect.y0, rect.y1);
  const bottom = Math.max(rect.y0, rect.y1);

  const ranges: [number, number][] = [];
  // This runs on every band pointermove and autoscroll frame, so it visits only the rows
  // the band can reach rather than the whole library. It starts at the row holding `top`:
  // every row before it ends, gap and all, at or above that row's top, which is at or above
  // `top`, and its tiles stop a whole GAP short of that, so none can be touched. Rows are
  // in `top` order, so the first one starting below `bottom` ends the walk.
  for (let i = rowIndexAt(rows, top); i < rows.length; i++) {
    const row = rows[i];
    if (row.top > bottom) break;
    if (row.kind !== 'tiles') continue;
    // The row holding `top` can still end above it, when `top` lies in the gap under it.
    if (row.top + tile < top) continue;
    // Tile k spans GAP + k*tileRow(tile) .. + tile, and touching counts. `firstColumn`
    // measures from each tile's *right* edge, so a band whose left edge lies in the gap
    // after tile k starts at k+1 rather than at k; `lastColumn` measures from the left edges
    // and is clamped to what this row actually holds, which is what stops a band running off
    // the end of a short last row into the next one's offsets.
    const firstColumn = Math.max(0, Math.ceil((left - GAP - tile) / tileRow(tile)));
    const lastColumn = Math.min(row.count - 1, Math.floor((right - GAP) / tileRow(tile)));
    if (lastColumn < firstColumn) continue;
    const from = row.first + firstColumn;
    const to = row.first + lastColumn;
    const previous = ranges[ranges.length - 1];
    if (previous && previous[1] + 1 === from) previous[1] = to;
    else ranges.push([from, to]);
  }
  return ranges;
}

/** How far to scroll the grid this frame while a rubber band is dragged towards an edge,
 *  in pixels: negative up, positive down, zero while the pointer is well inside.
 *
 *  `y`, `top` and `bottom` are in the window's coordinates (the viewport's own
 *  `getBoundingClientRect`), because that is where the pointer is. The answer is in pixels
 *  **per second**: the caller multiplies by the frame's own duration, so the grid scrolls at
 *  the same rate on a 60Hz screen and a 120Hz one. Speed ramps with how far into `margin`
 *  the pointer has gone and clamps at `max`, so a pointer dragged clean off the window - the
 *  common case, since the drag is captured - scrolls fast but not wildly.
 *
 *  A viewport shorter than two margins would otherwise have every point inside both, and a
 *  band in a short grid would scroll wherever the pointer rested. The margins are capped at
 *  a third of the height each, so there is always a middle third that holds still. */
export function edgeScrollSpeed(
  y: number,
  top: number,
  bottom: number,
  margin: number,
  max: number,
): number {
  const band = Math.min(margin, (bottom - top) / 3);
  // A viewport with no height (or a degenerate box) has no margins to be in, and dividing
  // by the band below would answer ±max for every point in it.
  if (band <= 0) return 0;
  if (y < top + band) return -max * Math.min(1, (top + band - y) / band);
  if (y > bottom - band) return max * Math.min(1, (y - (bottom - band)) / band);
  return 0;
}

/** A place in the grid that survives the rows changing height: the row at the top of the
 *  viewport, by its first photo, and how much of it has been scrolled past. */
export interface Pin {
  /** The grid offset of the row's first photo (for a header, of the section it heads). */
  offset: number;
  /** Whether the row is a header: a header and the row of photos under it share an offset. */
  header: boolean;
  /** The share of the row above the top of the viewport, 0 to 1. */
  into: number;
}

/** The row at the top of the viewport as a `Pin`, or null when the grid has no rows.
 *
 *  This is how the grid keeps your place when its tiles change width - a new size, or a
 *  window or sidebar resized under tiles that fill the row: every row's `top` moves, so a
 *  scroll position kept as a number points somewhere else afterwards, and a grid that jumps
 *  to a different year when the tiles grow is worse than no size control at all. The pin is
 *  read here from the layout as it was and `pinTop` finds it in the layout as it is.
 *
 *  A header answers with the section it heads (the photo the eye is on), and says it is a
 *  header, so it comes back to the header and not to the row under it. The share is kept
 *  because a resize spends the pin on every frame: coming back to the row's top would jump
 *  by up to a row on the first one. Past the end of the row - the space between two
 *  folders - is the whole of it. */
export function pinAt(rows: Row[], scrollTop: number): Pin | null {
  if (rows.length === 0) return null;
  const row = rows[rowIndexAt(rows, scrollTop)];
  const into = Math.min(1, Math.max(0, (scrollTop - row.top) / row.height));
  return { offset: row.first, header: row.kind === 'header', into };
}

/** Where `pin` is in `rows`, as a scroll position, or null when its photo is not there. */
export function pinTop(rows: Row[], pin: Pin): number | null {
  const i = rowOfItem(rows, pin.offset);
  if (i < 0) return null;
  const above = rows[i - 1];
  const row = pin.header && above?.kind === 'header' && above.first === pin.offset ? above : rows[i];
  return row.top + pin.into * row.height;
}

/** The scroll position to draw `rows` at. While the tiles are as they were when the
 *  viewport last reported its position (`moved` false), that position. On the render where
 *  they are not, the place the pin names in the new rows, held to where the browser will
 *  let the viewport go.
 *
 *  The viewport itself is moved by an effect, after that render: drawn at the position it
 *  still holds, the new rows are other rows - in a deep library a screen or more away, so
 *  blank - and every frame of a resize showed them for a frame, unmounting the tiles on
 *  screen and naming another folder at the top on the way. */
export function placeIn(rows: Row[], viewport: number, scrollTop: number, pin: Pin | null, moved: boolean): number {
  if (!moved || pin === null) return scrollTop;
  const back = pinTop(rows, pin);
  if (back === null) return scrollTop;
  return Math.min(back, Math.max(0, totalHeight(rows) - viewport));
}
