/** Grid geometry. Square tiles in fixed-height rows, with one header row per folder
 *  section. Everything here is pure, so 100k items lay out in microseconds. */

import type { GridView, Section } from './api';

export type TileSize = 'small' | 'medium' | 'large';

/** How wide a tile is at each step, in CSS pixels.
 *
 *  Every step is at or below 256, which is `ThumbSize::Grid`'s maximum edge. The cache key
 *  is the photo's fingerprint while the cache *directory* is what separates the sizes, so
 *  raising that maximum without renaming the directory would silently serve already-cached
 *  256px files at the new size - soft forever, with nothing to say why. A step above 256
 *  needs its own `ThumbSize` variant, not a larger `Grid`. */
export const TILE_WIDTH: Record<TileSize, number> = { small: 120, medium: 160, large: 224 };

export const GAP = 8;
export const HEADER = 32;

/** A tile row's full height: the tile plus the gutter under it. */
export function tileRow(tile: number): number {
  return tile + GAP;
}

export interface SectionLike {
  folderId: number;
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

export function columnsFor(width: number, tile: number = TILE_WIDTH.medium): number {
  return Math.max(1, Math.floor((width + GAP) / tileRow(tile)));
}

/** The sections the grid lays out for `view`, which are not always the index's own.
 *
 *  Recent is a flat newest-first list rather than a folder listing, so wherever folders
 *  overlap in time `GridIndex::build` starts a new section on every photo — 500 photos came
 *  back as 500 sections from a library of twelve interleaved folders. Laid out as folder
 *  runs each of those takes a header and a row of its own, so the view rendered as tall as
 *  the entire library with one tile per row and the rest of every row blank. Collapsing them
 *  into one run (and dropping the headers, which would name a folder per photo) is what
 *  makes Recent read as the shortlist it is.
 *
 *  Only the grid's geometry is collapsed. `library.info.sections` stays as the index built
 *  it, because the sidebar still has to know which folders the view's photos came from. */
export function layoutSections(view: GridView, sections: Section[], len: number): Section[] {
  if (view !== 'recent' || len === 0) return sections;
  return [
    {
      folderId: sections[0]?.folderId ?? 0,
      offset: 0,
      count: len,
      // Nothing laid out here reads either field — the run has no header — but a section
      // that lied about its folder or its date would be a trap for the next reader.
      takenAtMin: Math.min(...sections.map((s) => s.takenAtMin)),
    },
  ];
}

export function buildRows(
  sections: SectionLike[],
  columns: number,
  headers = true,
  tile: number = TILE_WIDTH.medium,
): Row[] {
  const rows: Row[] = [];
  let top = 0;
  sections.forEach((s, section) => {
    if (headers) {
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
export function itemsInRect(rows: Row[], rect: Rect, tile: number = TILE_WIDTH.medium): [number, number][] {
  const left = Math.min(rect.x0, rect.x1);
  const right = Math.max(rect.x0, rect.x1);
  const top = Math.min(rect.y0, rect.y1);
  const bottom = Math.max(rect.y0, rect.y1);

  const ranges: [number, number][] = [];
  for (const row of rows) {
    if (row.kind !== 'tiles') continue;
    if (row.top + tile < top || row.top > bottom) continue;
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

/** The grid offset of the first photo in the row at the top of the viewport, or null when
 *  the grid has no rows.
 *
 *  Paired with `scrollTopForOffset` to keep your place when the tile size changes: every
 *  row's `top` moves, so a scroll position kept as a number points somewhere else
 *  afterwards, and a grid that jumps to a different year when the tiles grow is worse than
 *  no size control at all. A header row answers with the offset of the section it heads,
 *  which is the photo the eye is on. */
export function firstVisibleOffset(rows: Row[], scrollTop: number): number | null {
  if (rows.length === 0) return null;
  return rows[rowIndexAt(rows, scrollTop)].first;
}

/** The scroll position that puts `offset`'s row at the top of the viewport, or null when
 *  no row holds it. */
export function scrollTopForOffset(rows: Row[], offset: number): number | null {
  const row = rowOfItem(rows, offset);
  return row < 0 ? null : rows[row].top;
}
