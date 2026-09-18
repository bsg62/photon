/** The only module that talks to the Rust side. Types mirror the serde structs in
 *  crates/photon-app/src/commands.rs and events.rs (camelCase). */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

export { mediaUrl } from './url';

export interface WatchedFolder { id: number; path: string; online: boolean }
export interface Folder { id: number; watchedId: number; parentId: number | null; path: string; name: string }
export interface FolderList { watched: WatchedFolder[]; folders: Folder[] }
/** A root with no photos is absent from the list. */
export interface WatchedFolderStats { watchedId: number; photoCount: number }
export interface AppInfo { version: string; libraryPath: string; licence: string }
/** `takenAtMin` is the capture time of the folder's OLDEST photo, in SECONDS (multiply by
 *  1000 for a JS Date). The sidebar groups folders by the year it falls in. Oldest rather
 *  than newest, to match Picasa. */
export interface Section { folderId: number; offset: number; count: number; takenAtMin: number }
export interface GridEntry { id: number; folderId: number; takenAt: number; aspect: number; kind: 'image'; thumbKey: string; starred: boolean }
export type GridView = 'all' | 'starred' | 'recent' | 'search' | 'person' | 'album' | 'tag' | 'duplicates';
/** `searchQuery`, `person`, `album` and `tag` are the argument of the matching view and
 *  empty/null in every other view: the backend is the source of truth for which one is
 *  active, so the UI reads the argument from here rather than remembering what it asked for. */
export interface GridInfo {
  version: number;
  len: number;
  sections: Section[];
  starredCount: number;
  /** Photos with a byte-identical twin elsewhere in the library. */
  duplicateCount: number;
  view: GridView;
  searchQuery: string;
  /** Picasa contact hash while `view` is 'person'. */
  person: string | null;
  /** Album id while `view` is 'album'. */
  album: number | null;
  /** Keyword while `view` is 'tag'. */
  tag: string | null;
}
export interface GridRows { version: number; rows: GridEntry[] }
/** A named Picasa face; the rectangle is fractions of the displayed (oriented) image. */
export interface ItemFace { hash: string; name: string; left: number; top: number; right: number; bottom: number }
export interface ViewerItem {
  id: number;
  path: string;
  fileName: string;
  width: number;
  height: number;
  orientation: number;
  takenAt: number;
  /** File size in bytes. */
  size: number;
  thumbKey: string;
  thumbState: 'pending' | 'ready' | 'failed';
  thumbError: string | null;
  starred: boolean;
  make: string | null;
  model: string | null;
  lens: string | null;
  /** Millimetres, as shot. */
  focalMm: number | null;
  /** The f-number. */
  aperture: number | null;
  /** Seconds. */
  exposureS: number | null;
  iso: number | null;
  /** Keywords from the file's XMP and IPTC, in file order. */
  tags: string[];
  faces: ItemFace[];
  /** Ids of the albums the photo is in. */
  albums: number[];
  /** Other files with the same bytes as this one. */
  copies: ItemCopy[];
  /** The picture turned but not cropped: what `image/<id>/uncropped` serves. */
  uncroppedWidth: number;
  uncroppedHeight: number;
  /** What the user has done to the photo in photon; null for an untouched one. For an
   *  edited photo `width`, `height`, `orientation` and `faces` describe the picture as
   *  shown, because the edit is rendered into every image the backend serves. */
  edit: ItemEdit | null;
}
/** `crop` is `[left, top, right, bottom]` in 1/65535ths of the turned picture. */
export interface ItemEdit { turns: number; crop: [number, number, number, number] | null }
export interface ItemCopy { id: number; path: string }
export interface Person { hash: string; name: string; count: number }
export interface TagCount { tag: string; count: number }
/** A tag the user renamed (`target` set) or removed (`target` null). photon applies it
 *  when reading tags; the photo files keep their keywords. */
