/** The only module that talks to the Rust side. Types mirror the serde structs in
 *  crates/photon-app/src/commands.rs and events.rs (camelCase). */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export { mediaUrl } from './url';

export interface WatchedFolder { id: number; path: string; online: boolean }
export interface Folder { id: number; watchedId: number; parentId: number | null; path: string; name: string }
export interface FolderList { watched: WatchedFolder[]; folders: Folder[] }
/** `takenAtMin` is the capture time of the folder's OLDEST photo, in SECONDS (multiply by
 *  1000 for a JS Date). The sidebar groups folders by the year it falls in. Oldest rather
 *  than newest, to match Picasa. */
export interface Section { folderId: number; offset: number; count: number; takenAtMin: number }
export interface GridEntry { id: number; folderId: number; takenAt: number; aspect: number; kind: 'image'; thumbKey: string; starred: boolean }
export type GridView = 'all' | 'starred' | 'search';
export interface GridInfo { version: number; len: number; sections: Section[]; starredCount: number; view: GridView; searchQuery: string }
export interface GridRows { version: number; rows: GridEntry[] }
export interface ViewerItem {
  id: number;
  path: string;
  fileName: string;
  width: number;
  height: number;
  orientation: number;
  takenAt: number;
  thumbKey: string;
  thumbState: 'pending' | 'ready' | 'failed';
  thumbError: string | null;
}
export interface ScanProgressEvent {
  watchedId: number;
  filesSeen: number;
  added: number;
  changed: number;
  done: boolean;
  cancelled: boolean;
}
export interface FolderStatus { watchedId: number; online: boolean; degraded: boolean }
export interface LibraryChanged { version: number; len: number }
export interface AppError { kind: string; message: string }

export const api = {
  listFolders: () => invoke<FolderList>('list_folders'),
  addFolder: (path: string) => invoke<WatchedFolder>('add_folder', { path }),
  removeFolder: (watchedId: number) => invoke<void>('remove_folder', { watchedId }),
  rescanFolder: (watchedId: number) => invoke<void>('rescan_folder', { watchedId }),
  gridInfo: () => invoke<GridInfo>('grid_info'),
  gridRows: (offset: number, count: number) => invoke<GridRows>('grid_rows', { offset, count }),
  gridOffsetOfFolder: (folderId: number) => invoke<number | null>('grid_offset_of_folder', { folderId }),
  setGridView: (view: GridView) => invoke<void>('set_grid_view', { view }),
  setSearchQuery: (query: string) => invoke<void>('set_search_query', { query }),
  setVisible: (ids: number[]) => invoke<void>('set_visible', { ids }),
  viewerItem: (id: number) => invoke<ViewerItem>('viewer_item', { id }),
  neighbours: (id: number, radius: number) => invoke<number[]>('neighbours', { id, radius }),
  revealInFileManager: (id: number) => invoke<void>('reveal_in_file_manager', { id }),
  revealFolder: (folderId: number) => invoke<void>('reveal_folder', { folderId }),
};

export const events = {
  onLibraryChanged: (cb: (e: LibraryChanged) => void): Promise<UnlistenFn> =>
    listen<LibraryChanged>('library-changed', (e) => cb(e.payload)),
  onScanProgress: (cb: (e: ScanProgressEvent) => void): Promise<UnlistenFn> =>
    listen<ScanProgressEvent>('scan-progress', (e) => cb(e.payload)),
  onFolderStatus: (cb: (e: FolderStatus) => void): Promise<UnlistenFn> =>
    listen<FolderStatus>('folder-status', (e) => cb(e.payload)),
};

export function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
  return String(e);
}
