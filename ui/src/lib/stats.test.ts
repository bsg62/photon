import { describe, expect, it } from 'vitest';
import type { LibraryStats } from './api';
import { cameraStatRows, lensStatRows, librarySize, statsSummary, yearRows } from './stats';

const empty: LibraryStats = {
  photos: 0,
  videos: 0,
  bytes: 0,
  oldest: null,
  newest: null,
  years: [],
  cameras: [],
  noCamera: 0,
  lenses: [],
};

const utc = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

describe('statsSummary', () => {
  it('names photos, videos, the size and the span of years', () => {
    const stats = { ...empty, photos: 12034, videos: 310, bytes: 5 * 1024 ** 3, oldest: utc(2004, 3, 1), newest: utc(2026, 7, 14) };
    expect(statsSummary(stats, 'en-US')).toBe('12,034 photos and 310 videos · 5.0 GB · 2004 to 2026');
  });

  it('leaves the videos out when there are none, and says one year once', () => {
    const stats = { ...empty, photos: 1, bytes: 2 * 1024 ** 2, oldest: utc(2020, 1, 1), newest: utc(2020, 12, 31) };
    expect(statsSummary(stats, 'en-US')).toBe('1 photo · 2 MB · 2020');
  });

  it('reads the year off the camera clock, not the local zone', () => {
    // The last second of 2019 and the first of 2020, which a zone either side of UTC
    // would put in the same year.
    const stats = { ...empty, photos: 2, oldest: utc(2020, 1, 1) - 1, newest: utc(2020, 1, 1) };
    expect(statsSummary(stats, 'en-US')).toContain('2019 to 2020');
  });

  it('has no years for an empty library', () => {
    expect(statsSummary(empty, 'en-US')).toBe('0 photos · 0 MB');
  });
});

describe('librarySize', () => {
  it('steps from MB to GB to TB at the binary thousands', () => {
    expect(librarySize(1024 ** 3 - 1024 ** 2)).toBe('1023 MB');
    expect(librarySize(1024 ** 3)).toBe('1.0 GB');
    expect(librarySize(48.25 * 1024 ** 3)).toBe('48.3 GB');
    expect(librarySize(1024 ** 4)).toBe('1.00 TB');
    expect(librarySize(2.5 * 1024 ** 4)).toBe('2.50 TB');
  });
});

describe('yearRows', () => {
  it('links each year to exactly that year and scales the bars to the largest', () => {
    const rows = yearRows({ ...empty, years: [{ year: 2024, count: 50 }, { year: 2019, count: 200 }, { year: 2001, count: 1 }] });
    expect(rows.map((r) => [r.label, r.count, r.search])).toEqual([
      ['2024', 50, 'from:2024 to:2024'],
      ['2019', 200, 'from:2019 to:2019'],
      ['2001', 1, 'from:2001 to:2001'],
    ]);
    expect(rows.map((r) => r.share)).toEqual([0.25, 1, 0.005]);
  });

  it('is empty for an empty library', () => {
    expect(yearRows(empty)).toEqual([]);
  });
});

describe('cameraStatRows and lensStatRows', () => {
  it('names a camera as the info panel does and searches its field', () => {
    const rows = cameraStatRows({
      ...empty,
      cameras: [
        { make: 'Canon', model: 'Canon EOS 5D', count: 30 },
        { make: 'Apple', model: 'iPhone 15', count: 15 },
        { make: null, model: 'Scanner "Pro"', count: 3 },
      ],
    });
    expect(rows.map((r) => [r.label, r.search, r.share])).toEqual([
      ['Canon EOS 5D', 'camera:"Canon EOS 5D"', 1],
      ['Apple iPhone 15', 'camera:"Apple iPhone 15"', 0.5],
      // A quote cannot be escaped in the grammar, so it is dropped from the search only.
      ['Scanner "Pro"', 'camera:"Scanner  Pro"', 0.1],
    ]);
  });

  it('adds up cameras the files spell differently and the list names alike', () => {
    // With the make and without it, one camera; together they outnumber the phone.
    const rows = cameraStatRows({
      ...empty,
      cameras: [
        { make: 'Apple', model: 'iPhone 15', count: 50 },
        { make: 'Canon', model: 'Canon EOS 5D', count: 40 },
        { make: null, model: 'Canon EOS 5D', count: 20 },
      ],
    });
    expect(rows.map((r) => [r.label, r.count, r.share])).toEqual([
      ['Canon EOS 5D', 60, 1],
      ['Apple iPhone 15', 50, 50 / 60],
    ]);
    expect(new Set(rows.map((r) => r.search)).size).toBe(rows.length);
  });

  it('links a lens to its field', () => {
    const rows = lensStatRows({ ...empty, lenses: [{ lens: 'EF50mm f/1.8 STM', count: 4 }] });
    expect(rows).toEqual([{ label: 'EF50mm f/1.8 STM', count: 4, share: 1, search: 'lens:"EF50mm f/1.8 STM"' }]);
  });
});