export interface TagRule { tag: string; target: string | null }
export interface Album { id: number; name: string; createdMs: number }
export interface AlbumSummary { id: number; name: string; count: number }
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
  watchedFolderStats: () => invoke<WatchedFolderStats[]>('watched_folder_stats'),
  appInfo: () => invoke<AppInfo>('app_info'),
  /** Reveals a watched root itself; unlike `revealFolder` it works for a root that has no
   *  folder row yet (offline, or never scanned). */
  revealWatched: (watchedId: number) => invoke<void>('reveal_watched', { watchedId }),
  revealLibrary: () => invoke<void>('reveal_library'),
  gridInfo: () => invoke<GridInfo>('grid_info'),
  gridRows: (offset: number, count: number) => invoke<GridRows>('grid_rows', { offset, count }),
  gridOffsetOfFolder: (folderId: number) => invoke<number | null>('grid_offset_of_folder', { folderId }),
  /** Where an item sits in the current grid, or null if this view no longer holds it. The
   *  viewer uses it to re-find the photo it is showing after the index is rebuilt. */
  gridOffsetOfItem: (itemId: number) => invoke<number | null>('grid_offset_of_item', { itemId }),
  lastFolder: () => invoke<number | null>('last_folder'),
  setLastFolder: (folderId: number) => invoke<void>('set_last_folder', { folderId }),
  /** The window's own fullscreen, granted in `capabilities/default.json`. */
  windowFullscreen: () => getCurrentWindow().isFullscreen(),
  setWindowFullscreen: (on: boolean) => getCurrentWindow().setFullscreen(on),
  slideshowInterval: () => invoke<number>('slideshow_interval'),
  /** Resolves to the clamped value the backend stored. */
  setSlideshowInterval: (seconds: number) => invoke<number>('set_slideshow_interval', { seconds }),
  setGridView: (view: GridView) => invoke<void>('set_grid_view', { view }),
  setSearchQuery: (query: string) => invoke<void>('set_search_query', { query }),
  setPersonView: (contact: string) => invoke<void>('set_person_view', { contact }),
  setAlbumView: (albumId: number) => invoke<void>('set_album_view', { albumId }),
  setTagView: (tag: string) => invoke<void>('set_tag_view', { tag }),
  listPeople: () => invoke<Person[]>('list_people'),
  listTags: () => invoke<TagCount[]>('list_tags'),
  listTagRules: () => invoke<TagRule[]>('list_tag_rules'),
  /** Renames a tag, merging it into `to` if that already exists. */
  renameTag: (from: string, to: string) => invoke<void>('rename_tag', { from, to }),
  hideTag: (tag: string) => invoke<void>('hide_tag', { tag }),
  restoreTagRule: (tag: string) => invoke<void>('restore_tag_rule', { tag }),
  /** Adds a tag to one photo. Resolves to the name stored, which a rename rule can make
   *  different from what was typed. */
  addItemTag: (id: number, tag: string) => invoke<string>('add_item_tag', { id, tag }),
  removeItemTag: (id: number, tag: string) => invoke<void>('remove_item_tag', { id, tag }),
  listAlbums: () => invoke<AlbumSummary[]>('list_albums'),
  createAlbum: (name: string) => invoke<Album>('create_album', { name }),
  renameAlbum: (albumId: number, name: string) => invoke<void>('rename_album', { albumId, name }),
  deleteAlbum: (albumId: number) => invoke<void>('delete_album', { albumId }),
  addToAlbum: (albumId: number, itemIds: number[]) => invoke<void>('add_to_album', { albumId, itemIds }),
  removeFromAlbum: (albumId: number, itemIds: number[]) => invoke<void>('remove_from_album', { albumId, itemIds }),
  setVisible: (ids: number[]) => invoke<void>('set_visible', { ids }),
  viewerItem: (id: number) => invoke<ViewerItem>('viewer_item', { id }),
  /** Sets or clears a star. Written into the folder's Picasa INI first, then mirrored into
   *  the library; the grid rebuilds and `library-changed` follows. */
  setStar: (id: number, starred: boolean) => invoke<void>('set_star', { id, starred }),
  /** Turns the photo a quarter; the crop goes round with it. Nothing is written to the file. */
  rotateItem: (id: number, clockwise: boolean) => invoke<void>('rotate_item', { id, clockwise }),
  /** Replaces the photo's edit; no turns and no crop is the original again. */
  setItemEdit: (id: number, turns: number, crop: [number, number, number, number] | null) =>
    invoke<void>('set_item_edit', { id, turns, crop }),
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
