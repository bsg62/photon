import { describe, expect, it } from 'vitest';
import type { Folder, GridView } from './api';
import {
  arrangeFolders,
  enterFolder,
  folderRows,
  folderSummary,
  groupByYear,
  locateItem,
  photoCount,
  returnToAll,
  yearOf,
  type FolderRow,
} from './folders';

/** Seconds since the epoch, since that is what `takenAtMin` carries. */
const at = (iso: string) => Math.floor(new Date(iso).getTime() / 1000);

const folders = [
  { id: 1, watchedId: 1, parentId: null, path: '/photos', name: 'photos', hidden: false, alias: null },
  { id: 2, watchedId: 1, parentId: 1, path: '/photos/rome', name: 'rome', hidden: false, alias: null },
  { id: 3, watchedId: 1, parentId: 1, path: '/photos/oslo', name: 'oslo', hidden: false, alias: null },
  { id: 4, watchedId: 1, parentId: 1, path: '/photos/old', name: 'old', hidden: false, alias: null },
];

const tallies = [
  { folderId: 2, count: 12, takenAtMin: at('2024-06-01T12:00:00'), bytes: 500, modifiedMs: 3_000 },
  { folderId: 3, count: 3, takenAtMin: at('2024-11-20T12:00:00'), bytes: 900, modifiedMs: 1_000 },
  { folderId: 4, count: 40, takenAtMin: at('2019-02-02T12:00:00'), bytes: 100, modifiedMs: 2_000 },
];

describe('folderRows', () => {
  it('names each folder that has photos, and carries its count', () => {
    expect(folderRows(tallies, folders)).toEqual([
      { folderId: 2, name: 'rome', count: 12, year: 2024, takenAtMin: tallies[0].takenAtMin, bytes: 500, modifiedMs: 3_000 },
      { folderId: 3, name: 'oslo', count: 3, year: 2024, takenAtMin: tallies[1].takenAtMin, bytes: 900, modifiedMs: 1_000 },
      { folderId: 4, name: 'old', count: 40, year: 2019, takenAtMin: tallies[2].takenAtMin, bytes: 100, modifiedMs: 2_000 },
    ]);
  });

  it('shows a folder by its alias, in place of its directory name', () => {
    const aliased = folders.map((f) => (f.id === 3 ? { ...f, alias: 'Aarhus trip' } : f));
    expect(folderRows(tallies, aliased).map((r) => r.name)).toEqual(['rome', 'Aarhus trip', 'old']);
  });

  it('lists only folders that have photos', () => {
    // Folder 1 holds no photos of its own — it has no tally — so it must not appear,
    // which is the whole point of listing tallies rather than the folder table.
    const names = folderRows(tallies, folders).map((r) => r.name);
    expect(names).not.toContain('photos');
  });

  it('still renders a tally whose folder row has not arrived yet', () => {
    // An item implies a folder row, but a tally can arrive before the folder list is
    // refreshed. There is no path to fall back to in that case, so the name is blank —
    // which beats throwing and losing the whole sidebar.
    const tally = { folderId: 99, count: 1, takenAtMin: at('2024-01-01T12:00:00'), bytes: 1, modifiedMs: 1 };
    const rows = folderRows([tally], folders);
    expect(rows).toEqual([{ folderId: 99, name: '', count: 1, year: 2024, takenAtMin: tally.takenAtMin, bytes: 1, modifiedMs: 1 }]);
  });

  it('handles an empty grid', () => {
    expect(folderRows([], folders)).toEqual([]);
  });
});

describe('groupByYear', () => {
  it('groups by year, newest year first', () => {
    const groups = groupByYear(folderRows(tallies, folders));
    expect(groups.map((g) => g.year)).toEqual([2024, 2019]);
    expect(groups[0].rows.map((r) => r.name)).toEqual(['oslo', 'rome']);
    expect(groups[1].rows.map((r) => r.name)).toEqual(['old']);
  });

  it('puts the folder with the newest oldest-photo first within a year', () => {
    // oslo's newest photo is November, rome's is June, so oslo leads despite rome coming
    // first in grid order.
    const groups = groupByYear(folderRows(tallies, folders));
    expect(groups[0].rows[0].name).toBe('oslo');
  });

  it('handles no folders at all', () => {
    expect(groupByYear([])).toEqual([]);
  });
});

