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

/** Lets every pending promise callback run, however many hops deep. */
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

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
    gridFolderIdsAt: vi.fn(),
    setGridView: vi.fn(),
    setSort: vi.fn(),
    setSearchQuery: vi.fn(),
    listAlbums: vi.fn(),
    listSavedSearches: vi.fn(),
    saveSearch: vi.fn(),
    renameSavedSearch: vi.fn(),
    deleteSavedSearch: vi.fn(),
    listPeople: vi.fn(),
    listTags: vi.fn(),
    setAlbumView: vi.fn(),
    createAlbum: vi.fn(),
    renameTag: vi.fn(),
    hideTag: vi.fn(),
    restoreTagRule: vi.fn(),
    watchedFolderStats: vi.fn(),
    setItemsHidden: vi.fn(),
    setFolderHidden: vi.fn(),
    setFolderAlias: vi.fn(),
    copyPhoto: vi.fn(),
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
    onFaceProgress: vi.fn((cb: Handler) => {
      handlers.faceProgress = cb;
      return Promise.resolve(makeUnlisten('faceProgress'));
    }),
  },
  errorMessage: (e: unknown) => (e instanceof Error ? e.message : String(e)),
}));

import { api, events } from './api';
import { LibraryStore, type GridState } from './library.svelte';

/** A `grid_info` answer carrying what the store holds, layout and all. */
function asAnswer({ sections, folders, layoutGen, ...rest }: GridState): GridInfo {
  return { ...rest, layout: { generation: layoutGen ?? 1, sections, folders } };
}

