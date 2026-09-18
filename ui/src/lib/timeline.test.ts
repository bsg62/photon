import { describe, expect, it } from 'vitest';
import type { Section } from './api';
import { buildRows } from './layout';
import { labelledMarks, scrollTopFor, stripY, yearAt, yearMarks } from './timeline';

/** Noon on 1 July, so no timezone can move it into a neighbouring year. */
const mid = (year: number) => Date.UTC(year, 6, 1, 12) / 1000;
const section = (folderId: number, offset: number, count: number, year: number): Section => ({
  folderId,
  offset,
  count,
  takenAtMin: mid(year),
});

describe('yearMarks', () => {
  it('marks the first header of each run of a year', () => {
    const sections = [section(1, 0, 3, 2024), section(2, 3, 2, 2024), section(3, 5, 4, 2019)];
    const rows = buildRows(sections, 2);
    // 2024: header at 0. Folder 1 is two tile rows, folder 2 a header and one: 2019's
    // header follows them.
    const top2019 = rows.find((r) => r.kind === 'header' && r.section === 2)!.top;
    expect(yearMarks(sections, rows)).toEqual([
      { year: 2024, top: 0 },
      { year: 2019, top: top2019 },
    ]);
    expect(top2019).toBeGreaterThan(0);
  });

  it('marks a year again when it comes back, as a search can make it', () => {
    const sections = [section(1, 0, 1, 2020), section(2, 1, 1, 2024), section(3, 2, 1, 2020)];
    expect(yearMarks(sections, buildRows(sections, 4)).map((m) => m.year)).toEqual([2020, 2024, 2020]);
  });

  it('has nothing to mark without headers', () => {
    const sections = [section(1, 0, 3, 2024)];
    expect(yearMarks(sections, buildRows(sections, 2, false))).toEqual([]);
  });
});

describe('yearAt', () => {
  const marks = [
    { year: 2024, top: 0 },
    { year: 2019, top: 500 },
  ];
  it('is the year of the last mark at or above the position', () => {
    expect(yearAt(marks, 0)).toBe(2024);
    expect(yearAt(marks, 499)).toBe(2024);
    expect(yearAt(marks, 500)).toBe(2019);
    expect(yearAt(marks, 9999)).toBe(2019);
    expect(yearAt([], 10)).toBeNull();
  });
});

describe('labelledMarks', () => {
  it('drops a label that would print over the one before it', () => {
    const marks = [
      { year: 2024, top: 0 },
      { year: 2023, top: 50 }, // 5px down a 100px strip: too close to 2024
      { year: 2022, top: 300 },
      { year: 2021, top: 390 }, // 9px below 2022: too close
      { year: 2020, top: 900 },
    ];
    expect(labelledMarks(marks, 1000, 100, 14).map((m) => m.year)).toEqual([2024, 2022, 2020]);
  });
});

describe('scrollTopFor', () => {
  it('lands a press on a label exactly on that year', () => {
    const total = 10_000;
    const strip = 400;
    const y = stripY(2_500, total, strip);
    expect(scrollTopFor(y, strip, total, 800)).toBe(2_500);
  });

  it('clamps to the scrollable range', () => {
    expect(scrollTopFor(-20, 400, 10_000, 800)).toBe(0);
    expect(scrollTopFor(400, 400, 10_000, 800)).toBe(9_200);
    expect(scrollTopFor(999, 400, 10_000, 800)).toBe(9_200);
    expect(scrollTopFor(100, 0, 10_000, 800)).toBe(0);
    // Content shorter than the viewport cannot scroll at all.
    expect(scrollTopFor(200, 400, 500, 800)).toBe(0);
  });
});
