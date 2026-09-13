import { describe, expect, it } from 'vitest';
import { folderRows, groupByYear } from './folders';

/** Seconds since the epoch, since that is what `takenAtMin` carries. */
const at = (iso: string) => Math.floor(new Date(iso).getTime() / 1000);

const folders = [
  { id: 1, watchedId: 1, parentId: null, path: '/photos', name: 'photos' },
  { id: 2, watchedId: 1, parentId: 1, path: '/photos/rome', name: 'rome' },
  { id: 3, watchedId: 1, parentId: 1, path: '/photos/oslo', name: 'oslo' },
  { id: 4, watchedId: 1, parentId: 1, path: '/photos/old', name: 'old' },
];

const sections = [
  { folderId: 2, offset: 0, count: 12, takenAtMin: at('2024-06-01T12:00:00') },
  { folderId: 3, offset: 12, count: 3, takenAtMin: at('2024-11-20T12:00:00') },
  { folderId: 4, offset: 15, count: 40, takenAtMin: at('2019-02-02T12:00:00') },
];

describe('folderRows', () => {
  it('names each folder that has photos, and carries its count', () => {
    expect(folderRows(sections, folders)).toEqual([
      { folderId: 2, name: 'rome', count: 12, year: 2024, takenAtMin: sections[0].takenAtMin },
      { folderId: 3, name: 'oslo', count: 3, year: 2024, takenAtMin: sections[1].takenAtMin },
      { folderId: 4, name: 'old', count: 40, year: 2019, takenAtMin: sections[2].takenAtMin },
    ]);
  });

  it('lists only folders that have photos', () => {
    // Folder 1 holds no photos of its own — it has no section — so it must not appear,
    // which is the whole point of listing sections rather than the folder table.
    const names = folderRows(sections, folders).map((r) => r.name);
    expect(names).not.toContain('photos');
  });

  it('still renders a section whose folder row has not arrived yet', () => {
    // An item implies a folder row, but a section can arrive before the folder list is
    // refreshed. There is no path to fall back to in that case, so the name is blank —
    // which beats throwing and losing the whole sidebar.
    const rows = folderRows([{ folderId: 99, offset: 0, count: 1, takenAtMin: at('2024-01-01T12:00:00') }], folders);
    expect(rows).toEqual([{ folderId: 99, name: '', count: 1, year: 2024, takenAtMin: at('2024-01-01T12:00:00') }]);
  });

  it('handles an empty grid', () => {
    expect(folderRows([], folders)).toEqual([]);
  });
});

describe('groupByYear', () => {
  it('groups by year, newest year first', () => {
    const groups = groupByYear(folderRows(sections, folders));
    expect(groups.map((g) => g.year)).toEqual([2024, 2019]);
    expect(groups[0].rows.map((r) => r.name)).toEqual(['oslo', 'rome']);
    expect(groups[1].rows.map((r) => r.name)).toEqual(['old']);
  });

  it('puts the folder with the newest oldest-photo first within a year', () => {
    // oslo's newest photo is November, rome's is June, so oslo leads despite rome coming
    // first in grid order.
    const groups = groupByYear(folderRows(sections, folders));
    expect(groups[0].rows[0].name).toBe('oslo');
  });

  it('handles no folders at all', () => {
    expect(groupByYear([])).toEqual([]);
  });
});
