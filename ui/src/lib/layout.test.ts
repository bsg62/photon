import { describe, expect, it } from 'vitest';
import { buildRows, columnsFor, edgeScrollSpeed, GAP, HEADER, itemSpan, itemsInRect, layoutSections, rowIndexAt, rowOfItem, TILE, TILE_ROW, topFolderId, totalHeight, visibleRange } from './layout';

const sections = [
  { folderId: 1, offset: 0, count: 5 },
  { folderId: 2, offset: 5, count: 3 },
];

describe('layout', () => {
  it('fits columns to the width', () => {
    expect(columnsFor(800)).toBe(4);
    expect(columnsFor(100)).toBe(1);
    expect(columnsFor(0)).toBe(1);
  });

  it('builds header and tile rows per section', () => {
    const rows = buildRows(sections, 2);
    expect(rows.map((r) => [r.kind, r.first, r.count, r.top])).toEqual([
      ['header', 0, 0, 0],
      ['tiles', 0, 2, 32],
      ['tiles', 2, 2, 200],
      ['tiles', 4, 1, 368],
      ['header', 5, 0, 536],
      ['tiles', 5, 2, 568],
      ['tiles', 7, 1, 736],
    ]);
    expect(totalHeight(rows)).toBe(904);
    expect(totalHeight([])).toBe(0);
  });

  it('hit-tests rows by y', () => {
    const rows = buildRows(sections, 2);
    expect(rowIndexAt(rows, -5)).toBe(0);
    expect(rowIndexAt(rows, 31)).toBe(0);
    expect(rowIndexAt(rows, 32)).toBe(1);
    expect(rowIndexAt(rows, 500)).toBe(3);
    expect(rowIndexAt(rows, 10_000)).toBe(6);
  });

  it('finds visible ranges and their items', () => {
    const rows = buildRows(sections, 2);
    expect(visibleRange(rows, 200, 100, 0)).toEqual([2, 3]);
    expect(visibleRange(rows, 0, 10_000, 0)).toEqual([0, 7]);
    expect(visibleRange([], 0, 100, 0)).toEqual([0, 0]);
    expect(itemSpan(rows.slice(0, 3))).toEqual([0, 4]);
    expect(itemSpan(rows.slice(4, 5))).toBeNull();
  });

  it('lays Recent out as one continuous run with no headers', () => {
    // Recent orders photos by date across folders, so wherever folders overlap in time the
    // index hands back a section per photo — 500 photos, 500 sections, measured against a
    // library of 12 interleaved folders. Laid out as folder runs that is a header and a
    // one-tile row each: a grid as tall as the whole library and five-sixths empty.
    const perPhoto = [0, 1, 2, 3, 4].map((i) => ({
      folderId: 10 + (i % 2),
      offset: i,
      count: 1,
      takenAtMin: 900 - i,
    }));
    const flat = layoutSections('recent', perPhoto, 5);
    expect(flat).toEqual([{ folderId: 10, offset: 0, count: 5, takenAtMin: 896 }]);

    const rows = buildRows(flat, 2, false);
    expect(rows.map((r) => [r.kind, r.first, r.count, r.top])).toEqual([
      ['tiles', 0, 2, 0],
      ['tiles', 2, 2, 168],
      ['tiles', 4, 1, 336],
    ]);
    // Against 5 * (HEADER + TILE_ROW) = 1000 for the same photos as folder sections.
    expect(totalHeight(rows)).toBe(504);
  });

  it('leaves every other view grouped by folder', () => {
    const withDates = sections.map((s) => ({ ...s, takenAtMin: 100 }));
    expect(layoutSections('all', withDates, 8)).toBe(withDates);
    expect(layoutSections('starred', withDates, 8)).toBe(withDates);
    expect(layoutSections('search', withDates, 8)).toBe(withDates);
    // An empty Recent view has nothing to lay out, and must not invent a run of zero.
    expect(layoutSections('recent', [], 0)).toEqual([]);
  });

  it('names the folder at the top of the viewport', () => {
    // What gets remembered for the next launch: whichever folder the eye is on, whether
    // the user scrolled there or clicked it in the sidebar.
    const rows = buildRows(sections, 2);
    expect(topFolderId(rows, sections, 0)).toBe(1);
    // Still inside folder 1's last tile row.
    expect(topFolderId(rows, sections, 400)).toBe(1);
    // Folder 2's header is at 536.
    expect(topFolderId(rows, sections, 536)).toBe(2);
    expect(topFolderId(rows, sections, 10_000)).toBe(2);
  });

  it('names no folder when there is no grid to be scrolled', () => {
    // A launch before the first scan has produced anything: nothing to remember, and
    // nothing that should overwrite what the last session remembered.
    expect(topFolderId([], [], 0)).toBeNull();
    expect(topFolderId(buildRows(sections, 2), [], 0)).toBeNull();
  });

  it('locates the row holding an item', () => {
    const rows = buildRows(sections, 2);
    expect(rowOfItem(rows, 0)).toBe(1);
    expect(rowOfItem(rows, 4)).toBe(3);
    expect(rowOfItem(rows, 5)).toBe(5);
    expect(rowOfItem(rows, 7)).toBe(6);
    expect(rowOfItem(rows, 8)).toBe(-1);
  });
});