describe('LibraryStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    for (const k of Object.keys(handlers)) delete handlers[k];
    for (const k of Object.keys(unlistenCounts)) delete unlistenCounts[k];
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 1,
      len: 0,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });
    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [] });
    vi.mocked(api.listAlbums).mockResolvedValue([]);
    vi.mocked(api.listSavedSearches).mockResolvedValue([]);
    vi.mocked(api.listPeople).mockResolvedValue([]);
    vi.mocked(api.listTags).mockResolvedValue([]);
    vi.mocked(api.watchedFolderStats).mockResolvedValue([]);
  });

  it('the first fetch asks for the layout', async () => {
    const store = new LibraryStore();
    await store.refresh();
    expect(vi.mocked(api.gridInfo)).toHaveBeenLastCalledWith(null);
    expect(store.info.layoutGen).toBe(1);
  });

  // A star, a keyword, an edit or a poster frame moves no photo between folders: the answer
  // leaves the layout out, and everything else in it - the counts, the view read live beside
  // the index - still lands. Review Focus: a view switch whose sections come out the same.
  it('an answer without a layout keeps the sections and takes everything else', async () => {
    const section: Section = { folderId: 3, offset: 0, count: 2, takenAtMin: 0 };
    const folder = { folderId: 3, count: 2, takenAtMin: 0, bytes: 10, modifiedMs: 0 };
    const answer = (version: number, over: Partial<GridInfo>): GridInfo => ({
      version,
      len: 2,
      layout: null,
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date', reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
      ...over,
    });
    const store = new LibraryStore();
    vi.mocked(api.gridInfo).mockResolvedValueOnce(answer(2, { layout: { generation: 5, sections: [section], folders: [folder] } }));
    await store.refresh();
    vi.mocked(api.gridInfo).mockResolvedValueOnce(answer(2, { starredCount: 4, view: 'search', searchQuery: 'x' }));
    await store.refresh();

    expect(vi.mocked(api.gridInfo)).toHaveBeenLastCalledWith(5);
    expect(store.info.sections).toEqual([section]);
    expect(store.info.folders).toEqual([folder]);
    expect(store.info.layoutGen).toBe(5);
    expect(store.info.starredCount).toBe(4);
    expect(store.info.view).toBe('search');
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

  it('refetches albums, saved searches, people and tags on every data change and after an album mutation', async () => {
    const store = new LibraryStore();
    await store.init();
    expect(api.listAlbums).toHaveBeenCalledTimes(1);

    vi.mocked(api.listAlbums).mockResolvedValue([{ id: 1, name: 'Trip', count: 2, picasa: false }]);
    vi.mocked(api.listPeople).mockResolvedValue([{ key: 'c:abc', name: 'Ada', count: 1 }]);
    vi.mocked(api.listTags).mockResolvedValue([{ tag: 'beach', count: 3, total: 3 }]);
    vi.mocked(api.listSavedSearches).mockResolvedValue([
      { id: 7, name: 'Canon', query: 'camera:canon', createdMs: 0 },
    ]);
    handlers.libraryChanged({ version: 2, len: 0, dataChanged: true });
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(store.albums).toEqual([{ id: 1, name: 'Trip', count: 2, picasa: false }]);
    expect(store.people[0]?.name).toBe('Ada');
    expect(store.tags[0]?.tag).toBe('beach');
    expect(store.albumName(1)).toBe('Trip');
    expect(store.personName('c:abc')).toBe('Ada');
    expect(store.albumName(99)).toBe('');
    expect(store.searches).toEqual([{ id: 7, name: 'Canon', query: 'camera:canon', createdMs: 0 }]);

    vi.mocked(api.createAlbum).mockResolvedValue({ id: 2, name: 'Zoo', createdMs: 0 });
    vi.mocked(api.listAlbums).mockResolvedValue([
      { id: 1, name: 'Trip', count: 2, picasa: false },
      { id: 2, name: 'Zoo', count: 0, picasa: false },
    ]);
    await expect(store.createAlbum('Zoo')).resolves.toBe(2);
    expect(api.createAlbum).toHaveBeenCalledWith('Zoo');
    expect(store.albums).toHaveLength(2);
  });

  // The tag counts alone are a quarter of a second at 300k photos, and a scan announces a
  // data change every 250ms: unserialised, the fetches stacked up faster than they answered.
  it('fetches the collections one at a time, and an awaiting mutation still sees its own change', async () => {
    const store = new LibraryStore();
    await store.init();
    const tags = () => vi.mocked(api.listTags).mock.calls.length;
    const before = tags();

    const slow = deferred<{ tag: string; count: number; total: number }[]>();
    vi.mocked(api.listTags).mockReturnValueOnce(slow.promise);
    handlers.libraryChanged({ version: 2, len: 0, dataChanged: true });
    handlers.libraryChanged({ version: 3, len: 0, dataChanged: true });
    handlers.libraryChanged({ version: 4, len: 0, dataChanged: true });
    vi.mocked(api.createAlbum).mockResolvedValue({ id: 2, name: 'Zoo', createdMs: 0 });
    // The fetch in flight read the albums before this one existed.
    const created = store.createAlbum('Zoo');
    await flush();
    expect(tags()).toBe(before + 1);

    vi.mocked(api.listAlbums).mockResolvedValue([{ id: 2, name: 'Zoo', count: 0, picasa: false }]);
    slow.resolve([]);
    await expect(created).resolves.toBe(2);
    // Everything that arrived during the first fetch shared one more.
    expect(tags()).toBe(before + 2);
    expect(store.albums).toEqual([{ id: 2, name: 'Zoo', count: 0, picasa: false }]);
  });

  // A view switch awaits its own refresh, which usually lands before the event announcing the
  // switch's rebuild; that event then has nothing to add, and fetching the whole grid again
  // for it doubled the cost of every switch and every search keystroke.
  it('does not refetch the grid for an event at a version it already shows', async () => {
    const store = new LibraryStore();
    await store.init();
    expect(store.info.version).toBe(1);
    const calls = vi.mocked(api.gridInfo).mock.calls.length;

    handlers.libraryChanged({ version: 1, len: 0, dataChanged: false });
    await flush();
    expect(vi.mocked(api.gridInfo).mock.calls.length).toBe(calls);

    vi.mocked(api.gridInfo).mockResolvedValue({ ...asAnswer(store.info), version: 2 });
    handlers.libraryChanged({ version: 2, len: 0, dataChanged: false });
    await flush();
    expect(vi.mocked(api.gridInfo).mock.calls.length).toBe(calls + 1);
    expect(store.info.version).toBe(2);
  });

  // Every page of a screenful answered at a newer version reports itself stale.
  it('refetches the grid once for a screenful of stale pages, not once a page', async () => {
    const info = (version: number) => ({
      version,
      len: 1000,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all' as const,
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });
    vi.mocked(api.gridInfo).mockResolvedValue(info(1));
    const store = new LibraryStore();
    await store.init();
    const calls = vi.mocked(api.gridInfo).mock.calls.length;

    // The backend has rebuilt; the event announcing it has not arrived yet.
    vi.mocked(api.gridInfo).mockResolvedValue(info(2));
    vi.mocked(api.gridRows).mockResolvedValue({ version: 2, rows: [] });
    await store.ensure(0, 1000);
    await flush();
    // The first fetch lands at the version the pages reported, which answers the rest.
    expect(vi.mocked(api.gridInfo).mock.calls.length).toBe(calls + 1);
    expect(store.info.version).toBe(2);
  });

  // A view switch, a sort or a search rebuilds the grid and changes no data; refetching the
  // collections for it cost a quarter of a second of tag counting per keystroke.
  it('leaves the collections alone on a library change that moved no data', async () => {
    const store = new LibraryStore();
    await store.init();
    const calls = () => [api.listAlbums, api.listSavedSearches, api.listPeople, api.listTags].map((f) => vi.mocked(f).mock.calls.length);
    expect(calls()).toEqual([1, 1, 1, 1]);
    const data = store.dataVersion;

    handlers.libraryChanged({ version: 2, len: 0, dataChanged: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(calls()).toEqual([1, 1, 1, 1]);
    expect(store.dataVersion).toBe(data);

    handlers.libraryChanged({ version: 3, len: 0, dataChanged: true });
    await Promise.resolve();
    expect(calls()).toEqual([2, 2, 2, 2]);
    expect(store.dataVersion).toBe(data + 1);
  });

  // None of the three changes the grid, so no `library_changed` is coming to refresh the
  // sidebar; each mutation has to refetch for itself or the new row never appears.
  it('saved-search mutations refetch the collections themselves', async () => {
    const store = new LibraryStore();
    await store.init();
    const calls = vi.mocked(api.listSavedSearches).mock.calls.length;

    vi.mocked(api.saveSearch).mockResolvedValue({ id: 1, name: 'a', query: 'a', createdMs: 0 });
    vi.mocked(api.listSavedSearches).mockResolvedValue([{ id: 1, name: 'a', query: 'a', createdMs: 0 }]);
    await store.saveSearch('a', 'a');
    expect(api.saveSearch).toHaveBeenCalledWith('a', 'a');
    expect(store.searches).toHaveLength(1);

    vi.mocked(api.renameSavedSearch).mockResolvedValue();
    vi.mocked(api.listSavedSearches).mockResolvedValue([{ id: 1, name: 'b', query: 'a', createdMs: 0 }]);
    await store.renameSavedSearch(1, 'b');
    expect(store.searches[0]?.name).toBe('b');

    vi.mocked(api.deleteSavedSearch).mockResolvedValue();
    vi.mocked(api.listSavedSearches).mockResolvedValue([]);
    await store.deleteSavedSearch(1);
    expect(store.searches).toEqual([]);
    expect(vi.mocked(api.listSavedSearches).mock.calls.length).toBe(calls + 3);
  });

  it('tag rule changes refetch the collections', async () => {
    const store = new LibraryStore();
    await store.init();
    vi.mocked(api.listTags).mockResolvedValue([{ tag: 'vacation', count: 1, total: 1 }]);
    vi.mocked(api.renameTag).mockResolvedValue();
    await store.renameTag('holiday', 'vacation');
    expect(api.renameTag).toHaveBeenCalledWith('holiday', 'vacation');
    expect(store.tags).toEqual([{ tag: 'vacation', count: 1, total: 1 }]);

    vi.mocked(api.listTags).mockResolvedValue([]);
    vi.mocked(api.hideTag).mockResolvedValue();
    await store.hideTag('vacation');
    expect(api.hideTag).toHaveBeenCalledWith('vacation');
    expect(store.tags).toEqual([]);

    vi.mocked(api.listTags).mockResolvedValue([{ tag: 'holiday', count: 1, total: 1 }]);
    vi.mocked(api.restoreTagRule).mockResolvedValue();
    await store.restoreTagRule('holiday');
    expect(api.restoreTagRule).toHaveBeenCalledWith('holiday');
    expect(store.tags).toEqual([{ tag: 'holiday', count: 1, total: 1 }]);
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
    handlers.libraryChanged({ version: 2, len: 0, dataChanged: true });
    await flush();
    expect(store.toasts.some((t) => t.message === 'albums-fail')).toBe(true);
  });

  it('routes background refresh failures from event handlers into reportError instead of throwing unhandled', async () => {
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.gridInfo).mockRejectedValueOnce(new Error('boom'));
    handlers.libraryChanged({ version: 2, len: 0, dataChanged: false });
    await flush();

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
      durationMs: null,
      thumbKey: '0',
      starred: false,
      hasCopies: false,
    });
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 1,
      len: 2,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
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
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });
    vi.mocked(api.gridOffsetOfItem).mockResolvedValue(2);
    await store.refresh();

    expect(api.gridOffsetOfItem).toHaveBeenCalledWith(11);
    expect(store.selected).toBe(2);
  });

  it('keeps the rows on screen until the rebuilt index can replace them', async () => {
    // A scan rebuilds the grid every couple of seconds. Dropping the loaded rows before the
    // new ones arrived left every visible tile empty for a round trip on each rebuild.
    const entry = (id: number) => ({
      id,
      folderId: 1,
      takenAt: 0,
      aspect: 1,
      kind: 'image' as const,
      durationMs: null,
      thumbKey: '0',
      starred: false,
      hasCopies: false,
    });
    const info = (version: number, len: number) => ({
      version,
      len,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all' as const,
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });
    vi.mocked(api.gridInfo).mockResolvedValue(info(1, 2));
    vi.mocked(api.gridRows).mockResolvedValue({ version: 1, rows: [entry(10), entry(11)] });
    const store = new LibraryStore();
    await store.init();
    await store.ensure(0, 2);

    // A new photo sorts first.
    vi.mocked(api.gridInfo).mockResolvedValue(info(2, 3));
    const rows = deferred<{ version: number; rows: ReturnType<typeof entry>[] }>();
    vi.mocked(api.gridRows).mockReturnValue(rows.promise);
    const refreshed = store.refresh();
    await Promise.resolve();
    await Promise.resolve();
    expect(api.gridRows).toHaveBeenCalledWith(0, 200);
    // Still loading: the old version is what shows, rows and length together.
    expect(store.entry(0)?.id).toBe(10);
    expect(store.info.len).toBe(2);

    rows.resolve({ version: 2, rows: [entry(9), entry(10), entry(11)] });
    await refreshed;
    expect(store.info.len).toBe(3);
    expect(store.entry(0)?.id).toBe(9);
    expect(store.entry(2)?.id).toBe(11);
  });

  // One grid fetch at a time: a scan announces a rebuild every 250ms, and unserialised
  // fetches piled up. The one issued second still has to see the grid as it is after its
  // call, so it runs once the first has landed rather than being folded into it.
  it('a refresh issued while another is loading waits for it, then fetches again', async () => {
    const entry = (id: number) => ({
      id,
      folderId: 1,
      takenAt: 0,
      aspect: 1,
      kind: 'image' as const,
      durationMs: null,
      thumbKey: '0',
      starred: false,
      hasCopies: false,
    });
    const info = (version: number, len: number) => ({
      version,
      len,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all' as const,
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });
    vi.mocked(api.gridInfo).mockResolvedValue(info(1, 1));
    vi.mocked(api.gridRows).mockResolvedValue({ version: 1, rows: [entry(10)] });
    const store = new LibraryStore();
    await store.init();
    await store.ensure(0, 1);

    vi.mocked(api.gridInfo).mockResolvedValueOnce(info(2, 2));
    const late = deferred<{ version: number; rows: ReturnType<typeof entry>[] }>();
    vi.mocked(api.gridRows).mockReturnValueOnce(late.promise);
    const calls = vi.mocked(api.gridInfo).mock.calls.length;
    const slow = store.refresh();
    await flush();

    vi.mocked(api.gridInfo).mockResolvedValueOnce(info(3, 3));
    vi.mocked(api.gridRows).mockResolvedValueOnce({ version: 3, rows: [entry(8), entry(9), entry(10)] });
    let landed = false;
    const second = store.refresh().then(() => (landed = true));
    const third = store.refresh();
    await flush();
    expect(vi.mocked(api.gridInfo).mock.calls.length).toBe(calls + 1);
    expect(landed).toBe(false);

    late.resolve({ version: 2, rows: [entry(9), entry(10)] });
    await slow;
    await Promise.all([second, third]);
    // The two callers who arrived during the first fetch shared one more.
    expect(vi.mocked(api.gridInfo).mock.calls.length).toBe(calls + 2);
    expect(store.info.version).toBe(3);
    expect(store.info.len).toBe(3);
    expect(store.entry(0)?.id).toBe(8);
  });

  it('a selection made by id is re-found after a rebuild even when its page was never loaded', async () => {
    // "Locate in photon" and closing the viewer both know the photo's id but land on an
    // offset whose page the grid has not fetched yet. Recording only the offset would leave
    // nothing to re-find by, and the next scan would slide the selection onto a neighbour.
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 1,
      len: 5,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });
    const store = new LibraryStore();
    await store.init();
    store.selectItem(3, 11);
    expect(store.selected).toBe(3);

    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 2,
      len: 6,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
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
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });
    const store = new LibraryStore();
    await store.init();
    store.selected = 4;

    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 2,
      len: 2,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });
    await store.refresh();
    expect(store.selected).toBe(1);
    expect(api.gridOffsetOfItem).not.toHaveBeenCalled();

    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 3,
      len: 0,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
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

  /** The status bar's line for the face pass: held while a pass runs, gone at its end (a
   *  switch-off sends a zeroed `running: false` event, sometimes twice) and on dispose. */
  it('holds face progress while a pass runs and clears it at the end and on dispose', async () => {
    const store = new LibraryStore();
    await store.init();
    expect(events.onFaceProgress).toHaveBeenCalledTimes(1);

    handlers.faceProgress({ phase: 'detecting', checked: 64, total: 900, running: true });
    expect(store.faces).toEqual({ phase: 'detecting', checked: 64, total: 900, running: true });

    handlers.faceProgress({ phase: 'detecting', checked: 0, total: 0, running: false });
    handlers.faceProgress({ phase: 'detecting', checked: 0, total: 0, running: false });
    expect(store.faces).toBeNull();

    handlers.faceProgress({ phase: 'detecting', checked: 10, total: 900, running: true });
    store.dispose();
    expect(store.faces).toBeNull();
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
      layout: { generation: number; sections: never[]; folders: never[] };
      starredCount: number;
      duplicateCount: number;
      hiddenCount: number;
      videoCount: number;
      view: 'all';
      sort: { key: 'date'; reverse: boolean };
      searchQuery: string;
      person: null;
      album: null;
      tag: null;
      copiesOf: null;
      buildError: null;
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
    gridInfoGate.resolve({ version: 1, len: 0, layout: { generation: 1, sections: [], folders: [] }, starredCount: 0, duplicateCount: 0, hiddenCount: 0, videoCount: 0, view: 'all', sort: { key: 'date', reverse: false }, searchQuery: '', person: null, album: null, tag: null, copiesOf: null, buildError: null });
    await initPromise;

    expect(unlistenCounts.libraryChanged).toBe(1);
    expect(unlistenCounts.folderStatus).toBe(1);
    expect(unlistenCounts.scanProgress).toBe(1);
  });

  it('setView switches the backend view and only resolves once the refreshed grid has landed', async () => {
    const store = new LibraryStore();
    await store.init();

    const refreshGate = deferred<GridInfo>();
    vi.mocked(api.setGridView).mockResolvedValue(null);
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

    refreshGate.resolve({ version: 2, len: 0, layout: { generation: 1, sections: [], folders: [] }, starredCount: 0, duplicateCount: 0, hiddenCount: 0, videoCount: 0, view: 'starred', sort: { key: 'date', reverse: false }, searchQuery: '', person: null, album: null, tag: null, copiesOf: null, buildError: null });
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

  // The backend's rebuild announces itself with `library-changed`, and that event can reach
  // the webview before the command's own reply. The listener then fetches the grid, and the
  // command's refresh used to fetch the very same grid again. The command answers with the
  // version it published, and the refresh after it waits for that version instead.
  describe('when the rebuild is announced before the command replies', () => {
    const at = (version: number, over: Partial<GridInfo>): GridInfo => ({
      version,
      len: 0,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date', reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
      ...over,
    });
    const announce = (version: number) => handlers.libraryChanged({ version, len: 0, dataChanged: false });

    it('a view switch fetches the grid once, while the listener’s fetch is still in flight', async () => {
      const store = new LibraryStore();
      await store.init();
      const fetches = vi.mocked(api.gridInfo).mock.calls.length;
      vi.mocked(api.gridInfo).mockResolvedValue(at(2, { view: 'starred' }));
      vi.mocked(api.setGridView).mockImplementationOnce(async () => {
        announce(2);
        return 2;
      });

      await store.setView('starred');

      expect(vi.mocked(api.gridInfo).mock.calls.length - fetches).toBe(1);
      expect(store.info.view).toBe('starred');
    });

    it('a sort fetches the grid once, when the listener’s fetch has already landed', async () => {
      const store = new LibraryStore();
      await store.init();
      const fetches = vi.mocked(api.gridInfo).mock.calls.length;
      const byName = { key: 'name' as const, reverse: false };
      vi.mocked(api.gridInfo).mockResolvedValue(at(2, { sort: byName }));
      vi.mocked(api.setSort).mockImplementationOnce(async () => {
        announce(2);
        await flush();
        return 2;
      });

      await store.setSort(byName);

      expect(vi.mocked(api.gridInfo).mock.calls.length - fetches).toBe(1);
      expect(store.info.sort).toEqual(byName);
    });

    it('a search fetches the grid once', async () => {
      const store = new LibraryStore();
      await store.init();
      const fetches = vi.mocked(api.gridInfo).mock.calls.length;
      vi.mocked(api.gridInfo).mockResolvedValue(at(2, { view: 'search', searchQuery: 'lake' }));
      vi.mocked(api.setSearchQuery).mockImplementationOnce(async () => {
        announce(2);
        return 2;
      });

      await expect(store.setSearchQuery('lake')).resolves.toBe('lake');

      expect(vi.mocked(api.gridInfo).mock.calls.length - fetches).toBe(1);
      expect(store.info.searchQuery).toBe('lake');
    });

    // No version is the backend saying it cannot vouch for one - its rebuild was superseded
    // before it landed - so the grid on hand, whatever its version, may be the old view's.
    it('a command that answers with no version still gets a fetch of its own', async () => {
      const store = new LibraryStore();
      await store.init();
      vi.mocked(api.gridInfo).mockResolvedValue(at(2, { view: 'all' }));
      vi.mocked(api.setGridView).mockImplementationOnce(async () => {
        announce(2);
        await flush();
        return null;
      });
      const fetches = vi.mocked(api.gridInfo).mock.calls.length;
      vi.mocked(api.gridInfo).mockResolvedValueOnce(at(2, { view: 'all' }));
      vi.mocked(api.gridInfo).mockResolvedValueOnce(at(2, { view: 'starred' }));

      await store.setView('starred');

      expect(vi.mocked(api.gridInfo).mock.calls.length - fetches).toBe(2);
      expect(store.info.view).toBe('starred');
    });
  });

  it('setSearchQuery("") issues the command and refreshes, restoring the All view', async () => {
    // Spec §7: "clearing the box restores the All view." The engine-level behaviour behind
    // this is already covered on the Rust side; this pins the store's half of the path.
    const store = new LibraryStore();
    await store.init();

    vi.mocked(api.setSearchQuery).mockResolvedValue(null);
    vi.mocked(api.gridInfo).mockResolvedValueOnce({
      version: 2,
      len: 2,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'all',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
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
      return null;
    });
    vi.mocked(api.setSearchQuery).mockImplementationOnce(async (q: string) => {
      order.push(`start:${q}`);
      await second.promise;
      order.push(`end:${q}`);
      return null;
    });
    vi.mocked(api.gridInfo).mockResolvedValue({
      version: 2,
      len: 0,
      layout: { generation: 1, sections: [], folders: [] },
      starredCount: 0,
      duplicateCount: 0,
      hiddenCount: 0,
      videoCount: 0,
      view: 'search',
      sort: { key: 'date' as const, reverse: false },
      searchQuery: 'beach',
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    });

    const p1 = store.setSearchQuery('b');
    // Sent before the second is issued: a search still waiting its turn would be replaced
    // by the second rather than run before it.
    await flush();
    expect(order).toEqual(['start:b']);
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

  // Each debounced keystroke used to queue a whole-library search behind the one running,
  // to be thrown away the moment the next landed. A search still waiting its turn takes
  // the newer query instead - but only while nothing else has been queued after it, or a
  // view switch issued between two searches would be reordered around them.
  it('replaces a search still waiting its turn instead of queueing another behind it', async () => {
    const store = new LibraryStore();
    await store.init();
    const order: string[] = [];
    const running = deferred<void>();
    vi.mocked(api.setSearchQuery).mockImplementation(async (q: string) => {
      order.push(`search:${q}`);
      if (q === 'a') await running.promise;
      return null;
    });
    vi.mocked(api.setGridView).mockImplementation(async (view: GridView) => {
      order.push(`view:${view}`);
      return null;
    });

    const a = store.setSearchQuery('a');
    await flush();
    expect(order).toEqual(['search:a']);

    // Three keystrokes while 'a' runs: one step, holding the last.
    const b = store.setSearchQuery('b');
    const be = store.setSearchQuery('be');
    const bea = store.setSearchQuery('bea');
    // A switch after them, and a search after the switch: neither may merge across it.
    const starred = store.setView('starred');
    const x = store.setSearchQuery('x');
    const xy = store.setSearchQuery('xy');

    running.resolve();
    await Promise.all([a, b, be, bea, starred, x, xy]);
    expect(order).toEqual(['search:a', 'search:bea', 'view:starred', 'search:xy']);
    // A replaced search answers with its own query: it was never refused, and answering
    // 'bea' would hand that text back to a box the user may have typed on since.
    await expect(b).resolves.toBe('b');
    await expect(bea).resolves.toBe('bea');
    await expect(x).resolves.toBe('x');
  });

  it('a refused search that replaced others rolls back only the caller whose query it sent', async () => {
    const store = new LibraryStore();
    await store.init();
    const running = deferred<void>();
    vi.mocked(api.setSearchQuery).mockImplementationOnce(() => running.promise.then(() => null));
    vi.mocked(api.setSearchQuery).mockRejectedValueOnce(new Error('refused'));
    const first = store.setSearchQuery('lake');
    await flush();

    const b = store.setSearchQuery('b');
    const beach = store.setSearchQuery('beach');
    vi.mocked(api.gridInfo).mockResolvedValue({ ...asAnswer(store.info), version: 2, view: 'search', searchQuery: 'lake' });
    running.resolve();
    await expect(first).resolves.toBe('lake');
    await expect(beach).resolves.toBe('lake');
    await expect(b).resolves.toBe('b');
    expect(api.setSearchQuery).toHaveBeenCalledTimes(2);
  });

  it('a view switch waits for a search already sent, so the search cannot land after it', async () => {
    const store = new LibraryStore();
    await store.init();

    const order: string[] = [];
    const search = deferred<void>();
    vi.mocked(api.setSearchQuery).mockImplementationOnce(async (q: string) => {
      order.push(`start:${q}`);
      await search.promise;
      order.push(`end:${q}`);
      return null;
    });
    vi.mocked(api.setGridView).mockImplementationOnce(async (view: GridView) => {
      order.push(`view:${view}`);
      return null;
    });

    const p1 = store.setSearchQuery('beach');
    const p2 = store.setView('starred');
    await Promise.resolve();
    await Promise.resolve();
    search.resolve();
    await Promise.all([p1, p2]);

    // Unchained, Starred is applied while 'beach' is still in flight, and 'beach' then puts
    // the backend back into Search under a box the switch has already emptied.
    expect(order).toEqual(['start:beach', 'end:beach', 'view:starred']);
  });

  it('setSort reloads the grid in the new order without running the view-switch hooks', async () => {
    const store = new LibraryStore();
    await store.init();
    const hook = vi.fn(() => () => {});
    store.onViewSwitch(hook);
    store.selectItem(3, 42);

    const bySize = { key: 'size' as const, reverse: true };
    vi.mocked(api.setSort).mockResolvedValueOnce(null);
    vi.mocked(api.gridInfo).mockResolvedValueOnce({ version: 2, len: 0, layout: { generation: 1, sections: [], folders: [] }, starredCount: 0, duplicateCount: 0, hiddenCount: 0, videoCount: 0, view: 'search', sort: bySize, searchQuery: 'lake', person: null, album: null, tag: null, copiesOf: null, buildError: null });
    await store.setSort(bySize);

    expect(api.setSort).toHaveBeenCalledWith(bySize);
    expect(store.info.sort).toEqual(bySize);
    // A sort is not a view switch: the search box keeps what it holds.
    expect(hook).not.toHaveBeenCalled();
    // The Shift+click anchor is an offset, and in the new order it names another photo. The
    // id set is what a refresh never prunes, so it is what shows the clear happened.
    expect(store.selectionCount).toBe(0);
    expect(store.isSelected(42)).toBe(false);
  });

  it('builds a second sort change on the first while the first is still in flight', async () => {
    const store = new LibraryStore();
    await store.init();
    const first = deferred<void>();
    const second = deferred<void>();
    vi.mocked(api.setSort)
      .mockReturnValueOnce(first.promise.then(() => null))
      .mockReturnValueOnce(second.promise.then(() => null));
    const info = (sort: { key: 'name'; reverse: boolean }) => ({ version: 2, len: 0, layout: { generation: 1, sections: [], folders: [] }, starredCount: 0, duplicateCount: 0, hiddenCount: 0, videoCount: 0, view: 'all' as const, sort, searchQuery: '', person: null, album: null, tag: null, copiesOf: null, buildError: null });
    vi.mocked(api.gridInfo)
      .mockResolvedValueOnce(info({ key: 'name', reverse: false }))
      .mockResolvedValueOnce(info({ key: 'name', reverse: true }));

    const byName = store.setSort({ ...store.sort, key: 'name' });
    const reversed = store.setSort({ ...store.sort, reverse: !store.sort.reverse });
    expect(store.sort).toEqual({ key: 'name', reverse: true });
    first.resolve();
    await byName;
    // The first has landed and `info` says Name ascending, but the reversal is still queued:
    // the control must keep showing it, or a third click would build on the wrong sort.
    expect(store.info.sort).toEqual({ key: 'name', reverse: false });
    expect(store.sort).toEqual({ key: 'name', reverse: true });
    second.resolve();
    await reversed;

    expect(vi.mocked(api.setSort).mock.calls.map((c) => c[0])).toEqual([
      { key: 'name', reverse: false },
      { key: 'name', reverse: true },
    ]);
    expect(store.sort).toEqual({ key: 'name', reverse: true });
  });

  it('routes a refused sort into reportError and leaves the grid as it was', async () => {
    const store = new LibraryStore();
    await store.init();
    const calls = vi.mocked(api.gridInfo).mock.calls.length;

    vi.mocked(api.setSort).mockRejectedValueOnce(new Error('sort-fail'));
    await expect(store.setSort({ key: 'name', reverse: false })).resolves.toBeUndefined();

    expect(store.toasts.some((t) => t.message === 'sort-fail')).toBe(true);
    expect(vi.mocked(api.gridInfo).mock.calls.length).toBe(calls);
    expect(store.info.sort).toEqual({ key: 'date', reverse: false });
    // And the control, which showed Name while it was asked for, is back on the grid's sort.
    expect(store.sort).toEqual({ key: 'date', reverse: false });
  });

  it('runs the view-switch hooks as the switch is issued, and takes them back only if it is refused', async () => {
    const store = new LibraryStore();
    await store.init();
    const undo = vi.fn();
    const hook = vi.fn(() => undo);
    store.onViewSwitch(hook);

    const command = deferred<void>();
    vi.mocked(api.setGridView).mockReturnValueOnce(command.promise.then(() => null));
    const switched = store.setView('starred');
    // Before the backend has answered: text typed from here on belongs after the switch.
    expect(hook).toHaveBeenCalledOnce();
    command.resolve();
    await switched;
    expect(undo).not.toHaveBeenCalled();

    vi.mocked(api.setGridView).mockRejectedValueOnce(new Error('refused'));
    await store.setView('recent');
    expect(undo).toHaveBeenCalledOnce();

    // A refresh failing after the command succeeded is not a refusal: the backend has
    // already moved, so the box must stay empty to match it.
    vi.mocked(api.setGridView).mockResolvedValueOnce(null);
    vi.mocked(api.gridInfo).mockRejectedValueOnce(new Error('refresh failed'));
    await store.setView('starred');
    expect(undo).toHaveBeenCalledOnce();
  });

  it('a refused switch does not take its hooks back once a later switch has been issued', async () => {
    const store = new LibraryStore();
    await store.init();
    const undo = vi.fn();
    store.onViewSwitch(() => undo);

    const starred = deferred<void>();
    vi.mocked(api.setGridView).mockReturnValueOnce(starred.promise.then(() => null));
    vi.mocked(api.setGridView).mockResolvedValueOnce(null);
    const first = store.setView('starred');
    const second = store.setView('recent');
    starred.reject(new Error('refused'));
    await Promise.all([first, second]);

    // Recent emptied the box and landed; Starred putting the search back would leave it
    // over Recent's grid.
    expect(undo).not.toHaveBeenCalled();
  });

  it('answers a refused search with the query the backend rolled back to', async () => {
    const store = new LibraryStore();
    await store.init();
    vi.mocked(api.setSearchQuery).mockResolvedValueOnce(null);
    vi.mocked(api.gridInfo).mockResolvedValueOnce({ version: 2, len: 0, layout: { generation: 1, sections: [], folders: [] }, starredCount: 0, duplicateCount: 0, hiddenCount: 0, videoCount: 0, view: 'search', sort: { key: 'date', reverse: false }, searchQuery: 'lake', person: null, album: null, tag: null, copiesOf: null, buildError: null });
    await expect(store.setSearchQuery('lake')).resolves.toBe('lake');

    vi.mocked(api.setSearchQuery).mockRejectedValueOnce(new Error('refused'));
    await expect(store.setSearchQuery('beach')).resolves.toBe('lake');
  });

  it('reports the view only once the commands already issued have landed', async () => {
    const store = new LibraryStore();
    await store.init();
    const search = deferred<void>();
    vi.mocked(api.setSearchQuery).mockReturnValueOnce(search.promise.then(() => null));
    vi.mocked(api.gridInfo).mockResolvedValueOnce({ version: 2, len: 0, layout: { generation: 1, sections: [], folders: [] }, starredCount: 0, duplicateCount: 0, hiddenCount: 0, videoCount: 0, view: 'search', sort: { key: 'date', reverse: false }, searchQuery: 'beach', person: null, album: null, tag: null, copiesOf: null, buildError: null });

    void store.setSearchQuery('beach');
    const view = store.settledView();
    search.resolve();

    // Read at the click, this is still All - and a folder jump from All skips the switch,
    // leaving the search to land and carry the grid away from the folder.
    await expect(view).resolves.toBe('search');
  });

  it('hiding a folder refetches the folder list, so its menu offers Unhide next time', async () => {
    const store = new LibraryStore();
    await store.init();
    const folder = { id: 3, watchedId: 1, parentId: 1, path: '/p/a', name: 'a', hidden: false, alias: null };
    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [folder] });
    await store.refreshFolders();
    expect(store.folderOf(3)?.hidden).toBe(false);

    vi.mocked(api.setFolderHidden).mockResolvedValue(4);
    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [{ ...folder, hidden: true }] });
    await store.setFolderHidden(3, true);
    expect(api.setFolderHidden).toHaveBeenCalledWith(3, true);
    expect(store.folderOf(3)?.hidden).toBe(true);
  });

  it('an emptied alias is sent as none, and the folder list is refetched for the new label', async () => {
    const store = new LibraryStore();
    await store.init();
    const folder = { id: 3, watchedId: 1, parentId: 1, path: '/p/a', name: 'a', hidden: false, alias: 'Easter' };
    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [folder] });
    await store.refreshFolders();

    vi.mocked(api.setFolderAlias).mockResolvedValue(true);
    vi.mocked(api.listFolders).mockResolvedValue({ watched: [], folders: [{ ...folder, alias: null }] });
    await store.setFolderAlias(3, '');
    expect(api.setFolderAlias).toHaveBeenCalledWith(3, null);
    expect(store.folderOf(3)?.alias).toBe(null);
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
      durationMs: null,
      thumbKey: '0',
      starred: false,
      hasCopies: false,
    });

    /** A store over `len` photos, with every page answerable. `sections` and `view` matter
     *  only to select-all, which reads the folder the lead is in. */
    async function storeOf(len: number, opts: { sections?: Section[]; view?: GridView } = {}) {
      vi.mocked(api.gridInfo).mockResolvedValue({
        version: 1,
        len,
        layout: { generation: 1, sections: opts.sections ?? [], folders: [] },
        starredCount: 0,
        duplicateCount: 0,
        hiddenCount: 0,
      videoCount: 0,
        view: opts.view ?? 'all',
        sort: { key: 'date' as const, reverse: false },
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
        copiesOf: null,
        buildError: null,
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

    it('hiding the selection moves it to the next photo still shown, and drops the hidden ones', async () => {
      // The cleanup workflow: hide one, and the arrow keys carry on from where it was
      // rather than from the top of the library; and nothing hidden stays selected, so
      // the next action cannot reach a photo the user can no longer see.
      const store = await storeOf(10);
      store.selected = 2;
      store.toggleSelected(5);
      store.toggleSelected(6);
      store.toggleSelected(5);
      store.toggleSelected(5); // lead on 5, selection {2, 5, 6}
      // Where the rebuilt index puts photo 7: after 0, 1, 3 and 4.
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(4);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        // The backend announces its rebuild before the command returns, so the store can
        // have reloaded the new index - where the offset after the lead is no longer the
        // photo after it - by the time the write resolves.
        const kept = [0, 1, 3, 4, 7, 8, 9];
        vi.mocked(api.gridInfo).mockResolvedValue({
          version: 2,
          len: kept.length,
          layout: { generation: 1, sections: [], folders: [] },
          starredCount: 0,
          duplicateCount: 0,
          hiddenCount: 3,
          videoCount: 0,
          view: 'all',
          sort: { key: 'date' as const, reverse: false },
          searchQuery: '',
          person: null,
          album: null,
          tag: null,
          copiesOf: null,
          buildError: null,
        });
        vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
          version: 2,
          rows: kept.slice(offset, offset + count).map(entryAt),
        }));
        await store.refresh();
        await store.ensure(0, kept.length);
        return 3;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(api.setItemsHidden).toHaveBeenCalledWith(expect.arrayContaining([idAt(2), idAt(5), idAt(6)]), true);
      expect(store.selectedItemIds).toEqual([idAt(7)]);
      expect(api.gridOffsetOfItem).toHaveBeenCalledWith(idAt(7));
      expect(store.selected).toBe(4);
    });

    it('hiding the last photo moves the selection back to the one before it', async () => {
      const store = await storeOf(10);
      store.selected = 9;
      vi.mocked(api.setItemsHidden).mockResolvedValue(1);
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(8);
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(8)]);
      expect(store.selected).toBe(8);
    });

    it('lets go of pages far outside the window, and fetches them again on the way back', async () => {
      // A scrub through a large library held every page it passed until the next rebuild.
      const store = await storeOf(5000);
      expect(store.entry(0)?.id).toBe(idAt(0));

      await store.ensure(4000, 4050);
      expect(store.entry(0)).toBeUndefined();
      expect(store.entry(4000)?.id).toBe(idAt(4000));

      vi.mocked(api.gridRows).mockClear();
      await store.ensure(0, 50);
      expect(api.gridRows).toHaveBeenCalledWith(0, 200);
      expect(store.entry(0)?.id).toBe(idAt(0));
      // Near the window is kept: scrolling back a little costs no fetch.
      await store.ensure(3000, 3050);
      vi.mocked(api.gridRows).mockClear();
      await store.ensure(2400, 2450);
      await store.ensure(3000, 3050);
      expect(api.gridRows).toHaveBeenCalledTimes(1);
    });

    it('keeps the pages around the lead wherever the grid scrolls, so H still moves on from it', async () => {
      // The lead is the last photo of its page: the photo to move on to is on the next one.
      const store = await storeOf(5000);
      await store.ensure(150, 250);
      store.selected = 199;
      await store.ensure(4000, 4050);
      expect(store.entry(199)?.id).toBe(idAt(199));

      vi.mocked(api.setItemsHidden).mockResolvedValue(1);
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(199);
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(200)]);
    });

    /** What the backend holds once a hide of the photos at `[from, to]` has landed: the
     *  rest, in the same order, at a new version. */
    function hiddenFrom(len: number, from: number, to: number) {
      const kept = Array.from({ length: len }, (_, i) => i).filter((at) => at < from || at > to);
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 2,
        rows: kept.slice(offset, offset + count).map(entryAt),
      }));
      vi.mocked(api.gridOffsetOfItem).mockImplementation(async (id: number) => {
        const at = kept.indexOf(id - 100);
        return at < 0 ? null : at;
      });
    }

    it('hiding a band whose middle pages were let go lands after the band, not before it', async () => {
      // A band from 10 to 3000, autoscrolled: the window is at the far end, so every page
      // between the lead's own and the window's has been evicted. The photo to move to is
      // 3001, after the band; a walk that gave up at the first missing page fell back to 9.
      const store = await storeOf(5000);
      store.beginBand(false);
      await store.ensure(2950, 3050);
      await store.endBand([[10, 3000]]);
      expect(store.selected).toBe(10);
      expect(store.entry(1000)).toBeUndefined();

      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 10, 3000);
        return 2991;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(3001)]);
      expect(store.selected).toBe(10);
    });

    /** Ctrl+A over a big view, then H. Finding the photo after the selection walked the
     *  old index first, a thousand rows a round trip - some 300 of them at 300,000 photos -
     *  before the write was even sent, so H did nothing for seconds. */
    it('a select-all hide sends the write without first fetching every page', async () => {
      const store = await storeOf(5000, { view: 'starred' });
      store.selected = 0;
      await store.selectAll();
      expect(store.selectionCount).toBe(5000);

      vi.mocked(api.gridRows).mockClear();
      let fetchedFirst = -1;
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        fetchedFirst = vi.mocked(api.gridRows).mock.calls.length;
        hiddenFrom(5000, 0, 4999);
        return 5000;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(fetchedFirst).toBe(0);
      // Nothing stays in the view, so there is nothing to move to.
      expect(store.selectionCount).toBe(0);
      expect(store.selected).toBe(null);
    });

    /** Ctrl+A in All takes the lead's folder, and the lead stays where it was - in the
     *  middle of a folder bigger than the pages around it, so the loaded pages settle
     *  neither side. The photo at the lead's offset in the rebuilt index was one as far into
     *  the next folder as the lead was into its own: 2,400 photos on, or past the end. */
    it('hiding a folder selected around the lead lands on the photo after the folder', async () => {
      const sections: Section[] = [
        { folderId: 1, offset: 0, count: 100, takenAtMin: 0 },
        { folderId: 2, offset: 100, count: 4800, takenAtMin: 0 },
        { folderId: 3, offset: 4900, count: 5100, takenAtMin: 0 },
      ];
      const store = await storeOf(10000, { sections });
      await store.ensure(2450, 2550);
      store.selected = 2500;
      await store.selectAll();
      expect(store.selectionCount).toBe(4800);
      // The window scrolled away: nothing before the folder is loaded any more.
      await store.ensure(8000, 8050);
      expect(store.entry(99)).toBeUndefined();

      vi.mocked(api.gridRows).mockClear();
      let fetchedFirst = -1;
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        fetchedFirst = vi.mocked(api.gridRows).mock.calls.length;
        hiddenFrom(10000, 100, 4899);
        return 4800;
      });
      await store.setHidden(store.selectedItemIds, true);
      // One photo asked for before the write, not a walk.
      expect(fetchedFirst).toBe(1);
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(100);
    });

    /** The same, for the view's first folder: nothing comes before it to ask for, and the
     *  photo after the folder is the rebuilt index's first. */
    it('hiding the first folder selected around the lead lands on the photo after it', async () => {
      const sections: Section[] = [
        { folderId: 1, offset: 0, count: 4900, takenAtMin: 0 },
        { folderId: 2, offset: 4900, count: 5100, takenAtMin: 0 },
      ];
      const store = await storeOf(10000, { sections });
      await store.ensure(2450, 2550);
      store.selected = 2500;
      await store.selectAll();
      await store.ensure(8000, 8050);

      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(10000, 0, 4899);
        return 4900;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(0);
    });

    /** The fetch of the photo before the selection only chooses where to land: failing, it
     *  must not stop the hide the user asked for. */
    it('a hide is still sent when the photo before its selection cannot be fetched', async () => {
      const store = await storeOf(5000);
      store.selectItem(2500, idAt(2500));
      await store.ensure(4000, 4050);
      vi.mocked(api.gridRows).mockRejectedValueOnce(new Error('busy'));
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 2500, 2500);
        return 1;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(api.setItemsHidden).toHaveBeenCalledWith([idAt(2500)], true);
      expect(store.selectedItemIds).toEqual([idAt(2501)]);
    });

    /** Asked of an index a scan has rebuilt, the offset before the selection names some
     *  other photo, and landing after it lands somewhere the user never was. */
    it('does not land after a photo fetched from another index than the selection', async () => {
      const store = await storeOf(5000);
      store.selectItem(2500, idAt(2500));
      await store.ensure(4000, 4050);
      vi.mocked(api.gridRows).mockResolvedValueOnce({ version: 2, rows: [entryAt(2400)] });
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 2500, 2500);
        return 1;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(2501)]);
    });

    it('a click made while the photo before the selection is fetched is not overwritten', async () => {
      const store = await storeOf(5000);
      store.selectItem(2500, idAt(2500));
      await store.ensure(0, 50);
      vi.mocked(api.gridRows).mockImplementationOnce(async () => {
        store.selected = 7;
        return { version: 1, rows: [entryAt(2499)] };
      });
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 2500, 2500);
        return 1;
      });
      await store.setHidden([idAt(2500)], true);
      expect(store.selectedItemIds).toEqual([idAt(7)]);
      expect(store.selected).toBe(7);
    });

    /** Ctrl+A, then a photo Ctrl+clicked off and on again: the lead and the anchor are both
     *  inside the folder now. The loaded pages before the lead are all leaving, so the
     *  selection starts no later than where they give out, and the photo there is the one
     *  to ask for - the photo before the anchor is one of the leaving ones. */
    it('hiding a folder whose lead and anchor moved inside it still lands after the folder', async () => {
      const sections: Section[] = [
        { folderId: 1, offset: 0, count: 2400, takenAtMin: 0 },
        { folderId: 2, offset: 2400, count: 2500, takenAtMin: 0 },
        { folderId: 3, offset: 4900, count: 5100, takenAtMin: 0 },
      ];
      const store = await storeOf(10000, { sections });
      await store.ensure(2450, 2550);
      store.selected = 2450;
      await store.selectAll();
      store.toggleSelected(2450);
      store.toggleSelected(2450);
      expect(store.selectionCount).toBe(2500);
      await store.ensure(8000, 8050);
      expect(store.entry(2399)).toBeUndefined();

      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(10000, 2400, 4899);
        return 2500;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(2400);
    });

    /** What the backend holds once a hide of the photos `leaving` names has landed. */
    function hiddenWhere(len: number, leaving: (at: number) => boolean) {
      const kept = Array.from({ length: len }, (_, i) => i).filter((at) => !leaving(at));
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 2,
        rows: kept.slice(offset, offset + count).map(entryAt),
      }));
      vi.mocked(api.gridOffsetOfItem).mockImplementation(async (id: number) => {
        const at = kept.indexOf(id - 100);
        return at < 0 ? null : at;
      });
    }

    /** A folder of 4,800 photos at 100..4899, between two others, with the pages around
     *  offset 2500 loaded: those reach neither end of it. */
    const bigFolder = (at: number) => at >= 100 && at <= 4899;
    async function storeWithBigFolder() {
      const sections: Section[] = [
        { folderId: 1, offset: 0, count: 100, takenAtMin: 0 },
        { folderId: 2, offset: 100, count: 4800, takenAtMin: 0 },
        { folderId: 3, offset: 4900, count: 5100, takenAtMin: 0 },
      ];
      const store = await storeOf(10000, { sections });
      await store.ensure(2450, 2550);
      return store;
    }
    async function bigFolderSelectedAt2500() {
      const store = await storeWithBigFolder();
      store.selected = 2500;
      await store.selectAll();
      return store;
    }

    /** Scrolls the window far away - nothing before the folder stays loaded - and hides
     *  the selection, which is the photos `leaving` names. */
    async function hideFromAfar(store: LibraryStore, leaving: (at: number) => boolean) {
      await store.ensure(8000, 8050);
      expect(store.entry(99)).toBeUndefined();
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenWhere(10000, leaving);
        return store.selectionCount;
      });
      await store.setHidden(store.selectedItemIds, true);
    }

    /** Ctrl+A, Enter, Escape: closing the viewer on the photo it opened keeps the selection
     *  and moves the anchor to the lead, in the middle of the folder. The selection reaches
     *  above its anchor further than the pages kept around the lead, so neither bounds its
     *  start, and the photo at the lead's offset in the rebuilt index is 2,400 photos past
     *  the folder. */
    it('hiding a folder whose anchor moved inside it lands after the folder, whatever is loaded', async () => {
      const store = await bigFolderSelectedAt2500();
      store.closeViewerOn(2500, idAt(2500));
      expect(store.selectionCount).toBe(4800);
      await hideFromAfar(store, bigFolder);
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(100);
    });

    /** The same run picked by Shift+click, with the lead moved inside it by a Ctrl+click off
     *  and on again. */
    it('hiding a shift+clicked run whose lead moved inside it lands after the run', async () => {
      const store = await storeWithBigFolder();
      await store.ensure(0, 50);
      store.selected = 100;
      await store.extendSelection(4899);
      await store.ensure(2450, 2550);
      store.toggleSelected(2500);
      store.toggleSelected(2500);
      expect(store.selectionCount).toBe(4800);
      await hideFromAfar(store, bigFolder);
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(100);
    });

    /** The folder with a photo far above it Ctrl+clicked in, and the lead then moved back
     *  inside the folder. The selection's lowest offset is that photo, but the photos
     *  between it and the folder stay: landing after the photo before *it* would land far
     *  above the lead. What bounds the landing is the start of the run the lead is in. */
    it('hiding a folder plus a photo far above it lands after the folder, not after that photo', async () => {
      const store = await bigFolderSelectedAt2500();
      await store.ensure(0, 50);
      store.toggleSelected(10);
      await store.ensure(2450, 2550);
      store.toggleSelected(2500);
      store.toggleSelected(2500);
      expect(store.selectionCount).toBe(4801);
      await hideFromAfar(store, (at) => at === 10 || bigFolder(at));
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(99);
    });

    /** Ctrl+clicked in right before the folder, the photo joins its run: the one to land
     *  after is the photo before *it*, not the photo before the folder, which is leaving. */
    it('hiding a folder plus the photo just before it lands after the folder', async () => {
      const store = await bigFolderSelectedAt2500();
      await store.ensure(0, 50);
      store.toggleSelected(99);
      await store.ensure(2450, 2550);
      store.toggleSelected(2500);
      store.toggleSelected(2500);
      expect(store.selectionCount).toBe(4801);
      await hideFromAfar(store, (at) => at >= 99 && at <= 4899);
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(99);
    });

    /** A photo Ctrl+clicked out of the folder splits its run: the photos before the gap are
     *  not in the lead's run, and landing after the photo before the folder lands on the
     *  photo left in the gap, far above the lead. */
    it('hiding a folder with a photo taken out of it lands after the folder, not in the gap', async () => {
      const store = await bigFolderSelectedAt2500();
      await store.ensure(950, 1050);
      store.toggleSelected(1000);
      await store.ensure(2450, 2550);
      store.toggleSelected(2500);
      store.toggleSelected(2500);
      expect(store.selectionCount).toBe(4799);
      await hideFromAfar(store, (at) => at !== 1000 && bigFolder(at));
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(101);
    });

    /** An additive band carrying the folder's selection on into the next folder: the lead
     *  is the band's first photo, and the run it is in began with the folder, not the band. */
    it('hiding a folder and an additive band after it lands after the band', async () => {
      const store = await bigFolderSelectedAt2500();
      store.beginBand(true);
      await store.ensure(5050, 5100);
      await store.endBand([[4900, 5100]]);
      expect(store.selected).toBe(4900);
      expect(store.selectionCount).toBe(5001);
      await hideFromAfar(store, (at) => at >= 100 && at <= 5100);
      expect(store.selectedItemIds).toEqual([idAt(5101)]);
      expect(store.selected).toBe(100);
    });

    /** A band that ends without changing the selection leaves it as it was, runs and all. */
    it.each([
      ['cancelled', async (store: LibraryStore) => {
        store.beginBand(false);
        store.bandTo([[2500, 2501]]);
        store.cancelBand();
      }],
      ['released over nothing', async (store: LibraryStore) => {
        store.beginBand(true);
        await store.endBand([]);
      }],
      ['abandoned', async (store: LibraryStore) => {
        store.beginBand(false);
        vi.mocked(api.gridRows).mockResolvedValueOnce({ version: 2, rows: [] });
        await store.endBand([[2500, 2501]]);
      }],
    ])('hiding a folder after a band %s still lands after the folder', async (_, band) => {
      const store = await bigFolderSelectedAt2500();
      store.closeViewerOn(2500, idAt(2500));
      await band(store);
      expect(store.selectionCount).toBe(4800);
      await hideFromAfar(store, bigFolder);
      expect(store.selectedItemIds).toEqual([idAt(4900)]);
      expect(store.selected).toBe(100);
    });

    /** A band begun from a lead, so the lead's page is kept while the window autoscrolls
     *  away: the photo before the band is loaded, the pages after the lead's are not. */
    async function bandFromLead(first: number, last: number) {
      const store = await storeOf(5000);
      store.selected = first;
      store.beginBand(false);
      await store.ensure(2950, 3050);
      await store.endBand([[first, last]]);
      expect(store.entry(first - 1)?.id).toBe(idAt(first - 1));
      expect(store.entry(first + 2 * 200)).toBeUndefined();
      return store;
    }

    it('hiding a band lands on the photo after the one before it, in the rebuilt index', async () => {
      const store = await bandFromLead(10, 3000);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 10, 3000);
        return 2991;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(3001)]);
      expect(store.selected).toBe(10);
    });

    it('hiding a band that runs to the end lands on the photo before it', async () => {
      const store = await bandFromLead(10, 4999);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 10, 4999);
        return 4990;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(9)]);
      expect(store.selected).toBe(9);
    });

    it('lands nowhere when the index moves between finding the photo before and its next', async () => {
      const store = await bandFromLead(10, 3000);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 10, 3000);
        // A scan indexes a photo ahead of 9 between the two asks: offset 9 now holds 8.
        vi.mocked(api.gridOffsetOfItem).mockResolvedValueOnce(10);
        return 2991;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectionCount).toBe(0);
    });

    /** The photos are hidden by then: an error thrown from the lookup told the user a hide
     *  that worked had failed, and left the hidden photos selected. */
    it('a landing lookup that fails after the write lands nowhere, without an error', async () => {
      const store = await bandFromLead(10, 3000);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 10, 3000);
        vi.mocked(api.gridOffsetOfItem).mockRejectedValue(new Error('gone'));
        return 2991;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectionCount).toBe(0);
      expect(store.selected).toBe(null);
    });

    it('hiding a band that starts the view lands on the first photo after it', async () => {
      // Nothing before the lead stays and the pages after it were let go: the rebuilt
      // index's first photo is the first one after the band.
      const store = await storeOf(5000);
      store.beginBand(false);
      await store.ensure(2950, 3050);
      await store.endBand([[0, 3000]]);

      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 0, 3000);
        return 3001;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(3001)]);
      expect(store.selected).toBe(0);
    });

    it('hiding a lead whose page never loaded lands on the photo now at its offset', async () => {
      // "Locate in photon" selects by id, far from anything the grid has fetched.
      const store = await storeOf(5000);
      store.selectItem(2500, idAt(2500));
      await store.ensure(4000, 4050);
      expect(store.entry(2501)).toBeUndefined();

      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 2500, 2500);
        return 1;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(2501)]);
      expect(store.selected).toBe(2500);
    });

    /** The hide's own rebuild re-finds the hidden lead, and answers "gone" only after the
     *  hide has moved the lead on. That stale answer cleared the new lead's id, and the next
     *  rebuild clamped the offset instead of re-finding the photo. */
    it('a rebind that answers after the lead has moved on leaves the new lead alone', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      const oldLead = deferred<number | null>();
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        vi.mocked(api.gridInfo).mockResolvedValue({ ...(await api.gridInfo(null)), version: 2, len: 9 });
        hiddenFrom(10, 2, 2);
        const rebuilt = vi.mocked(api.gridOffsetOfItem).getMockImplementation()!;
        vi.mocked(api.gridOffsetOfItem).mockImplementation((id) => (id === idAt(2) ? oldLead.promise : rebuilt(id)));
        handlers.libraryChanged({ version: 2, len: 9, dataChanged: true });
        await flush();
        return 1;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(3)]);
      oldLead.resolve(null);
      await flush();

      // A photo indexed ahead of it: photo 3 is now at offset 3, which only an id finds.
      vi.mocked(api.gridInfo).mockResolvedValue({ ...(await api.gridInfo(null)), version: 3, len: 10 });
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(3);
      await store.refresh();
      expect(store.selectedItemIds).toEqual([idAt(3)]);
      expect(store.selected).toBe(3);
    });

    /** In Duplicates, hiding one of a pair takes its partner out of the view with it, and
     *  the partner was the photo after it - the landing. Landing on it selected nothing, and
     *  pressing H again did nothing: the run through the duplicates stopped. */
    it('a landing that left the view with the hidden photo moves on to the one now there', async () => {
      const store = await storeOf(10, { view: 'duplicates' });
      store.selected = 2;
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(10, 2, 3);
        return 1;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(4)]);
      expect(store.selected).toBe(2);
    });

    /** The same at the end of the view: the lead's offset is past the rebuilt index's end,
     *  and the photo before it is the one to land on. */
    it('a landing past the rebuilt end moves back to the photo before it', async () => {
      const store = await storeOf(10, { view: 'duplicates' });
      store.selected = 8;
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(10, 8, 9);
        return 1;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(7)]);
      expect(store.selected).toBe(7);
    });

    /** The mirror at the very end: the lead is the last pair's second photo, so the
     *  landing - the photo before it - leaves with it, and the lead's offset is two past the
     *  rebuilt end. Neither the photo there nor the one before it exists; the last photo
     *  still shown is the one to land on. */
    it('a landing two past the rebuilt end lands on the last photo still shown', async () => {
      const store = await storeOf(10, { view: 'duplicates' });
      store.selected = 9;
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        vi.mocked(api.gridInfo).mockResolvedValue({ ...(await api.gridInfo(null)), version: 2, len: 8 });
        hiddenFrom(10, 8, 9);
        return 1;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(7)]);
      expect(store.selected).toBe(7);
    });

    /** A band hidden in Duplicates whose photo before it was a partner of one inside it:
     *  the photo the landing follows has left too, and landing nowhere stopped the run. The
     *  photo now at the lead's offset is near the band's end - one past it here, since the
     *  partner before the lead left as well. */
    it('a landing after a photo that left with the hidden ones lands at the lead', async () => {
      const store = await bandFromLead(10, 3000);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        hiddenFrom(5000, 9, 3000);
        return 2991;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(3002)]);
      expect(store.selected).toBe(10);
    });

    /** A Ctrl+click during the write adds to a selection still holding the photos being
     *  hidden. The click is kept, and the lead with it; the hidden photos are not, or "Hide
     *  3 photos" is offered for two the user can no longer see. */
    it('a ctrl+click made while a hide is out keeps the click and drops the hidden photos', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      store.toggleSelected(3);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        store.toggleSelected(7);
        hiddenFrom(10, 2, 3);
        return 2;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(7)]);
      expect(store.selected).toBe(7);
    });

    /** A band is built on the selection it started from, captured at `beginBand`. Started
     *  additively while the hide was out, that base still held the photos being hidden, and
     *  the band's next frame - or Escape, putting back what it started from - selected them
     *  again. */
    it('an additive band begun while a hide is out does not bring the hidden photos back', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      store.toggleSelected(3);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        store.beginBand(true);
        store.bandTo([[6, 6]]);
        return 2;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(6)]);
      store.bandTo([[6, 7]]);
      expect([...store.selectedItemIds].sort()).toEqual([idAt(6), idAt(7)]);
      store.cancelBand();
      expect(store.selectedItemIds).toEqual([]);
    });

    /** The same band begun but not yet moved: no pick, so the hide lands, and the band's
     *  first frame then builds on its base. */
    it('a band begun but not moved while a hide is out does not bring them back either', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      store.toggleSelected(3);
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        store.beginBand(true);
        hiddenFrom(10, 2, 3);
        return 2;
      });
      await store.setHidden(store.selectedItemIds, true);
      store.bandTo([[6, 6]]);
      expect(store.selectedItemIds).toEqual([idAt(6)]);
    });

    /** A band released while a hide is out: `endBand` holds its base and the selection it
     *  started from across the fetch of its ids, and the hide landing during that fetch
     *  could not reach them there. Resolved, the band put the hidden photos back. */
    async function bandReleasedWhileHiding(answer: { version: number; rows: ReturnType<typeof entryAt>[] }) {
      const store = await storeOf(10);
      store.selected = 2;
      store.toggleSelected(3);
      const write = deferred<number>();
      vi.mocked(api.setItemsHidden).mockReturnValue(write.promise);
      const hiding = store.setHidden(store.selectedItemIds, true);
      await flush();

      // Drawn over one of the photos being hidden, whose tile is still on screen.
      store.beginBand(true);
      store.bandTo([[3, 6]]);
      const fetch = deferred<{ version: number; rows: ReturnType<typeof entryAt>[] }>();
      vi.mocked(api.gridRows).mockReturnValueOnce(fetch.promise);
      const ending = store.endBand([[3, 6]]);
      await flush();

      hiddenFrom(10, 2, 3);
      write.resolve(2);
      await hiding;
      fetch.resolve(answer);
      await ending;
      return store;
    }

    it('a band released while a hide is out does not bring the hidden photos back', async () => {
      // Answered from the index the band was drawn on, which the hide's rebuild has not yet
      // replaced in the store, so it names the photo the hide took out as well.
      const store = await bandReleasedWhileHiding({ version: 1, rows: [3, 4, 5, 6].map(entryAt) });
      expect([...store.selectedItemIds].sort()).toEqual([idAt(4), idAt(5), idAt(6)]);
    });

    /** The same band abandoned, because its fetch answered from the hide's rebuilt index:
     *  the selection it puts back is the one it started from, which held the hidden photos. */
    it('a band abandoned while a hide is out does not put the hidden photos back', async () => {
      const store = await bandReleasedWhileHiding({ version: 2, rows: [entryAt(8)] });
      expect(store.selectedItemIds).toEqual([]);
    });

    it('a click made while a hide is out is not overwritten when it lands', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      vi.mocked(api.setItemsHidden).mockImplementation(async () => {
        store.selected = 7;
        hiddenFrom(10, 2, 2);
        return 1;
      });
      await store.setHidden(store.selectedItemIds, true);
      expect(store.selectedItemIds).toEqual([idAt(7)]);
      expect(store.selected).toBe(7);
    });

    it('a band autoscrolled far from where it began still leads with its first photo', async () => {
      // By the release the grid's window is thousands of photos below the band's first page,
      // which the window has let go; the lead's id must come from the fetch, or the next
      // rebuild clamps the lead instead of re-finding it.
      const store = await storeOf(5000);
      store.beginBand(false);
      await store.ensure(4000, 4050);
      await store.endBand([[10, 4020]]);
      expect(store.selected).toBe(10);

      vi.mocked(api.gridInfo).mockResolvedValue({ ...(await api.gridInfo(null)), version: 2 });
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 2,
        rows: Array.from({ length: Math.min(count, 5000 - offset) }, (_, i) => entryAt(offset + i)),
      }));
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(11);
      await store.refresh();
      expect(api.gridOffsetOfItem).toHaveBeenCalledWith(idAt(10));
      expect(store.selected).toBe(11);
    });

    it('a lead that has left the view is dropped from the selection too', async () => {
      // Hiding (or unstarring in Starred) the photo open in the viewer: the rebuild cannot
      // re-find the lead, and a selection still holding its id would offer "Hide 2 photos"
      // or Compare to a photo that is no longer on screen.
      const store = await storeOf(10);
      store.selected = 3;
      vi.mocked(api.gridInfo).mockResolvedValue({
        version: 2,
        len: 9,
        layout: { generation: 1, sections: [], folders: [] },
        starredCount: 0,
        duplicateCount: 0,
        hiddenCount: 1,
        videoCount: 0,
        view: 'all',
        sort: { key: 'date' as const, reverse: false },
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
        copiesOf: null,
        buildError: null,
      });
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(null);
      await store.refresh();
      expect(store.selectionCount).toBe(0);
      expect(store.selectedItemIds).toEqual([]);
    });

    it('a hide that fails keeps the selection, so the user can try again', async () => {
      const store = await storeOf(10);
      store.selected = 2;
      vi.mocked(api.setItemsHidden).mockRejectedValue(new Error('disk full'));
      await expect(store.setHidden(store.selectedItemIds, true)).rejects.toThrow('disk full');
      expect(store.selectedItemIds).toEqual([idAt(2)]);
    });

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
        expect(store.isSelected(idAt(1201))).toBe(false);

        await store.endBand([[1200, 1202]]);

        expect(store.selectionCount).toBe(3);
        expect(store.isSelected(idAt(1201))).toBe(true);
      });

      /** The status bar's "N selected" reads `selectionCount` while the band is live. An
       *  autoscroll a long way lets the pages behind the window go, and a count of the ids
       *  the preview could resolve fell further behind the band the further it went. */
      it('counts every photo a long band covers, even where its pages were let go', async () => {
        const store = await storeOf(5000);
        store.beginBand(false);
        await store.ensure(2950, 3050);
        expect(store.entry(1000)).toBeUndefined();
        store.bandTo([[10, 3000]]);
        expect(store.selectionCount).toBe(2991);

        await store.endBand([[10, 3000]]);
        expect(store.selectionCount).toBe(2991);
      });

      /** The count read while the release's fetch is out. Zeroed before it, the unloaded
       *  photos dropped out of "N selected" for a round trip and then came back. */
      it("keeps a long band's count while the release resolves it", async () => {
        const store = await storeOf(5000);
        store.beginBand(false);
        await store.ensure(2950, 3050);
        store.bandTo([[10, 3000]]);

        const ending = store.endBand([[10, 3000]]);
        expect(store.selectionCount).toBe(2991);
        await ending;
        expect(store.selectionCount).toBe(2991);
      });

      /** The release recomputes the ranges from the rectangle, and a grid laid out again
       *  since the last preview can make them empty. */
      it('counts nothing unresolved once a band that covers nothing at the release ends', async () => {
        const store = await storeOf(2000);
        store.beginBand(false);
        store.bandTo([[1200, 1202]]);
        expect(store.selectionCount).toBe(3);

        await store.endBand([]);
        expect(store.selectionCount).toBe(0);
      });

      it('counts nothing unresolved once a band the grid was rebuilt under is put back', async () => {
        const store = await storeOf(5000);
        store.selected = 20;
        store.beginBand(false);
        await store.ensure(2950, 3050);
        store.bandTo([[10, 3000]]);
        vi.mocked(api.gridRows).mockResolvedValue({ version: 2, rows: [] });

        await store.endBand([[10, 3000]]);
        expect(store.selectionCount).toBe(1);
      });

      /** A release whose fetch fails - the backend refusing it, the IPC dropped - left the
       *  unresolved count behind for good: "N selected" stayed thousands high over a
       *  selection holding only the preview's loaded photos. */
      it('counts nothing unresolved once a release whose fetch fails ends', async () => {
        const store = await storeOf(2000);
        store.beginBand(false);
        store.bandTo([[1200, 1202]]);
        vi.mocked(api.gridRows).mockRejectedValue(new Error('gone'));

        await expect(store.endBand([[1200, 1202]])).rejects.toThrow('gone');
        expect(store.selectionCount).toBe(0);
      });

      /** Anything that replaces the selection mid-band - a view switch clearing it, a plain
       *  selection - ends what the band counted too. */
      it('counts nothing unresolved once the selection is cleared under a live band', async () => {
        const store = await storeOf(2000);
        store.beginBand(false);
        store.bandTo([[1200, 1202]]);

        store.clearSelection();
        expect(store.selectionCount).toBe(0);
      });

      it('counts only the plain selection made under a live band', async () => {
        const store = await storeOf(2000);
        store.beginBand(false);
        store.bandTo([[1200, 1202]]);

        store.selected = 3;
        expect(store.selectionCount).toBe(1);
      });

      it('counts only what an additive band adds, and nothing once it is abandoned', async () => {
        const store = await storeOf(5000);
        store.selected = 20;
        store.beginBand(true);
        store.bandTo([[1000, 1009]]);
        expect(store.selectionCount).toBe(11);
        store.cancelBand();
        expect(store.selectionCount).toBe(1);
      });

      /** Dragging a box over empty space is how a selection is cleared - the canonical
       *  gesture in every file manager. Answering an empty band like an overtaken fetch put
       *  the old selection back, springing the rings on again after the preview had visibly
       *  taken them off. */
      it('clears the selection when the band covers nothing', async () => {
        const store = await storeOf(20);
        store.selected = 15;

        store.beginBand(false);
        store.bandTo([]);
        await store.endBand([]);

        expect(store.selectionCount).toBe(0);
        expect(store.selected).toBe(null);
      });

      it('leaves an additive band that covers nothing with what it started with', async () => {
        const store = await storeOf(20);
        store.selected = 15;

        store.beginBand(true);
        await store.endBand([]);

        expect(store.selectedItemIds).toEqual([idAt(15)]);
        expect(store.selected).toBe(15);
      });

      /** A grid rebuilt under the drag re-finds the lead by id (`rebindSelection`). A band
       *  that then wrote back the offset it captured when the drag began would undo that -
       *  the photo still rings, but Enter opens its neighbour. */
      it('does not write a pre-rebuild offset back over the lead when a band is abandoned', async () => {
        const store = await storeOf(20);
        store.selected = 15;
        store.beginBand(false);
        store.bandTo([[4, 5]]);
        // The rebuild lands mid-drag and re-finds the lead two offsets along.
        store.selectItem(17, idAt(15));
        vi.mocked(api.gridRows).mockResolvedValueOnce({ version: 2, rows: [entryAt(4)] });

        await store.endBand([[4, 5]]);

        expect(store.selected).toBe(17);
      });

      it('lands the lead and the anchor on the first photo of the band', async () => {
        const store = await storeOf(20);
        // An anchor somewhere else first: `extendSelection` falls back to the lead when
        // there is no anchor, so a band that set only the lead would answer this correctly
        // by accident unless the old anchor is a different, non-null offset.
        store.selected = 15;

        store.beginBand(false);
        await store.endBand([[4, 6]]);
        expect(store.selected).toBe(4);

        // A Shift+click afterwards extends from where the band began, not from offset 15.
        await store.extendSelection(8);
        expect(store.selectionCount).toBe(5);
        expect(store.isSelected(idAt(4))).toBe(true);
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
        layout: { generation: 1, sections: [], folders: [] },
        starredCount: 0,
        duplicateCount: 0,
        hiddenCount: 0,
      videoCount: 0,
        view: 'all',
        sort: { key: 'date' as const, reverse: false },
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
        copiesOf: null,
        buildError: null,
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
        layout: { generation: 1, sections: [], folders: [] },
        starredCount: 0,
        duplicateCount: 0,
        hiddenCount: 0,
      videoCount: 0,
        view: 'all',
        sort: { key: 'date' as const, reverse: false },
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
        copiesOf: null,
        buildError: null,
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

    /** A photo indexed ahead of the lead: every old offset is one later. The rebuild's
     *  re-find of `leadId` is held until the test answers it. */
    async function refreshAroundLead(store: LibraryStore, leadId: number) {
      vi.mocked(api.gridInfo).mockResolvedValue({ ...(await api.gridInfo(null)), version: 2, len: 11 });
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 2,
        rows: Array.from({ length: Math.min(count, 11 - offset) }, (_, i) => entryAt(offset + i - 1)),
      }));
      const found = deferred<number | null>();
      vi.mocked(api.gridOffsetOfItem).mockImplementation((id) =>
        id === leadId ? found.promise : Promise.resolve(null),
      );
      const refreshed = store.refresh();
      await flush();
      expect(api.gridOffsetOfItem).toHaveBeenCalledWith(leadId);
      return { found, refreshed };
    }

    /** A Shift+click while the rebuild re-finds the lead moves the lead and leaves the
     *  anchor, so the rebind answers about a photo that is no longer the lead and returns
     *  early - and the anchor, which named the clicked photo by its old offset, was left
     *  there: the next Shift+click ranged from the photo before the one clicked. */
    it('a shift+click while the lead is re-found still carries the anchor with it', async () => {
      const store = await storeOf(10);
      store.selected = 5;
      const { found, refreshed } = await refreshAroundLead(store, idAt(5));
      await store.extendSelection(8);
      found.resolve(6);
      await refreshed;

      await store.extendSelection(9);
      expect([...store.selectedItemIds].sort()).toEqual([idAt(5), idAt(6), idAt(7), idAt(8)]);
    });

    /** A plain click on the offset the lead had, while it is re-found: the anchor now names
     *  the photo clicked there, and must not be moved as though it were the old lead. */
    it('a click at the lead\'s old offset while it is re-found keeps its own anchor', async () => {
      const store = await storeOf(10);
      store.selected = 5;
      const { found, refreshed } = await refreshAroundLead(store, idAt(5));
      store.selected = 5;
      expect(store.selectedItemIds).toEqual([idAt(4)]);
      found.resolve(6);
      await refreshed;

      await store.extendSelection(9);
      expect([...store.selectedItemIds].sort()).toEqual([idAt(4), idAt(5), idAt(6), idAt(7), idAt(8)]);
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
        layout: { generation: 1, sections: [], folders: [] },
        starredCount: 0,
        duplicateCount: 0,
        hiddenCount: 0,
      videoCount: 0,
        view: 'all',
        sort: { key: 'date' as const, reverse: false },
        searchQuery: '',
        person: null,
        album: null,
        tag: null,
        copiesOf: null,
        buildError: null,
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
      vi.mocked(api.setGridView).mockResolvedValue(null);

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

    it('the viewer closing far from the grid selects its photo by id, and a rebuild re-finds it', async () => {
      // The viewer reached photo 3000 through ensureAt; the grid behind it never went there,
      // and a rebuild dropped every page but the grid's window. The page holding 3000 is gone.
      const store = await storeOf(5000);
      store.selected = 10;
      await store.ensureAt(3000);
      vi.mocked(api.gridInfo).mockResolvedValue({ ...(await api.gridInfo(null)), version: 2 });
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 2,
        rows: Array.from({ length: Math.min(count, 5000 - offset) }, (_, i) => entryAt(offset + i)),
      }));
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(10);
      await store.refresh();
      expect(store.entry(3000)).toBeUndefined();

      store.closeViewerOn(3000, idAt(3000));
      expect(store.selectedItemIds).toEqual([idAt(3000)]);

      // A photo indexed ahead of it: the lead follows it by id rather than clamping.
      vi.mocked(api.gridInfo).mockResolvedValue({ ...(await api.gridInfo(null)), version: 3 });
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 3,
        rows: Array.from({ length: Math.min(count, 5000 - offset) }, (_, i) => entryAt(offset + i)),
      }));
      vi.mocked(api.gridOffsetOfItem).mockResolvedValue(3001);
      await store.refresh();
      expect(api.gridOffsetOfItem).toHaveBeenLastCalledWith(idAt(3000));
      expect(store.selected).toBe(3001);
    });

    it('the viewer closing with no photo to name falls back to the offset', async () => {
      const store = await storeOf(10);
      store.selected = 3;
      store.toggleSelected(4);
      store.closeViewerOn(4, null); // closed on the lead: the selection stays
      expect(store.selectionCount).toBe(2);
      store.closeViewerOn(6, null);
      expect(store.selectedItemIds).toEqual([idAt(6)]);
    });

    it('keeps the viewer’s page while it is open: through the grid scrolling, and across a rebuild', async () => {
      const store = await storeOf(5000);
      store.setViewing(3000);
      await store.ensureAt(3000);
      await store.ensure(0, 50);
      expect(store.entry(3000)?.id).toBe(idAt(3000));

      vi.mocked(api.gridInfo).mockResolvedValue({ ...(await api.gridInfo(null)), version: 2 });
      vi.mocked(api.gridRows).mockImplementation(async (offset: number, count: number) => ({
        version: 2,
        rows: Array.from({ length: Math.min(count, 5000 - offset) }, (_, i) => entryAt(offset + i)),
      }));
      await store.refresh();
      expect(store.entry(3000)?.id).toBe(idAt(3000));
      expect(store.entry(0)?.id).toBe(idAt(0));

      // Closed: its page is the grid's to let go of again.
      store.setViewing(null);
      await store.ensure(0, 50);
      expect(store.entry(3000)).toBeUndefined();
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

      it('takes the lead photo\'s folder under a flat sort, wherever its photos are', async () => {
        // Sorted by size, All is one headerless run and a folder's photos are scattered
        // through it: still the folder, never the library.
        const flat = [{ folderId: null, offset: 0, count: 15, takenAtMin: 0 }];
        const store = await storeOf(15, { sections: flat });
        store.selected = 7;
        vi.mocked(api.gridFolderIdsAt).mockResolvedValueOnce({ version: 1, ids: [idAt(2), idAt(7), idAt(12)] });

        await store.selectAll();

        expect(api.gridFolderIdsAt).toHaveBeenCalledWith(7);
        expect([...store.selectedItemIds].sort((a, b) => a - b)).toEqual([2, 7, 12].map(idAt));
        expect(store.selected).toBe(7);
      });

      it('drops a flat folder answered against another version of the grid', async () => {
        const flat = [{ folderId: null, offset: 0, count: 15, takenAtMin: 0 }];
        const store = await storeOf(15, { sections: flat });
        store.selected = 7;
        vi.mocked(api.gridFolderIdsAt).mockResolvedValueOnce({ version: 2, ids: [idAt(2), idAt(7), idAt(12)] });

        await store.selectAll();

        expect(store.selectionCount).toBe(1);
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

  it('drops a second copy of the same photo while the first is still rendering', async () => {
    // A copy takes a second or two; an impatient second press must not queue another
    // full-size decode of the same photo, nor say "Photo copied" twice.
    let finish!: () => void;
    vi.mocked(api.copyPhoto).mockReset().mockReturnValue(new Promise<void>((r) => (finish = r)));
    const store = new LibraryStore();
    const first = store.copyPhoto(7);
    const second = store.copyPhoto(7);
    await second;
    expect(api.copyPhoto).toHaveBeenCalledTimes(1);
    finish();
    await first;
    expect(store.toasts.map((t) => t.message)).toEqual(['Photo copied']);

    // Once it has landed, the same photo can be copied again.
    vi.mocked(api.copyPhoto).mockResolvedValue(undefined);
    await store.copyPhoto(7);
    expect(api.copyPhoto).toHaveBeenCalledTimes(2);
  });
});

