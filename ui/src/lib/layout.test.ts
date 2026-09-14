import { describe, expect, it } from 'vitest';
import { buildRows, columnsFor, itemSpan, layoutSections, rowIndexAt, rowOfItem, topFolderId, totalHeight, visibleRange } from './layout';

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
