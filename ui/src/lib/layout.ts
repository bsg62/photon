/** Grid geometry. Square tiles in fixed-height rows, with one header row per folder
 *  section. Everything here is pure, so 100k items lay out in microseconds. */

import type { GridView, Section } from './api';

export const TILE = 160;
export const GAP = 8;
export const HEADER = 32;
export const TILE_ROW = TILE + GAP;

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

export function columnsFor(width: number): number {
  return Math.max(1, Math.floor((width + GAP) / TILE_ROW));
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

export function buildRows(sections: SectionLike[], columns: number, headers = true): Row[] {
  const rows: Row[] = [];
  let top = 0;
  sections.forEach((s, section) => {
    if (headers) {
      rows.push({ kind: 'header', section, first: s.offset, count: 0, top, height: HEADER });
      top += HEADER;
    }
    const end = s.offset + s.count;
    for (let first = s.offset; first < end; first += columns) {
      rows.push({ kind: 'tiles', section, first, count: Math.min(columns, end - first), top, height: TILE_ROW });
      top += TILE_ROW;
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
 *  A row's tiles occupy `top..top + TILE`, not `top + TILE_ROW`: the gap below a row belongs
 *  to no tile, so a band drawn entirely inside it selects nothing rather than both
 *  neighbours. Headers are not photos and are skipped.
 *
 *  `rect` may be given in any corner order; it is normalised here. */
export function itemsInRect(rows: Row[], columns: number, rect: Rect): [number, number][] {
  const left = Math.min(rect.x0, rect.x1);
  const right = Math.max(rect.x0, rect.x1);
  const top = Math.min(rect.y0, rect.y1);
  const bottom = Math.max(rect.y0, rect.y1);

  const ranges: [number, number][] = [];
  for (const row of rows) {
    if (row.kind !== 'tiles') continue;
    if (row.top + TILE < top || row.top > bottom) continue;
    // Tile k spans GAP + k*TILE_ROW .. + TILE. Touching counts, so the first tile is the
    // last one whose left edge is at or before `right`, and vice versa.
    const firstColumn = Math.max(0, Math.ceil((left - GAP - TILE) / TILE_ROW));
    const lastColumn = Math.min(row.count - 1, Math.floor((right - GAP) / TILE_ROW));
    if (lastColumn < firstColumn || lastColumn < 0) continue;
    const from = row.first + Math.min(firstColumn, row.count - 1);
    const to = row.first + lastColumn;
    const previous = ranges[ranges.length - 1];
    if (previous && previous[1] + 1 === from) previous[1] = to;
    else ranges.push([from, to]);
  }
  return ranges;
}
