import {
  api,
  errorMessage,
  events,
  type Folder,
  type FolderList,
  type GridEntry,
  type GridInfo,
  type GridView,
  type ScanProgressEvent,
} from './api';
import { PageCache } from './pages';
import type { UnlistenFn } from '@tauri-apps/api/event';

export interface Toast { id: number; message: string }

/** App-wide reactive state: the grid snapshot, the folder tree, scan status and selection. */
export class LibraryStore {
  info = $state<GridInfo>({ version: -1, len: 0, sections: [], starredCount: 0, view: 'all', searchQuery: '' });
  folders = $state<FolderList>({ watched: [], folders: [] });
  scans = $state<Record<number, ScanProgressEvent>>({});
  /** Watched folder ids the OS won't let photon watch live, from the most recent
   *  `folder-status` event for each: they fall back to periodic rescans instead. */
  degraded = $state<Record<number, boolean>>({});
  /** Selected grid offset. */
  selected = $state<number | null>(null);
  /** Bumped whenever pages arrive, so `entry()` readers re-run. */
  pageTick = $state(0);
  errors = $state<Toast[]>([]);

  private folderById = $derived(new Map(this.folders.folders.map((f) => [f.id, f])));
  private onlineByWatched = $derived(new Map(this.folders.watched.map((w) => [w.id, w.online])));
  private pages = new PageCache<GridEntry>((o, c) => api.gridRows(o, c), () => void this.refresh().catch(this.reportError));
  private unlisten: UnlistenFn[] = [];
  private nextToast = 0;
  private initPromise: Promise<void> | null = null;
  /** Bumped by `dispose()` so an in-flight `init()` can tell it was cancelled. */
  private generation = 0;

  /** Idempotent and safe to call again (root remount, HMR): a second call while
   *  already initialised/initialising is a no-op, and never leaves more than the
   *  three event subscriptions registered here. */
  init(): Promise<void> {
    if (this.initPromise) return this.initPromise;
    const generation = ++this.generation;
    this.initPromise = (async () => {
      const unlisten = await Promise.all([
        events.onLibraryChanged(() => void this.refresh().catch(this.reportError)),
        events.onFolderStatus((e) => {
          this.degraded[e.watchedId] = e.degraded;
          void this.refreshFolders().catch(this.reportError);
        }),
        events.onScanProgress((e) => {
          this.scans[e.watchedId] = e;
          if (e.done) void this.refreshFolders().catch(this.reportError);
        }),
      ]);
      if (generation !== this.generation) {
        // dispose() ran while we were subscribing: undo it instead of leaking.
        for (const u of unlisten) u();
        return;
      }
      this.unlisten = unlisten;
      await Promise.all([this.refresh(), this.refreshFolders()]);
    })();
    return this.initPromise;
  }

  dispose(): void {
    this.generation++;
    this.initPromise = null;
    for (const u of this.unlisten) u();
    this.unlisten = [];
    this.degraded = {};
  }

  async refresh(): Promise<void> {
    const info = await api.gridInfo();
    if (info.version < this.info.version) return;
    this.pages.reset(info.version);
    this.info = info;
    if (this.selected !== null && this.selected >= info.len) this.selected = info.len ? info.len - 1 : null;
    this.pageTick++;
  }

  async refreshFolders(): Promise<void> {
    this.folders = await api.listFolders();
  }

  /** Switches which photos the grid shows. The backend rebuilds its index, so the grid is
   *  reloaded from scratch rather than patched. */
  async setView(view: GridView): Promise<void> {
    try {
      await api.setGridView(view);
      await this.refresh();
    } catch (e) {
      this.reportError(e);
    }
  }

  /** Chains each `setSearchQuery` call onto the previous one, so two `setSearchQuery` calls
   *  issued close together (for example a debounced search followed by Escape clearing it)
   *  apply to the backend in the order they were issued rather than in whatever order their
   *  IPC round trips happen to finish. This says nothing about `setView`, which is not on
   *  this chain. `cancel()` on the debouncer only stops a call that hasn't fired yet — a
   *  call already dispatched cannot be cancelled, so ordering has to be guaranteed here
   *  instead. */
  private searchQueryChain: Promise<void> = Promise.resolve();

  /** Searches for `query`. A blank query returns the backend to the All view. */
  setSearchQuery(query: string): Promise<void> {
    const next = this.searchQueryChain.then(async () => {
      try {
        await api.setSearchQuery(query);
        await this.refresh();
      } catch (e) {
        this.reportError(e);
      }
    });
    // Stored separately from what's returned: `next` never rejects today because the
    // try/catch above routes every failure into reportError, but the chain must not depend
    // on that staying true. If some future failure ever did throw past this point, a bare
    // `this.searchQueryChain = next` would leave the chain permanently rejected and every
    // later setSearchQuery call would reject with it — search would go silently dead for
    // the rest of the session. `.catch(() => {})` keeps the chain alive regardless; callers
    // still see `next`, so a real failure is still reported and still rejects for them.
    this.searchQueryChain = next.catch(() => {});
    return next;
  }

  async ensure(start: number, end: number): Promise<void> {
    if (await this.pages.ensure(start, Math.min(end, this.info.len))) this.pageTick++;
  }

  entry(offset: number): GridEntry | undefined {
    void this.pageTick;
    return this.pages.get(offset);
  }

  folderOf(folderId: number): Folder | undefined {
    return this.folderById.get(folderId);
  }

  isOnline(folderId: number): boolean {
    const folder = this.folderById.get(folderId);
    return folder ? (this.onlineByWatched.get(folder.watchedId) ?? true) : true;
  }

  isScanning(watchedId: number): boolean {
    const scan = this.scans[watchedId];
    return !!scan && !scan.done;
  }

  /** True while any *currently watched* folder is relying on periodic rescans instead of
   *  live filesystem events, so the status bar can say live updates are limited.
   *
   *  Filtered against `folders.watched` rather than read straight off `degraded`: removing
   *  a folder emits no `folder-status` event (only a grid refresh), so a stale `true` entry
   *  for its id would otherwise survive the removal and the notice would never clear. */
  get anyDegraded(): boolean {
    return this.folders.watched.some((w) => this.degraded[w.id]);
  }

  reportError = (e: unknown): void => {
    const id = this.nextToast++;
    this.errors.push({ id, message: errorMessage(e) });
    setTimeout(() => this.dismissError(id), 6000);
  };

  dismissError(id: number): void {
    this.errors = this.errors.filter((t) => t.id !== id);
  }
}

export const library = new LibraryStore();
