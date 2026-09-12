import { beforeEach, describe, expect, it, vi } from 'vitest';

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
    vi.mocked(api.gridInfo).mockResolvedValue({ version: 1, len: 0, sections: [] });
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
    handlers.folderStatus({ watchedId: 1, online: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.errors.some((t) => t.message === 'folders-fail')).toBe(true);

    vi.mocked(api.listFolders).mockRejectedValueOnce(new Error('scan-done-fail'));
    handlers.scanProgress({ watchedId: 1, filesSeen: 1, added: 0, changed: 0, done: true, cancelled: false });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.errors.some((t) => t.message === 'scan-done-fail')).toBe(true);
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
    const gridInfoGate = deferred<{ version: number; len: number; sections: never[] }>();
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
    gridInfoGate.resolve({ version: 1, len: 0, sections: [] });
    await initPromise;

    expect(unlistenCounts.libraryChanged).toBe(1);
    expect(unlistenCounts.folderStatus).toBe(1);
    expect(unlistenCounts.scanProgress).toBe(1);
  });
});
