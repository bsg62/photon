import { describe, expect, it } from 'vitest';
import { buildRows, columnsFor, itemSpan, rowIndexAt, rowOfItem, totalHeight, visibleRange } from './layout';

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

  it('locates the row holding an item', () => {
    const rows = buildRows(sections, 2);
    expect(rowOfItem(rows, 0)).toBe(1);
    expect(rowOfItem(rows, 4)).toBe(3);
    expect(rowOfItem(rows, 5)).toBe(5);
    expect(rowOfItem(rows, 7)).toBe(6);
    expect(rowOfItem(rows, 8)).toBe(-1);
  });
});