describe('itemsInRect', () => {
  // Two sections of 5 photos, 3 columns, no headers: rows at 0 and 168 (TILE_ROW).
  const rows = buildRows([{ folderId: 1, offset: 0, count: 5 }], 3, false);
  const rect = (x0: number, y0: number, x1: number, y1: number) => ({ x0, y0, x1, y1 });

  /** The point of returning ranges rather than one span: a band narrower than the grid
   *  takes a few photos from each row it crosses, and merging those into one range would
   *  select everything between them - the over-selection ids-not-offsets exists to stop. */
  it('keeps a narrow band down one column as one range per row', () => {
    const tall = buildRows([{ folderId: 1, offset: 0, count: 9 }], 3, false);
    const column2 = GAP + 2 * TILE_ROW;
    expect(itemsInRect(tall, rect(column2 + 1, 0, column2 + TILE - 1, 3 * TILE_ROW))).toEqual([
      [2, 2],
      [5, 5],
      [8, 8],
    ]);
  });

  /** The left edge is measured from each tile's right edge, so a band starting in the gap
   *  after a tile does not take it. Measuring from the left edges instead agrees everywhere
   *  except here, which is why this case exists. */
  it('does not take the tile to the left of a band that starts in the gap', () => {
    const gapAfterTile0 = GAP + TILE + 2;
    expect(itemsInRect(rows, rect(gapAfterTile0, 10, 1000, 20))).toEqual([[1, 2]]);
  });

  it('takes the tiles a rectangle touches in one row', () => {
    // Tile k spans x = GAP + k*TILE_ROW .. + TILE, so tile 1 starts at 176.
    expect(itemsInRect(rows, rect(180, 10, 200, 20))).toEqual([[1, 1]]);
  });

  it('merges rows that join up into one range', () => {
    expect(itemsInRect(rows, rect(0, 0, 1000, 1000))).toEqual([[0, 4]]);
  });

  /** The gap below a row belongs to no tile: a band that only grazes it selects nothing,
   *  or dragging between two rows would sweep up both. */
  it('selects nothing from a band inside the gap between rows', () => {
    expect(itemsInRect(rows, rect(0, TILE + 1, 1000, TILE_ROW - 1))).toEqual([]);
  });

  it('stops at the last tile of a short row', () => {
    // The second row holds 2 of the 5 photos; a band across it cannot reach a third.
    expect(itemsInRect(rows, rect(0, TILE_ROW + 1, 1000, TILE_ROW + TILE))).toEqual([[3, 4]]);
  });

  it('ignores headers', () => {
    const withHeaders = buildRows([{ folderId: 1, offset: 0, count: 2 }], 3, true);
    expect(itemsInRect(withHeaders, rect(0, 0, 1000, HEADER - 1))).toEqual([]);
    expect(itemsInRect(withHeaders, rect(0, 0, 1000, HEADER + TILE))).toEqual([[0, 1]]);
  });

  it('takes nothing from an empty rectangle or an empty grid', () => {
    expect(itemsInRect(rows, rect(10, 10, 10, 10))).toEqual([[0, 0]]);
    expect(itemsInRect([], rect(0, 0, 1000, 1000))).toEqual([]);
  });

  /** Two sections are two runs of rows; a band over both is two ranges only if the offsets
   *  do not run on. Here they do, so it is one. */
  it('joins ranges across sections when the offsets are contiguous', () => {
    const two = buildRows(
      [
        { folderId: 1, offset: 0, count: 3 },
        { folderId: 2, offset: 3, count: 3 },
      ],
      3,
      false,
    );
    expect(itemsInRect(two, rect(0, 0, 1000, 1000))).toEqual([[0, 5]]);
  });
});