describe('arrangeFolders', () => {
  const names = (groups: { rows: FolderRow[] }[]) => groups.map((g) => g.rows.map((r) => r.name));

  it('keeps the year groups by date, and turns them over when reversed', () => {
    const rows = folderRows(tallies, folders);
    expect(arrangeFolders(rows, { key: 'date', reverse: false, group: 'folder' as const })).toEqual(groupByYear(rows));
    const reversed = arrangeFolders(rows, { key: 'date', reverse: true, group: 'folder' as const });
    expect(reversed.map((g) => g.year)).toEqual([2019, 2024]);
    expect(names(reversed)).toEqual([['old'], ['rome', 'oslo']]);
  });

  it('lists every folder under one headerless group by size and by modified', () => {
    const rows = folderRows(tallies, folders);
    const bySize = arrangeFolders(rows, { key: 'size', reverse: false, group: 'folder' as const });
    expect(bySize.map((g) => g.year)).toEqual([null]);
    expect(names(bySize)).toEqual([['oslo', 'rome', 'old']]);
    expect(names(arrangeFolders(rows, { key: 'size', reverse: true, group: 'folder' as const }))).toEqual([['old', 'rome', 'oslo']]);
    expect(names(arrangeFolders(rows, { key: 'modified', reverse: false, group: 'folder' as const }))).toEqual([['rome', 'old', 'oslo']]);
  });

  it('sorts an aliased folder by its alias', () => {
    const aliased = folders.map((f) => (f.id === 3 ? { ...f, alias: 'Aarhus trip' } : f));
    const rows = folderRows(tallies, aliased);
    expect(names(arrangeFolders(rows, { key: 'name', reverse: false, group: 'folder' as const }))).toEqual([['Aarhus trip', 'old', 'rome']]);
  });

  it('sorts names ignoring case and reading numbers', () => {
    const row = (folderId: number, name: string): FolderRow => ({ folderId, name, count: 1, year: 2024, takenAtMin: 0, bytes: 0, modifiedMs: 0 });
    const rows = [row(1, 'Trip 10'), row(2, 'beach'), row(3, 'trip 2'), row(4, 'Attic')];
    expect(names(arrangeFolders(rows, { key: 'name', reverse: false, group: 'folder' as const }))).toEqual([['Attic', 'beach', 'trip 2', 'Trip 10']]);
    expect(names(arrangeFolders(rows, { key: 'name', reverse: true, group: 'folder' as const }))).toEqual([['Trip 10', 'trip 2', 'beach', 'Attic']]);
  });

  it('keeps the grid order between folders that tie, reversed or not', () => {
    // Reversed, the tallies already arrive in the reversed grid's order; ties must keep it.
    const row = (folderId: number, name: string): FolderRow => ({ folderId, name, count: 1, year: 2024, takenAtMin: 0, bytes: 7, modifiedMs: 0 });
    const rows = [row(1, 'c'), row(2, 'a'), row(3, 'b')];
    expect(names(arrangeFolders(rows, { key: 'size', reverse: false, group: 'folder' as const }))).toEqual([['c', 'a', 'b']]);
    expect(names(arrangeFolders(rows, { key: 'size', reverse: true, group: 'folder' as const }))).toEqual([['c', 'a', 'b']]);
    expect(names(arrangeFolders(rows, { key: 'date', reverse: true, group: 'folder' as const }))).toEqual([['c', 'a', 'b']]);
  });

  it('lists nothing, not an empty group, when no folder has photos', () => {
    expect(arrangeFolders([], { key: 'name', reverse: false, group: 'folder' as const })).toEqual([]);
  });
});

