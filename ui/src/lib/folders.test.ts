import { describe, expect, it } from 'vitest';
import type { Folder, GridView } from './api';
import { enterFolder, locateItem, folderRows, groupByYear, returnToAll } from './folders';

/** Seconds since the epoch, since that is what `takenAtMin` carries. */
const at = (iso: string) => Math.floor(new Date(iso).getTime() / 1000);

const folders = [
  { id: 1, watchedId: 1, parentId: null, path: '/photos', name: 'photos', hidden: false },
  { id: 2, watchedId: 1, parentId: 1, path: '/photos/rome', name: 'rome', hidden: false },
  { id: 3, watchedId: 1, parentId: 1, path: '/photos/oslo', name: 'oslo', hidden: false },
  { id: 4, watchedId: 1, parentId: 1, path: '/photos/old', name: 'old', hidden: false },
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

  it('lists a folder once however many runs of it the view holds', () => {
    // Recent orders photos by date across folders, so one folder comes back as many runs —
    // one per stretch where its dates are the newest. One row per section listed the same
    // folder hundreds of times, each claiming a single photo; the sidebar has to show the
    // folder once, with the photos it contributes and the oldest of them deciding its year.
    const interleaved = [
      { folderId: 2, offset: 0, count: 1, takenAtMin: at('2024-06-05T12:00:00') },
      { folderId: 3, offset: 1, count: 1, takenAtMin: at('2024-06-04T12:00:00') },
      { folderId: 2, offset: 2, count: 2, takenAtMin: at('2024-06-01T12:00:00') },
      { folderId: 3, offset: 4, count: 1, takenAtMin: at('2023-12-31T12:00:00') },
    ];
    expect(folderRows(interleaved, folders)).toEqual([
      { folderId: 2, name: 'rome', count: 3, year: 2024, takenAtMin: at('2024-06-01T12:00:00') },
      { folderId: 3, name: 'oslo', count: 2, year: 2023, takenAtMin: at('2023-12-31T12:00:00') },
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

describe('enterFolder', () => {
  function spyDeps(view: GridView, setView: (v: GridView) => Promise<void> = () => Promise.resolve()) {
    const order: string[] = [];
    return {
      order,
      deps: {
        cancelSearch: () => order.push('cancel'),
        currentView: () => view,
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
        currentView: () => view,
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
        currentView: () => view,
        setView: (v: GridView) => {
          order.push(`setView:${v}`);
          return setView();
        },
        lastFolder: () => {
          order.push('lastFolder');
          return remembered();
        },
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

  it('opens at the top when nothing is remembered, or the lookup fails', async () => {
    const none = spyDeps('starred', () => Promise.resolve(null));
    await returnToAll(none.deps);
    expect(none.jumped).toEqual([]);
    const failing = spyDeps('starred', () => Promise.reject(new Error('db busy')));
    await returnToAll(failing.deps);
    expect(failing.jumped).toEqual([]);
  });
});

