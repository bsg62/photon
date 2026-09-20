import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { GridInfo, GridView, Section } from './api';

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
    listAlbums: vi.fn(),
    listPeople: vi.fn(),
    listTags: vi.fn(),
    setAlbumView: vi.fn(),
    createAlbum: vi.fn(),
    renameTag: vi.fn(),
    hideTag: vi.fn(),
    restoreTagRule: vi.fn(),
    watchedFolderStats: vi.fn(),
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
    onExportProgress: vi.fn((cb: Handler) => {
      handlers.exportProgress = cb;
      return Promise.resolve(makeUnlisten('exportProgress'));
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
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
    });
    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [] });
    vi.mocked(api.listAlbums).mockResolvedValue([]);
    vi.mocked(api.listPeople).mockResolvedValue([]);
    vi.mocked(api.listTags).mockResolvedValue([]);
    vi.mocked(api.watchedFolderStats).mockResolvedValue([]);
  });

  it('snapshots the folder’s photo count when a scan starts, net of what it has already added', async () => {
    const store = new LibraryStore();
    await store.init();
    vi.mocked(api.watchedFolderStats).mockResolvedValue([{ watchedId: 1, photoCount: 5_000 }]);

    // The first event of a scan arrives after its first batch: 200 of the 5,200 rows the
    // stats now report were added by this very scan, so 5,000 is what it started with.
    handlers.scanProgress({ watchedId: 1, filesSeen: 200, added: 200, changed: 0, done: false, cancelled: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(api.watchedFolderStats).toHaveBeenCalledTimes(1);
    expect(store.expected[1]).toBe(4_800);

    // Later ticks of the same scan do not re-snapshot: the count would drift upwards with
    // every batch and the bar would never reach the end.
    handlers.scanProgress({ watchedId: 1, filesSeen: 900, added: 300, changed: 0, done: false, cancelled: false });
    await Promise.resolve();
    expect(api.watchedFolderStats).toHaveBeenCalledTimes(1);

    // A brand-new folder: everything counted so far is this scan's own work.
    vi.mocked(api.watchedFolderStats).mockResolvedValue([{ watchedId: 2, photoCount: 150 }]);
    handlers.scanProgress({ watchedId: 2, filesSeen: 150, added: 150, changed: 0, done: false, cancelled: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.expected[2]).toBeUndefined();

    // The next scan of folder 1 snapshots afresh.
    handlers.scanProgress({ watchedId: 1, filesSeen: 5_300, added: 300, changed: 0, done: true, cancelled: false });
    vi.mocked(api.watchedFolderStats).mockResolvedValue([{ watchedId: 1, photoCount: 5_300 }]);
    handlers.scanProgress({ watchedId: 1, filesSeen: 10, added: 0, changed: 0, done: false, cancelled: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.expected[1]).toBe(5_300);
  });

  it('refetches albums, people and tags on every library change and after an album mutation', async () => {
    const store = new LibraryStore();
    await store.init();
    expect(api.listAlbums).toHaveBeenCalledTimes(1);

    vi.mocked(api.listAlbums).mockResolvedValue([{ id: 1, name: 'Trip', count: 2 }]);
    vi.mocked(api.listPeople).mockResolvedValue([{ hash: 'abc', name: 'Ada', count: 1 }]);
    vi.mocked(api.listTags).mockResolvedValue([{ tag: 'beach', count: 3 }]);
    handlers.libraryChanged({ version: 2, len: 0 });
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(store.albums).toEqual([{ id: 1, name: 'Trip', count: 2 }]);
    expect(store.people[0]?.name).toBe('Ada');
    expect(store.tags[0]?.tag).toBe('beach');
    expect(store.albumName(1)).toBe('Trip');
    expect(store.personName('abc')).toBe('Ada');
    expect(store.albumName(99)).toBe('');

    vi.mocked(api.createAlbum).mockResolvedValue({ id: 2, name: 'Zoo', createdMs: 0 });
    vi.mocked(api.listAlbums).mockResolvedValue([
      { id: 1, name: 'Trip', count: 2 },
      { id: 2, name: 'Zoo', count: 0 },
    ]);
    await expect(store.createAlbum('Zoo')).resolves.toBe(2);
    expect(api.createAlbum).toHaveBeenCalledWith('Zoo');
    expect(store.albums).toHaveLength(2);
  });

  it('tag rule changes refetch the collections', async () => {
    const store = new LibraryStore();
    await store.init();
    vi.mocked(api.listTags).mockResolvedValue([{ tag: 'vacation', count: 1 }]);
    vi.mocked(api.renameTag).mockResolvedValue();
    await store.renameTag('holiday', 'vacation');
    expect(api.renameTag).toHaveBeenCalledWith('holiday', 'vacation');
    expect(store.tags).toEqual([{ tag: 'vacation', count: 1 }]);

    vi.mocked(api.listTags).mockResolvedValue([]);
    vi.mocked(api.hideTag).mockResolvedValue();
    await store.hideTag('vacation');
    expect(api.hideTag).toHaveBeenCalledWith('vacation');
    expect(store.tags).toEqual([]);

    vi.mocked(api.listTags).mockResolvedValue([{ tag: 'holiday', count: 1 }]);
    vi.mocked(api.restoreTagRule).mockResolvedValue();
    await store.restoreTagRule('holiday');
    expect(api.restoreTagRule).toHaveBeenCalledWith('holiday');
    expect(store.tags).toEqual([{ tag: 'holiday', count: 1 }]);
  });

  it('a tag change that saved is not reported as failed when the refetch fails', async () => {
    const store = new LibraryStore();
    await store.init();
    vi.mocked(api.renameTag).mockResolvedValue();
    vi.mocked(api.listTags).mockRejectedValueOnce(new Error('tags-fail'));
    await expect(store.renameTag('holiday', 'vacation')).resolves.toBeUndefined();
    expect(store.toasts.some((t) => t.message === 'tags-fail')).toBe(true);
  });

  it('a failed collections fetch is reported, not thrown', async () => {
    const store = new LibraryStore();
    await store.init();
    vi.mocked(api.listAlbums).mockRejectedValueOnce(new Error('albums-fail'));
    handlers.libraryChanged({ version: 2, len: 0 });
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(store.toasts.some((t) => t.message === 'albums-fail')).toBe(true);
  });

  it('routes background refresh failures from event handlers into reportError instead of throwing unhandled', async () => {
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.gridInfo).mockRejectedValueOnce(new Error('boom'));
    handlers.libraryChanged({ version: 2, len: 0 });
    await Promise.resolve();
    await Promise.resolve();

    expect(store.toasts).toHaveLength(1);
    expect(store.toasts[0]?.message).toBe('boom');
  });

  it('routes folder-status and done scan-progress failures into reportError too', async () => {
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.listFolders).mockRejectedValueOnce(new Error('folders-fail'));
    handlers.folderStatus({ watchedId: 1, online: false, degraded: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.toasts.some((t) => t.message === 'folders-fail')).toBe(true);

    vi.mocked(api.listFolders).mockRejectedValueOnce(new Error('scan-done-fail'));
    handlers.scanProgress({ watchedId: 1, filesSeen: 1, added: 0, changed: 0, done: true, cancelled: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.toasts.some((t) => t.message === 'scan-done-fail')).toBe(true);
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
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
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
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
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
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
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
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
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
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
    });
    const store = new LibraryStore();
    await store.init();
    store.selected = 4;

    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 2,
      len: 2,
      sections: [],
      starredCount: 0,
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
    });
    await store.refresh();
    expect(store.selected).toBe(1);
    expect(api.gridOffsetOfItem).not.toHaveBeenCalled();

    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 3,
      len: 0,
      sections: [],
      starredCount: 0,
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
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
    expect(events.onExportProgress).toHaveBeenCalledTimes(1);
  });

  /** The bar the user watches while an export runs. It has to clear at the end whatever
   *  happened on the way, including an export where every photo failed. */
  it('shows export progress while one is running and clears it at the end', async () => {
    const store = new LibraryStore();
    await store.init();

    handlers.exportProgress({ done: 3, total: 12, failed: 0 });
    expect(store.exporting).toEqual({ done: 3, total: 12, failed: 0 });

    handlers.exportProgress({ done: 12, total: 12, failed: 12 });
    expect(store.exporting).toBe(null);
  });

  it('unsubscribes cleanly when dispose() runs before init() finishes subscribing', async () => {
    const gridInfoGate = deferred<{
      version: number;
      len: number;
      sections: never[];
      starredCount: number;
      duplicateCount: number;
      view: 'all';
      searchQuery: string;
      person: null;
      album: null;
      tag: null;
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
    gridInfoGate.resolve({ version: 1, len: 0, sections: [], starredCount: 0, duplicateCount: 0, view: 'all', searchQuery: '', person: null, album: null, tag: null });
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

    refreshGate.resolve({ version: 2, len: 0, sections: [], starredCount: 0, duplicateCount: 0, view: 'starred', searchQuery: '', person: null, album: null, tag: null });
    await setViewPromise;

    expect(resolved).toBe(true);
    expect(store.info.view).toBe('starred');
  });

  it('routes a setGridView failure into reportError instead of throwing', async () => {
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.setGridView).mockRejectedValueOnce(new Error('set-view-fail'));

    await expect(store.setView('starred')).resolves.toBeUndefined();
    expect(store.toasts.some((t) => t.message === 'set-view-fail')).toBe(true);
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
      duplicateCount: 0,
      view: 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
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
      duplicateCount: 0,
      view: 'search',
      searchQuery: 'beach',
      person: null,
      album: null,
      tag: null,
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

  describe('multi-selection', () => {
    /** Photo ids are offset + 100, so a wrong offset is visible in the failure message. */
    const idAt = (offset: number) => offset + 100;
    const entryAt = (offset: number) => ({
      id: idAt(offset),
      folderId: 1,
      takenAt: 0,
      aspect: 1,
      kind: 'image' as const,
      thumbKey: '0',
      starred: false,
    });

    /** A store over `len` photos, with every page answerable. `sections` and `view` matter
     *  only to select-all, which reads the folder the lead is in. */
    async function storeOf(len: number, opts: { sections?: Section[]; view?: GridView } = {}) {
      vi.mocked(api.gridInfo).mockResolvedValue({
        version: 1,
        len,
        sections: opts.sections ?? [],
        starredCount: 0,
        duplicateCount: 0,
        view: opts.view ?? 'all',
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
      });
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 1,
        rows: Array.from({ length: Math.min(count, len - offset) }, (_, i) => entryAt(offset + i)),
      }));
      const store = new LibraryStore();
      await store.init();
      await store.ensure(0, Math.min(len, 50));
      return store;
    }

    it('ctrl+click toggles a photo in and out, moving the lead each time', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      expect(store.selectedItemIds).toEqual([idAt(2)]);

      store.toggleSelected(5);
      expect([...store.selectedItemIds].sort()).toEqual([idAt(2), idAt(5)].sort());
      expect(store.selected).toBe(5);

      store.toggleSelected(5);
      expect(store.selectedItemIds).toEqual([idAt(2)]);
      expect(store.selected).toBe(5);
    });

    it('a plain selection replaces the whole set', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      store.toggleSelected(5);
      store.selected = 7;
      expect(store.selectedItemIds).toEqual([idAt(7)]);
      expect(store.selectionCount).toBe(1);
    });

    describe('a rubber band', () => {
      it('previews from the loaded pages and replaces the selection', async () => {
        const store = await storeOf(20);
        store.selected = 15;

        store.beginBand(false);
        store.bandTo([[2, 4]]);

        expect([...store.selectedItemIds].sort()).toEqual([idAt(2), idAt(3), idAt(4)].sort());
        expect(store.isSelected(idAt(15))).toBe(false);
      });

      /** Ctrl/Cmd or Shift while dragging adds to what was already selected. The base is
       *  captured at `beginBand`, so dragging the band smaller takes photos back off rather
       *  than piling every frame's worth on top of the last. */
      it('adds to the selection it started with, and shrinks back again', async () => {
        const store = await storeOf(20);
        store.selected = 15;

        store.beginBand(true);
        store.bandTo([[2, 4]]);
        store.bandTo([[2, 3]]);

        expect([...store.selectedItemIds].sort()).toEqual(
          [idAt(15), idAt(2), idAt(3)].sort(),
        );
      });

      it('puts the selection back when the drag is abandoned', async () => {
        const store = await storeOf(20);
        store.selected = 15;

        store.beginBand(false);
        store.bandTo([[2, 4]]);
        store.cancelBand();

        expect(store.selectedItemIds).toEqual([idAt(15)]);
        expect(store.selected).toBe(15);
      });

      /** The preview can only see pages that have arrived; the release asks the backend,
       *  which is what makes a band over a still-loading tile come out right. */
      it('fills in a tile the preview could not see when the drag ends', async () => {
        const store = await storeOf(2000);
        // Offsets past the first 50 were never fetched, so no page holds them.
        store.beginBand(false);
        store.bandTo([[1200, 1202]]);
        expect(store.selectionCount).toBe(0);

        await store.endBand([[1200, 1202]]);

        expect(store.selectionCount).toBe(3);
        expect(store.isSelected(idAt(1201))).toBe(true);
      });

      it('lands the lead and the anchor on the first photo of the band', async () => {
        const store = await storeOf(20);
        store.beginBand(false);
        await store.endBand([[4, 6]]);

        expect(store.selected).toBe(4);
        // A shift+click afterwards extends from where the band began, not from nothing.
        await store.extendSelection(8);
        expect(store.selectionCount).toBe(5);
      });

      it('writes nothing when the grid is rebuilt under the drag', async () => {
        const store = await storeOf(20);
        store.selected = 15;
        store.beginBand(false);
        vi.mocked(api.gridRows).mockResolvedValueOnce({
          version: 2,
          rows: [entryAt(4), entryAt(5)],
        });

        await store.endBand([[4, 5]]);

        expect(store.selectedItemIds).toEqual([idAt(15)]);
      });
    });

    it('shift+click selects the range from the anchor, chunking past MAX_ROWS', async () => {
      // 1301 > MAX_ROWS (1000): one un-chunked grid_rows call is silently truncated by
      // clamp_count, and the range's last three hundred photos would go unselected with
      // no error anywhere.
      const store = await storeOf(1500);
      store.selected = 100;
      vi.mocked(api.gridRows).mockClear(); // only this call's fetches count below

      await store.extendSelection(1400);

      expect(store.selectionCount).toBe(1301);
      expect(store.isSelected(idAt(1400))).toBe(true);
      expect(store.isSelected(idAt(99))).toBe(false);
      expect(vi.mocked(api.gridRows).mock.calls.filter(([, count]) => count > 1000)).toEqual([]);
      expect(store.selected).toBe(1400);
    });

    it('shift+click twice re-ranges from the same anchor', async () => {
      const store = await storeOf(20);
      store.selected = 5;
      await store.extendSelection(10);
      await store.extendSelection(7);
      expect(store.selectionCount).toBe(3);
      expect(store.isSelected(idAt(10))).toBe(false);
      expect(store.isSelected(idAt(5))).toBe(true);
    });

    it('a later extendSelection call wins even if an earlier, wider one resolves after it', async () => {
      // Two fast Shift+clicks issue overlapping calls with no ordering guarantee on their
      // fetches. Without a per-call guard, whichever fetch's last chunk happens to resolve
      // last would win - here that is the wide range, issued first but still awaiting its
      // gridRows call when the narrow range, issued second, has already finished.
      const store = await storeOf(20);
      store.selected = 0;

      const wideChunk = deferred<{ version: number; rows: ReturnType<typeof entryAt>[] }>();
      vi.mocked(api.gridRows).mockImplementationOnce(() => wideChunk.promise);

      const wide = store.extendSelection(15); // its one chunk is now stuck on wideChunk
      const narrow = store.extendSelection(2); // falls through to the default mock, resolves fast
      await narrow;

      expect(store.selectionCount).toBe(3); // the narrow range has already landed
      wideChunk.resolve({ version: 1, rows: Array.from({ length: 16 }, (_, i) => entryAt(i)) });
      await wide;

      expect(store.selectionCount).toBe(3);
      expect(store.selected).toBe(2);
      expect(store.isSelected(idAt(15))).toBe(false);
    });

    it('extends backwards from the anchor too', async () => {
      const store = await storeOf(20);
      store.selected = 10;
      await store.extendSelection(6);
      expect(store.selectionCount).toBe(5);
      expect(store.isSelected(idAt(6))).toBe(true);
      expect(store.isSelected(idAt(11))).toBe(false);
    });

    it('keeps the selection across a refresh that shifts every offset', async () => {
      // The test that discriminates ids from offsets. A scan indexing a photo into an
      // earlier folder moves every later offset by one; an offset-based selection would
      // silently ring - and star - the neighbours of what the user picked.
      const store = await storeOf(10);
      store.selected = 3;
      store.toggleSelected(4);

      // One photo appears ahead of them all, so every old offset is now one later.
      vi.mocked(api.gridInfo).mockResolvedValue({
        version: 2,
        len: 11,
        sections: [],
        starredCount: 0,
        duplicateCount: 0,
        view: 'all',
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
      });
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(5);
      await store.refresh();

      expect([...store.selectedItemIds].sort((a, b) => a - b)).toEqual([idAt(3), idAt(4)]);
      expect(store.selected).toBe(5);
    });

    it('discards rows fetched against a newer index than the offsets they were asked for', async () => {
      // `this.info.version` guard alone is not enough: it only catches a refresh the
      // library-changed listener has already applied. Here the backend has published a
      // newer index but that listener has not run yet, so `version` still matches while the
      // ids `gridRows` answers with belong to the new index - meaningless for the offsets
      // this call asked for.
      const store = await storeOf(20);
      store.selected = 0;

      vi.mocked(api.gridRows).mockResolvedValueOnce({ version: 2, rows: [entryAt(0), entryAt(1)] });

      await store.extendSelection(1);

      expect(store.selectionCount).toBe(1);
      expect(store.isSelected(idAt(0))).toBe(true);
      expect(store.selected).toBe(0);
    });

    it('isSelectedTile rings the lead even when its page has not loaded', async () => {
      // storeOf only ensures the first 50 offsets, but pages are fetched whole (PAGE_SIZE =
      // 200), so offset 250 - in the second page - is never cached. The plain `selected`
      // setter can only record an id for a loaded page, so `selection` is empty here -
      // `isSelectedTile` has to fall back to comparing offsets directly, or the tile the
      // keyboard is actually on goes unringed.
      const store = await storeOf(500);
      store.selected = 250;

      expect(store.entry(250)).toBeUndefined();
      expect(store.isSelectedTile(250, undefined)).toBe(true);
      expect(store.isSelectedTile(249, undefined)).toBe(false);
    });

    it('carries the anchor with the lead across a refresh that shifts every offset', async () => {
      // Without this, a Shift+click after the rebuild would range from the stale offset - one
      // photo off from where the user actually clicked - and the user would star the wrong
      // range without anything looking wrong.
      const store = await storeOf(20);
      store.selected = 3; // anchor = 3

      vi.mocked(api.gridInfo).mockResolvedValue({
        version: 2,
        len: 21,
        sections: [],
        starredCount: 0,
        duplicateCount: 0,
        view: 'all',
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
      });
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(4);
      // The rebuilt index answers version 2 now, so the range fetched below must match it too.
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 2,
        rows: Array.from({ length: Math.min(count, 20 - offset) }, (_, i) => entryAt(offset + i)),
      }));
      await store.refresh();
      expect(store.selected).toBe(4);

      await store.extendSelection(10);
      expect(store.selectionCount).toBe(7); // 4..10, not 3..10
      expect(store.isSelected(idAt(4))).toBe(true);
      expect(store.isSelected(idAt(3))).toBe(false);
    });

    it('drops a stale anchor when a rebuild had no lead to re-find it by', async () => {
      // Ctrl+click deselecting the last-selected tile nulls the lead but leaves the anchor
      // at that offset (`toggleSelected`). If a rebuild then finds no id to rebind, and
      // does not also clear the anchor, a Shift+click with no plain click in between ranges
      // from that stale, pre-shift offset instead of falling back to the lead.
      const store = await storeOf(10);
      store.selected = 2; // lead = 2, anchor = 2
      store.toggleSelected(2); // deselects it: lead = null, anchor stays 2

      // One photo appears ahead of them all, so every old offset is now one later - but
      // there is no lead id for rebindSelection to re-find, so it never learns that.
      vi.mocked(api.gridInfo).mockResolvedValue({
        version: 2,
        len: 11,
        sections: [],
        starredCount: 0,
        duplicateCount: 0,
        view: 'all',
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
      });
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 2,
        rows: Array.from({ length: Math.min(count, 11 - offset) }, (_, i) => entryAt(offset + i)),
      }));
      await store.refresh();
      expect(store.selected).toBeNull();

      await store.extendSelection(5);

      // With a live anchor, extendSelection falls back to `this.selectedOffset ?? 0`, i.e.
      // the same range a user's very first Shift+click would get: 0..5. A stale anchor of 2
      // would instead range 2..5, four photos short.
      expect(store.selectionCount).toBe(6);
      expect(store.isSelected(idAt(0))).toBe(true);
      expect(store.isSelected(idAt(5))).toBe(true);
    });

    it('a view switch clears the selection', async () => {
      const store = await storeOf(10);
      store.selected = 3;
      store.toggleSelected(4);
      vi.mocked(api.setGridView).mockResolvedValue(undefined);

      await store.setView('starred');

      expect(store.selectionCount).toBe(0);
      expect(store.selected).toBeNull();
    });

    it('selectItem collapses only when it names a different photo', async () => {
      const store = await storeOf(10);
      store.selected = 3;
      store.toggleSelected(4);

      store.selectItem(4, idAt(4)); // the viewer closing on the photo it opened with
      expect(store.selectionCount).toBe(2);

      store.selectItem(8, idAt(8)); // the viewer navigated away and closed there
      expect(store.selectedItemIds).toEqual([idAt(8)]);
    });

    describe('select all', () => {
      /** Three folders: 0..4, 5..11, 12..14. */
      const folders: Section[] = [
        { folderId: 1, offset: 0, count: 5, takenAtMin: 0 },
        { folderId: 2, offset: 5, count: 7, takenAtMin: 0 },
        { folderId: 3, offset: 12, count: 3, takenAtMin: 0 },
      ];

      it('takes the folder the lead is in, not the whole library', async () => {
        // The point of the key on a fifty-thousand photo library: All is not a result set,
        // so "all" is the folder being looked at.
        const store = await storeOf(15, { sections: folders });
        store.selected = 7;

        await store.selectAll();

        expect([...store.selectedItemIds].sort((a, b) => a - b)).toEqual(
          [5, 6, 7, 8, 9, 10, 11].map(idAt),
        );
        expect(store.isSelected(idAt(4))).toBe(false);
        expect(store.isSelected(idAt(12))).toBe(false);
        expect(store.selected).toBe(7);
      });

      it('takes the first folder when nothing is selected yet', async () => {
        const store = await storeOf(15, { sections: folders });

        await store.selectAll();

        expect(store.selectionCount).toBe(5);
        expect(store.isSelected(idAt(0))).toBe(true);
        expect(store.isSelected(idAt(5))).toBe(false);
        expect(store.selected).toBe(0);
      });

      it('takes the whole view outside the library view', async () => {
        // Search results, an album, a person, a tag, Recent: each is already a set the user
        // asked for, and its folder sections are an arrangement of that set, not a bound.
        const store = await storeOf(15, { sections: folders, view: 'search' });
        store.selected = 7;

        await store.selectAll();

        expect(store.selectionCount).toBe(15);
        expect(store.isSelected(idAt(0))).toBe(true);
        expect(store.isSelected(idAt(14))).toBe(true);
      });

      it('leaves the anchor at the start of what it selected', async () => {
        const store = await storeOf(15, { sections: folders });
        store.selected = 7;
        await store.selectAll();

        await store.extendSelection(13); // a Shift+click after Ctrl+A

        expect(
          [...store.selectedItemIds].sort((a, b) => a - b),
          'the range runs from the folder the selection started at, not from the lead',
        ).toEqual([5, 6, 7, 8, 9, 10, 11, 12, 13].map(idAt));
      });

      it('is discarded when the rows come from a newer index than the offsets', async () => {
        const store = await storeOf(15, { sections: folders });
        store.selected = 7;
        vi.mocked(api.gridRows).mockResolvedValue({ version: 2, rows: [entryAt(5)] });

        await store.selectAll();

        expect(store.selectionCount, 'still the lead alone, not a range of stale ids').toBe(1);
        expect(store.selectedItemIds).toEqual([idAt(7)]);
      });

      it('is superseded by a later range call', async () => {
        const store = await storeOf(15, { sections: folders });
        store.selected = 7;
        const slow = deferred<{ version: number; rows: ReturnType<typeof entryAt>[] }>();
        vi.mocked(api.gridRows).mockImplementationOnce(() => slow.promise);

        const all = store.selectAll(); // stuck on its fetch
        await store.extendSelection(1); // issued later, resolves first
        slow.resolve({ version: 1, rows: [5, 6, 7, 8, 9, 10, 11].map(entryAt) });
        await all;

        expect(
          [...store.selectedItemIds].sort((a, b) => a - b),
          'the later Shift+click wins, not the slower select-all',
        ).toEqual([1, 2, 3, 4, 5, 6, 7].map(idAt));
      });

      it('does nothing on an empty grid', async () => {
        const store = await storeOf(0, { sections: [] });
        vi.mocked(api.gridRows).mockClear();

        await store.selectAll();
        expect(store.selectionCount).toBe(0);
        expect(api.gridRows).not.toHaveBeenCalled();
      });
    });
  });
});