describe('enterFolder', () => {
  function spyDeps(view: GridView, setView: (v: GridView) => Promise<void> = () => Promise.resolve()) {
    const order: string[] = [];
    return {
      order,
      deps: {
        cancelSearch: () => order.push('cancel'),
        currentView: () => Promise.resolve(view),
        setView: (v: GridView) => {
          order.push('setView');
          return setView(v);
        },
        jump: () => order.push('jump'),
      },
    };
  }

  it('cancels a pending search before switching the view', async () => {
    const { order, deps } = spyDeps('search');
    await enterFolder(7, deps);
    expect(order).toEqual(['cancel', 'setView', 'jump']);
  });

  it('does not jump until the view switch has settled', async () => {
    let release!: () => void;
    const pending = new Promise<void>((res) => {
      release = res;
    });
    const { order, deps } = spyDeps('starred', () => pending);

    const done = enterFolder(7, deps);
    await Promise.resolve();
    expect(order).toEqual(['cancel', 'setView']);

    release();
    await done;
    expect(order).toEqual(['cancel', 'setView', 'jump']);
  });

  it('skips the view switch when All is already showing', async () => {
    const { order, deps } = spyDeps('all');
    await enterFolder(7, deps);
    expect(order).toEqual(['cancel', 'jump']);
  });

  it('stays in Hidden, whose folders All may not hold at all', async () => {
    const { order, deps } = spyDeps('hidden');
    await enterFolder(7, deps);
    expect(order).toEqual(['cancel', 'jump']);
  });
});

describe('locateItem', () => {
  function spyDeps(view: GridView, at: number | null = 5) {
    const order: string[] = [];
    return {
      order,
      deps: {
        cancelSearch: () => order.push('cancel'),
        currentView: () => Promise.resolve(view),
        setView: (v: GridView) => {
          order.push(`setView:${v}`);
          return Promise.resolve();
        },
        offsetOfItem: (id: number) => {
          order.push(`find:${id}`);
          return Promise.resolve(at);
        },
        select: (offset: number, itemId: number) => order.push(`select:${offset}:${itemId}`),
      },
    };
  }

  it('leaves a subset view for All before looking the photo up, so the offset is against the right index', async () => {
    const { order, deps } = spyDeps('starred');
    await locateItem(42, false, deps);
    expect(order).toEqual(['cancel', 'setView:all', 'find:42', 'select:5:42']);
  });

  it('does not switch views when already in All', async () => {
    const { order, deps } = spyDeps('all');
    await locateItem(42, false, deps);
    expect(order).toEqual(['cancel', 'find:42', 'select:5:42']);
  });

  it('looks for a hidden photo in Hidden, the one view that holds it', async () => {
    const { order, deps } = spyDeps('all');
    await locateItem(42, true, deps);
    expect(order).toEqual(['cancel', 'setView:hidden', 'find:42', 'select:5:42']);
  });

  it('does not switch views when a hidden photo is located from Hidden', async () => {
    const { order, deps } = spyDeps('hidden');
    await locateItem(42, true, deps);
    expect(order).toEqual(['cancel', 'find:42', 'select:5:42']);
  });

  it('selects nothing when the photo is no longer in the library', async () => {
    const { order, deps } = spyDeps('all', null);
    await locateItem(42, false, deps);
    expect(order).toEqual(['cancel', 'find:42']);
  });
});

