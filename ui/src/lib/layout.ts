/** Grid geometry. Square tiles in fixed-height rows, with one header row per folder
 *  section. Everything here is pure, so 100k items lay out in microseconds. */

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

export function buildRows(sections: SectionLike[], columns: number): Row[] {
  const rows: Row[] = [];
  let top = 0;
  sections.forEach((s, section) => {
    rows.push({ kind: 'header', section, first: s.offset, count: 0, top, height: HEADER });
    top += HEADER;
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

/** Index of the last row whose top is at or below `y` (the row containing `y`). */
export function rowIndexAt(rows: Row[], y: number): number {
  let lo = 0;
  let hi = rows.length - 1;
  if (hi < 0 || y <= 0) return 0;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (rows[mid].top <= y) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** Rows intersecting `[scrollTop - overscan, scrollTop + viewport + overscan)`, as `[start, end)`. */
export function visibleRange(rows: Row[], scrollTop: number, viewport: number, overscan: number): [number, number] {
  if (rows.length === 0) return [0, 0];
  const start = rowIndexAt(rows, scrollTop - overscan);
  const end = rowIndexAt(rows, scrollTop + viewport + overscan) + 1;
  return [start, Math.min(end, rows.length)];
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
