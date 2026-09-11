import { api, errorMessage, events, type Folder, type FolderList, type GridEntry, type GridInfo, type ScanProgressEvent } from './api';
import { PageCache } from './pages';
import type { UnlistenFn } from '@tauri-apps/api/event';

export interface Toast { id: number; message: string }

/** App-wide reactive state: the grid snapshot, the folder tree, scan status and selection. */
export class LibraryStore {
  info = $state<GridInfo>({ version: -1, len: 0, sections: [] });
  folders = $state<FolderList>({ watched: [], folders: [] });
  scans = $state<Record<number, ScanProgressEvent>>({});
  /** Selected grid offset. */
  selected = $state<number | null>(null);
  /** Bumped whenever pages arrive, so `entry()` readers re-run. */
  pageTick = $state(0);
  errors = $state<Toast[]>([]);

  private folderById = $derived(new Map(this.folders.folders.map((f) => [f.id, f])));
  private onlineByWatched = $derived(new Map(this.folders.watched.map((w) => [w.id, w.online])));
  private pages = new PageCache<GridEntry>((o, c) => api.gridRows(o, c), () => void this.refresh());
  private unlisten: UnlistenFn[] = [];
  private nextToast = 0;

  async init(): Promise<void> {
    this.unlisten = await Promise.all([
      events.onLibraryChanged(() => void this.refresh()),
      events.onFolderStatus(() => void this.refreshFolders()),
      events.onScanProgress((e) => {
        this.scans[e.watchedId] = e;
        if (e.done) void this.refreshFolders();
      }),
    ]);
    await Promise.all([this.refresh(), this.refreshFolders()]);
  }

  dispose(): void {
    for (const u of this.unlisten) u();
    this.unlisten = [];
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