describe('returnToAll', () => {
  function spyDeps(
    view: GridView,
    remembered: () => Promise<number | null> = () => Promise.resolve(7),
    setView: () => Promise<void> = () => Promise.resolve(),
  ) {
    const order: string[] = [];
    const jumped: number[] = [];
    return {
      order,
      jumped,
      deps: {
        cancelSearch: () => order.push('cancel'),
        currentView: () => Promise.resolve(view),
        setView: (v: GridView) => {
          order.push(`setView:${v}`);
          return setView();
        },
        lastFolder: () => {
          order.push('lastFolder');
          return remembered();
        },
        sortedByDate: () => true,
        jump: (id: number) => {
          order.push('jump');
          jumped.push(id);
        },
      },
    };
  }

  it('leaves the excursion for All and lands on the folder last browsed there', async () => {
    // Starred -> All photos: back where the gallery was left, not at a folder's top or the
    // library's.
    const { order, jumped, deps } = spyDeps('starred');
    await returnToAll(deps);
    expect(order).toEqual(['cancel', 'lastFolder', 'setView:all', 'jump']);
    expect(jumped).toEqual([7]);
  });

  it('leaves the grid where it is when All is already showing', async () => {
    // The user is at their own place already. Reading the remembered folder here would race
    // the write the grid made a moment ago on scrolling into a new folder, and jumping would
    // snap them to a folder's top - so nothing but the search cancel happens.
    const { order, jumped, deps } = spyDeps('all');
    await returnToAll(deps);
    expect(order).toEqual(['cancel']);
    expect(jumped).toEqual([]);
  });

  it('reads the remembered place before switching, and jumps only once the switch settles', async () => {
    // Read first: the grid overwrites the remembered folder with its own top as soon as All
    // shows. Jump last: the folder's offset is only meaningful against All's index.
    let release!: () => void;
    const pending = new Promise<void>((r) => (release = r));
    const { order, deps } = spyDeps('recent', undefined, () => pending);
    const done = returnToAll(deps);
    for (let i = 0; i < 5; i++) await Promise.resolve();
    expect(order).toEqual(['cancel', 'lastFolder', 'setView:all']);
    release();
    await done;
    expect(order).toEqual(['cancel', 'lastFolder', 'setView:all', 'jump']);
  });

  it('opens at the top under a sort other than date, whose grid has no folder place', async () => {
    const { order, jumped, deps } = spyDeps('starred');
    await returnToAll({ ...deps, sortedByDate: () => false });
    expect(order).toEqual(['cancel', 'setView:all']);
    expect(jumped).toEqual([]);
  });

  it('opens at the top when nothing is remembered, or the lookup fails', async () => {
    const none = spyDeps('starred', () => Promise.resolve(null));
    await returnToAll(none.deps);
    expect(none.jumped).toEqual([]);
    const failing = spyDeps('starred', () => Promise.reject(new Error('db busy')));
    await returnToAll(failing.deps);
    expect(failing.jumped).toEqual([]);
  });
});


describe('folderSummary', () => {
  /** Noon on the 15th, local time: far enough inside the month that no time zone moves it. */
  const mid = (year: number, month: number) => new Date(year, month - 1, 15, 12).getTime() / 1000;

  it('says how many photos, and the month the oldest was taken', () => {
    expect(folderSummary(23, mid(2026, 7), 'en-US')).toBe('23 photos · July 2026');
    expect(folderSummary(4, mid(1998, 1), 'en-US')).toBe('4 photos · January 1998');
  });

  it('does not call one photo "photos"', () => {
    expect(folderSummary(1, mid(2026, 7), 'en-US')).toBe('1 photo · July 2026');
  });

  it('groups the digits of a large folder', () => {
    expect(folderSummary(12480, mid(2019, 3), 'en-US')).toBe('12,480 photos · March 2019');
  });

  it('writes the month as the locale does', () => {
    expect(folderSummary(1200, mid(2026, 7), 'de-DE')).toBe('1.200 photos · Juli 2026');
  });

  it("is in the year the sidebar files the folder under, at the year's very edge", () => {
    // Half an hour either side of midnight on New Year's Eve, local time: the header's month
    // and the sidebar's year are read from the same instant in the same zone, so they cannot
    // name different years. Both sides, because which of them is another year in UTC depends
    // on the zone the test runs in: east of Greenwich it is the morning, west of it the night.
    const eve = new Date(2025, 11, 31, 23, 30).getTime() / 1000;
    expect(folderSummary(2, eve, 'en-US')).toBe('2 photos · December 2025');
    expect(yearOf(eve)).toBe(2025);
    const morning = new Date(2026, 0, 1, 0, 30).getTime() / 1000;
    expect(folderSummary(2, morning, 'en-US')).toBe('2 photos · January 2026');
    expect(yearOf(morning)).toBe(2026);
  });
});

describe('photoCount', () => {
  it('counts photos, and does not call one "photos"', () => {
    expect(photoCount(1, 'en-US')).toBe('1 photo');
    expect(photoCount(4210, 'en-US')).toBe('4,210 photos');
  });
});