describe('edgeScrollSpeed', () => {
  // A viewport 500px tall on screen at y = 100..600, with a 40px margin.
  const speed = (y: number) => edgeScrollSpeed(y, 100, 600, 40, 20);

  it('does not scroll while the pointer is well inside the viewport', () => {
    expect(speed(300)).toBe(0);
    expect(speed(141)).toBe(0);
    expect(speed(559)).toBe(0);
  });

  /** The boundaries themselves. Whether the comparison is `<` or `<=` cannot matter - the
   *  ramp is zero at the boundary either way - so this pins the behaviour (no movement, and
   *  movement one pixel further out) rather than the operator. `Math.abs` because the top
   *  edge computes `-0`, which `toBe(0)` refuses. */
  it('holds still exactly at the edge of the margin', () => {
    expect(Math.abs(speed(140))).toBe(0);
    expect(Math.abs(speed(560))).toBe(0);
    expect(speed(139)).toBeLessThan(0);
    expect(speed(561)).toBeGreaterThan(0);
  });

  /** A viewport with no height has no margins to be inside. Without the guard the ramp
   *  divides by zero and answers ±max for every point, so a grid measured before its first
   *  layout would scroll at full speed. */
  it('never scrolls a viewport with no height, or with no margin', () => {
    expect(edgeScrollSpeed(100, 100, 100, 40, 20)).toBe(0);
    expect(edgeScrollSpeed(120, 100, 100, 40, 20)).toBe(0);
    expect(edgeScrollSpeed(300, 100, 600, 0, 20)).toBe(0);
    expect(edgeScrollSpeed(110, 100, 600, 0, 20)).toBe(0);
  });

  it('scrolls faster the deeper into the margin the pointer is', () => {
    const shallow = speed(590);
    const deep = speed(599);
    expect(shallow).toBeGreaterThan(0);
    expect(deep).toBeGreaterThan(shallow);
  });

  it('scrolls up at the top edge and down at the bottom', () => {
    expect(speed(110)).toBeLessThan(0);
    expect(speed(590)).toBeGreaterThan(0);
  });

  /** A pointer dragged off the window entirely must not scroll arbitrarily fast: the drag
   *  is captured, so this is the common case, not an edge one. */
  it('clamps to the maximum however far outside the viewport the pointer goes', () => {
    expect(speed(601)).toBe(20);
    expect(speed(5000)).toBe(20);
    expect(speed(99)).toBe(-20);
    expect(speed(-5000)).toBe(-20);
  });

  /** A viewport shorter than two margins would otherwise be all edge, and a band in a short
   *  grid would scroll wherever the pointer rested. 100..160 is 60 tall, so each margin is
   *  capped at 20 and the middle third holds still. */
  it('keeps a still middle third in a viewport shorter than its margins', () => {
    expect(edgeScrollSpeed(130, 100, 160, 40, 20)).toBe(0);
    expect(edgeScrollSpeed(105, 100, 160, 40, 20)).toBeLessThan(0);
    expect(edgeScrollSpeed(155, 100, 160, 40, 20)).toBeGreaterThan(0);
  });
});
