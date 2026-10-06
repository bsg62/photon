import { describe, expect, it } from 'vitest';
import type { Grouping, Sort } from './api';
import { GROUPINGS, groupingApplies, laidOutByFolder, periodLabel } from './grouping';

const sort = (key: Sort['key'], group: Grouping): Sort => ({ key, reverse: false, group });

describe('GROUPINGS', () => {
  it('offers every grouping once, the folder first', () => {
    expect(GROUPINGS.map((g) => g.value)).toEqual(['folder', 'day', 'month', 'year', 'none']);
  });
});

describe('groupingApplies', () => {
  it('only under the date sort, whatever grouping is stored', () => {
    expect(groupingApplies(sort('date', 'month'))).toBe(true);
    expect(groupingApplies(sort('date', 'none'))).toBe(true);
    for (const key of ['modified', 'name', 'size'] as const) expect(groupingApplies(sort(key, 'month'))).toBe(false);
  });
});

describe('laidOutByFolder', () => {
  it('is the date sort grouped by folder, and nothing else', () => {
    expect(laidOutByFolder(sort('date', 'folder'))).toBe(true);
    expect(laidOutByFolder({ key: 'date', reverse: true, group: 'folder' })).toBe(true);
    for (const group of ['day', 'month', 'year', 'none'] as const) expect(laidOutByFolder(sort('date', group))).toBe(false);
    // A flat sort keeps its stored grouping, and is not by folder for having it.
    expect(laidOutByFolder(sort('size', 'folder'))).toBe(false);
  });
});

describe('periodLabel', () => {
  it('names a year, a month and a day', () => {
    expect(periodLabel({ year: 2026, month: null, day: null }, 'en-US')).toBe('2026');
    expect(periodLabel({ year: 2026, month: 10, day: null }, 'en-US')).toBe('October 2026');
    expect(periodLabel({ year: 2026, month: 10, day: 4 }, 'en-US')).toBe('Sunday, October 4, 2026');
  });

  it('reads the numbers as they are, at either end of a year', () => {
    // Built from an instant instead, one of these lands in the neighbouring year in any
    // zone that is not UTC.
    expect(periodLabel({ year: 2025, month: 12, day: 31 }, 'en-US')).toBe('Wednesday, December 31, 2025');
    expect(periodLabel({ year: 2026, month: 1, day: 1 }, 'en-US')).toBe('Thursday, January 1, 2026');
    expect(periodLabel({ year: 2026, month: 1, day: null }, 'en-US')).toBe('January 2026');
  });
});
