import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { GridInfo } from './api';

type Handler = (e: unknown) => void;

const handlers: Record<string, Handler> = {};
const unlistenCounts: Record<string, number> = {};

function makeUnlisten(name: string): () => void {
  return () => {
    unlistenCounts[name] = (unlistenCounts[name] ?? 0) + 1;
  };
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

vi.mock('./api', () => ({
  api: {
    gridInfo: vi.fn(),
    listFolders: vi.fn(),
    gridRows: vi.fn(),
    gridOffsetOfItem: vi.fn(),
    setGridView: vi.fn(),
    setSearchQuery: vi.fn(),
  },
  events: {
    onLibraryChanged: vi.fn((cb: Handler) => {
      handlers.libraryChanged = cb;
      return Promise.resolve(makeUnlisten('libraryChanged'));
    }),
    onFolderStatus: vi.fn((cb: Handler) => {
      handlers.folderStatus = cb;
      return Promise.resolve(makeUnlisten('folderStatus'));
    }),
    onScanProgress: vi.fn((cb: Handler) => {
      handlers.scanProgress = cb;
      return Promise.resolve(makeUnlisten('scanProgress'));
    }),
  },
  errorMessage: (e: unknown) => (e instanceof Error ? e.message : String(e)),
}));

import { api, events } from './api';
import { LibraryStore } from './library.svelte';

describe('LibraryStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    for (const k of Object.keys(handlers)) delete handlers[k];
    for (const k of Object.keys(unlistenCounts)) delete unlistenCounts[k];
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 1,
      len: 0,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });
    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [] });
  });

  it('routes background refresh failures from event handlers into reportError instead of throwing unhandled', async () => {
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.gridInfo).mockRejectedValueOnce(new Error('boom'));
    handlers.libraryChanged({ version: 2, len: 0 });
    await Promise.resolve();
    await Promise.resolve();

    expect(store.errors).toHaveLength(1);
    expect(store.errors[0]?.message).toBe('boom');
  });

  it('routes folder-status and done scan-progress failures into reportError too', async () => {
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.listFolders).mockRejectedValueOnce(new Error('folders-fail'));
    handlers.folderStatus({ watchedId: 1, online: false, degraded: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.errors.some((t) => t.message === 'folders-fail')).toBe(true);

    vi.mocked(api.listFolders).mockRejectedValueOnce(new Error('scan-done-fail'));
    handlers.scanProgress({ watchedId: 1, filesSeen: 1, added: 0, changed: 0, done: true, cancelled: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.errors.some((t) => t.message === 'scan-done-fail')).toBe(true);
  });

  it('tracks anyDegraded from folder-status events, filtered to currently-watched ids', async () => {
    vi.mocked(api.listFolders).mockResolvedValue({
      watched: [
        { id: 1, path: '/a', online: true },
        { id: 2, path: '/b', online: true },
      ],
      folders: [],
    });
    const store = new LibraryStore();
    await store.init();

    expect(store.anyDegraded).toBe(false);

    handlers.folderStatus({ watchedId: 1, online: true, degraded: true });
    expect(store.anyDegraded).toBe(true);

    // id 2 is unrelated and still fine
    handlers.folderStatus({ watchedId: 2, online: true, degraded: false });
    expect(store.anyDegraded).toBe(true);

    handlers.folderStatus({ watchedId: 1, online: true, degraded: false });
    expect(store.anyDegraded).toBe(false);
  });

  it('stops reporting a folder as degraded once it is removed, even though removal emits no folder-status event', async () => {
    vi.mocked(api.listFolders).mockResolvedValue({
      watched: [{ id: 1, path: '/a', online: true }],
      folders: [],
    });
    const store = new LibraryStore();
    await store.init();

    handlers.folderStatus({ watchedId: 1, online: true, degraded: true });
    expect(store.anyDegraded).toBe(true);

    // Engine::remove_folder emits no folder-status event for the removed id - only a
    // later folder-list refresh (triggered here directly, as `refreshFolders` normally
    // would be after a library-changed/scan-progress event) reflects the removal. Without
    // filtering `anyDegraded` against the current watched list, the stale `true` entry for
    // id 1 would make this notice never clear.
    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [] });
    await store.refreshFolders();

    expect(store.anyDegraded).toBe(false);
  });

  it('keeps the selection on its photo when the grid is renumbered', async () => {
    // A grid offset only means "this photo" against one version of the index: a scan that
    // indexes a photo into an earlier folder shifts every later offset by one. Clamping
    // catches only the offset falling off the end; in range the selection silently slides
    // onto the next photo.
    const entry = (id: number) => ({
      id,
      folderId: 1,
      takenAt: 0,
      aspect: 1,
      kind: 'image' as const,
      thumbKey: '0',
      starred: false,
    });
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 1,
      len: 2,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });
    vi.mocked(api.gridRows).mockResolvedValue({ version: 1, rows: [entry(10), entry(11)] });
    const store = new LibraryStore();
    await store.init();
    await store.ensure(0, 2);
    store.selected = 1;

    // A photo appears ahead of it, so the one that was at offset 1 is now at offset 2.
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 2,
      len: 3,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });
    vi.mocked(api.gridOffsetOfItem).mockResolvedValue(2);
    await store.refresh();

    expect(api.gridOffsetOfItem).toHaveBeenCalledWith(11);
    expect(store.selected).toBe(2);
  });

  it('a selection made by id is re-found after a rebuild even when its page was never loaded', async () => {
    // "Locate in photon" and closing the viewer both know the photo's id but land on an
    // offset whose page the grid has not fetched yet. Recording only the offset would leave
    // nothing to re-find by, and the next scan would slide the selection onto a neighbour.
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 1,
      len: 5,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });
    const store = new LibraryStore();
    await store.init();
    store.selectItem(3, 11);
    expect(store.selected).toBe(3);

    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 2,
      len: 6,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });
    vi.mocked(api.gridOffsetOfItem).mockResolvedValue(4);
    await store.refresh();

    expect(api.gridOffsetOfItem).toHaveBeenCalledWith(11);
    expect(store.selected).toBe(4);
  });

  it('clamps a selection it cannot re-find into the shrunken grid', async () => {
    // The fallback: nothing was ever loaded at that offset, so there is no id to follow.
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 1,
      len: 5,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });
    const store = new LibraryStore();
    await store.init();
    store.selected = 4;

    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 2,
      len: 2,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });
    await store.refresh();
    expect(store.selected).toBe(1);
    expect(api.gridOffsetOfItem).not.toHaveBeenCalled();

    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 3,
      len: 0,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });
    await store.refresh();
    expect(store.selected).toBeNull();
  });

  it('ignores a folder list that arrives after a later one', async () => {
    // `refreshFolders` has three callers that can overlap. Removing a watched folder
    // mid-scan emits a final `done: true` whose listener fires one of them, which can read
    // the database before the deletion commits; `FolderTree.remove` awaits its own. If the
    // first resolves last, the deleted root comes back in the sidebar - with a working
    // context menu - until some unrelated event happens to refresh it again.
    vi.mocked(api.listFolders).mockResolvedValue({
      watched: [{ id: 1, path: '/a', online: true }],
      folders: [],
    });
    const store = new LibraryStore();
    await store.init();

    const stale = deferred<{ watched: { id: number; path: string; online: boolean }[]; folders: [] }>();
    vi.mocked(api.listFolders).mockReturnValueOnce(stale.promise as never);
    const inFlight = store.refreshFolders();

    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [] });
    await store.refreshFolders();
    expect(store.folders.watched).toEqual([]);

    // The scan-done refresh answers last, with what the database held before the removal.
    stale.resolve({ watched: [{ id: 1, path: '/a', online: true }], folders: [] });
    await inFlight;

    expect(store.folders.watched).toEqual([]);
  });

  it('is idempotent: a second init() call never registers more than the three subscriptions', async () => {
    const store = new LibraryStore();
    const first = store.init();
    const second = store.init();
    expect(second).toBe(first);
    await first;

    await store.init();

    expect(events.onLibraryChanged).toHaveBeenCalledTimes(1);
    expect(events.onFolderStatus).toHaveBeenCalledTimes(1);
    expect(events.onScanProgress).toHaveBeenCalledTimes(1);
  });

  it('unsubscribes cleanly when dispose() runs before init() finishes subscribing', async () => {
    const gridInfoGate = deferred<{
      version: number;
      len: number;
      sections: never[];
      starredCount: number;
      view: 'all';
      searchQuery: string;
    }>();
    vi.mocked(api.gridInfo).mockReturnValueOnce(gridInfoGate.promise);

    // Control when onLibraryChanged resolves so dispose() can race init().
    const listenGate = deferred<void>();
    vi.mocked(events.onLibraryChanged).mockImplementationOnce((cb) => {
      handlers.libraryChanged = cb as Handler;
      return listenGate.promise.then(() => makeUnlisten('libraryChanged'));
    });

    const store = new LibraryStore();
    const initPromise = store.init();
    store.dispose();
    listenGate.resolve();
    gridInfoGate.resolve({ version: 1, len: 0, sections: [], starredCount: 0, view: 'all', searchQuery: '' });
    await initPromise;

    expect(unlistenCounts.libraryChanged).toBe(1);
    expect(unlistenCounts.folderStatus).toBe(1);
    expect(unlistenCounts.scanProgress).toBe(1);
  });

  it('setView switches the backend view and only resolves once the refreshed grid has landed', async () => {
    const store = new LibraryStore();
    await store.init();

    const refreshGate = deferred<GridInfo>();
    vi.mocked(api.setGridView).mockResolvedValue(undefined);
    vi.mocked(api.gridInfo).mockReturnValueOnce(refreshGate.promise);

    let resolved = false;
    const setViewPromise = store.setView('starred').then(() => {
      resolved = true;
    });

    // The command has been issued, but the grid snapshot behind it hasn't arrived yet:
    // setView must still be pending, not resolved early off the command call alone (the
    // race Important 1 warned about, pinned at the unit that owns it).
    await Promise.resolve();
    await Promise.resolve();
    expect(api.setGridView).toHaveBeenCalledWith('starred');
    expect(resolved).toBe(false);

    refreshGate.resolve({ version: 2, len: 0, sections: [], starredCount: 0, view: 'starred', searchQuery: '' });
    await setViewPromise;

    expect(resolved).toBe(true);
    expect(store.info.view).toBe('starred');
  });

  it('routes a setGridView failure into reportError instead of throwing', async () => {
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.setGridView).mockRejectedValueOnce(new Error('set-view-fail'));

    await expect(store.setView('starred')).resolves.toBeUndefined();
    expect(store.errors.some((t) => t.message === 'set-view-fail')).toBe(true);
  });

  it('setSearchQuery("") issues the command and refreshes, restoring the All view', async () => {
    // Spec §7: "clearing the box restores the All view." The engine-level behaviour behind
    // this is already covered on the Rust side; this pins the store's half of the path.
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.setSearchQuery).mockResolvedValue(undefined);
    vi.mocked(api.gridInfo).mockResolvedValueOnce({
      version: 2,
      len: 2,
      sections: [],
      starredCount: 0,
      view: 'all',
      searchQuery: '',
    });

    await store.setSearchQuery('');

    expect(api.setSearchQuery).toHaveBeenCalledWith('');
    expect(store.info.view).toBe('all');
    expect(store.info.searchQuery).toBe('');
  });

  it('applies setSearchQuery calls in the order they were issued, not the order their IPC round trips finish', async () => {
    // A pending call from a previous, slower request landing after a later one would put
    // the backend's query out of sync with what the box last asked for (Fix 3: cancel()
    // only stops a call that hasn't fired, so already-dispatched calls must be serialised
    // here instead).
    const store = new LibraryStore();
    await store.init();

    const order: string[] = [];
    const first = deferred<void>();
    const second = deferred<void>();
    vi.mocked(api.setSearchQuery).mockImplementationOnce(async (q: string) => {
      order.push(`start:${q}`);
      await first.promise;
      order.push(`end:${q}`);
    });
    vi.mocked(api.setSearchQuery).mockImplementationOnce(async (q: string) => {
      order.push(`start:${q}`);
      await second.promise;
      order.push(`end:${q}`);
    });
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 2,
      len: 0,
      sections: [],
      starredCount: 0,
      view: 'search',
      searchQuery: 'beach',
    });

    const p1 = store.setSearchQuery('b');
    const p2 = store.setSearchQuery('beach');

    // Resolve the second (later-issued) call's IPC first — if calls weren't serialised,
    // its effects would land before the first call's.
    second.resolve();
    await Promise.resolve();
    await Promise.resolve();
    first.resolve();
    await Promise.all([p1, p2]);

    expect(order).toEqual(['start:b', 'end:b', 'start:beach', 'end:beach']);
  });
});
