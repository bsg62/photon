/** The only module that talks to the Rust side. Types mirror the serde structs in
 *  crates/photon-app/src/commands.rs and events.rs (camelCase). */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';

export { mediaUrl } from './url';

export interface WatchedFolder { id: number; path: string; online: boolean }
/** `hidden`: the user hid the folder - its photos, and any added to it later, are hidden. */
/** `alias` is the user's name for the folder in photon, shown in place of `name` (see
 *  `folderLabel`); null for none. */
export interface Folder { id: number; watchedId: number; parentId: number | null; path: string; name: string; hidden: boolean; alias: string | null }
/** One step of a drag from outside the window; see `api.onFileDrag`. */
export type FileDrag = { type: 'enter' | 'drop'; paths: string[] } | { type: 'leave' };
export interface FolderList { watched: WatchedFolder[]; folders: Folder[] }
/** A root with no photos is absent from the list. */
export interface WatchedFolderStats { watchedId: number; photoCount: number }
/** Mirrors `library::LibraryStats`: the visible library, counted. `oldest`/`newest` are
 *  capture times in naive SECONDS like `takenAt`, null for an empty library. `cameras` and
 *  `lenses` are the ten most used, most first; `noCamera` counts files that name none. */
export interface LibraryStats {
  photos: number;
  videos: number;
  bytes: number;
  oldest: number | null;
  newest: number | null;
  years: { year: number; count: number }[];
  cameras: { make: string | null; model: string | null; count: number }[];
  noCamera: number;
  lenses: { lens: string; count: number }[];
}
export interface AppInfo { version: string; libraryPath: string; licence: string }
/** `includesWebview` is false on macOS, where the web view's processes cannot be told apart
 *  from other apps' and `bytes` is photon's own process alone. */
export interface MemoryUsage { bytes: number; processes: number; includesWebview: boolean }
/** Mirrors `grid::Section`: a run the grid lays out. `folderId` is null for a flat view's one
 *  run, which spans many folders and is drawn with no header. `takenAtMin` is in SECONDS. */
export interface Section { folderId: number | null; offset: number; count: number; takenAtMin: number }
/** Mirrors `grid::FolderTally`: one folder's photos in the view, whatever the layout.
 *  `takenAtMin` is the capture time of the folder's OLDEST photo, in SECONDS (multiply by
 *  1000 for a JS Date). The sidebar groups folders by the year it falls in. Oldest rather
 *  than newest, to match Picasa.
 *  `bytes` totals the folder's photos in the view and `modifiedMs` is the newest file
 *  modification among them, in MILLISECONDS: the sidebar's size and modified orders. */
export interface FolderTally { folderId: number; count: number; takenAtMin: number; bytes: number; modifiedMs: number }
/** Mirrors `sort::SortKey`. */
export type SortKey = 'date' | 'modified' | 'name' | 'size';
/** Mirrors `sort::Sort`: what every view is sorted by. `date` keeps the folder sections;
 *  any other key lays the grid out flat. `reverse` turns the whole order over. */
export interface Sort { key: SortKey; reverse: boolean }
/** `hasCopies`: another live file has the same bytes or is a look-alike, the same rule the
 *  Duplicates view uses (`GridEntry::has_copies`). `durationMs` is a video's running time;
 *  null for a photo. */
export interface GridEntry { id: number; folderId: number; takenAt: number; aspect: number; kind: 'image' | 'video'; durationMs: number | null; thumbKey: string; starred: boolean; hasCopies: boolean }
export type GridView = 'all' | 'starred' | 'recent' | 'search' | 'person' | 'album' | 'tag' | 'duplicates' | 'copies' | 'hidden' | 'videos';
/** Mirrors `commands::CopiesOf`. `fileName` is empty once the photo has left the library;
 *  `gone` is true once the anchor photo itself is gone (purged or missing) - the filter
 *  keys off the anchor's own row, so once it is gone every branch matches nothing and the
 *  grid empties even though the other copies are still live. `hidden` is true once the user
 *  has hidden the anchor: its copies stay in the view, and it does not. */
export interface CopiesOf { id: number; fileName: string; gone: boolean; hidden: boolean }
/** Mirrors `photon_core::library::ThemeChoice` (serde lowercase). */
export type ThemeChoice = 'system' | 'light' | 'dark';
/** Mirrors `photon_core::library::GridTile` (serde lowercase). */
export type GridTile = 'small' | 'medium' | 'large';
/** `searchQuery`, `person`, `album`, `tag` and `copiesOf` are the argument of the matching
 *  view and empty/null in every other view: the backend is the source of truth for which one
 *  is active, so the UI reads the argument from here rather than remembering what it asked for. */
/** Mirrors `commands::GridLayout`: the runs the grid lays out and the folders the sidebar
 *  lists, at one generation. */
export interface GridLayout {
  generation: number;
  sections: Section[];
  /** The folders the view's photos come from - the sidebar's list. */
  folders: FolderTally[];
}
export interface GridInfo {
  version: number;
  len: number;
  /** Null when the call named this generation as the one it holds: the store keeps its own. */
  layout: GridLayout | null;
  starredCount: number;
  /** Photos with a byte-identical twin elsewhere in the library. */
  duplicateCount: number;
  /** Photos the user has hidden; the sidebar shows the Hidden row only above 0. */
  hiddenCount: number;
  /** Visible videos; the sidebar shows the Videos row only above 0. */
  videoCount: number;
  view: GridView;
  sort: Sort;
  searchQuery: string;
  /** The key (`Person.key`) of the person on show while `view` is 'person'. */
  person: string | null;
  /** Album id while `view` is 'album'. */
  album: number | null;
  /** Keyword while `view` is 'tag'. */
  tag: string | null;
  /** The photo while `view` is 'copies'. */
  copiesOf: CopiesOf | null;
  /** Why the grid is empty when it is only because photon could not read the library at
   *  startup (`Engine::build_first_grid`); null for every grid actually built. */
  buildError: string | null;
}
export interface GridRows { version: number; rows: GridEntry[] }
/** Mirrors `commands::FolderIds`: one folder's photos, and the index version they are of. */
export interface FolderIds { version: number; ids: number[] }

/** Mirrors `face_detect::Rect`: fractions of the picture, from its left and top. */
export interface FaceRect { left: number; top: number; right: number; bottom: number }

/** A named face: Picasa's, under the person's key and name when its contact is linked to one,
 *  or a person's confirmed detection. The rectangle is fractions of the picture as shown.
 *  `faceId` is the `detected_faces.id` naming or un-naming this plate acts on: a detection's own
 *  id, or for Picasa's plate of a linked person the detection beneath it confirmed as that
 *  person. `null` for a plate of a contact no person is linked to (the face beneath may be
 *  someone else's) and for a Picasa plate with no detection of that person beneath it. */
export interface ItemFace { key: string; name: string; left: number; top: number; right: number; bottom: number; faceId: number | null }

/** A face with no name. `faceId` is the `detected_faces.id` naming it would name: the detection
 *  itself, or for Picasa's outline the detection beneath it that no named person is confirmed
 *  on. `null` when Picasa's outline has no detection beneath it (detection off, or missed). */
export interface UnnamedFace { left: number; top: number; right: number; bottom: number; faceId: number | null }
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
  /** Whether the user has hidden the photo: the menu offers the opposite, and Locate
   *  looks for it in the Hidden view. */
  hidden: boolean;
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
  /** The caption the photo carries (XMP or IPTC), shown under it. */
  caption: string | null;
  faces: ItemFace[];
  /** Faces with no name: Picasa's unnamed ones, then the ones photon detected that are none
   *  of Picasa's. A detection confirmed as a named person is in `faces` instead, and takes an
   *  unnamed Picasa face under it along. Fractions of the picture as shown, like `faces`. */
  unnamedFaces: UnnamedFace[];
  /** A video plays; the viewer shows no zoom, crop or turn for it. */
  kind: 'image' | 'video';
  /** The video's running time, or null for a photo. */
  durationMs: number | null;
  /** A video the window died opening: the viewer shows `thumbError` and never creates a
   *  `<video>` for it. */
  videoCrashed: boolean;
  /** Ids of the albums the photo is in. */
  albums: number[];
  /** Other files with the same bytes as this one. */
  copies: ItemCopy[];
  /** The picture turned but not cropped: what `image/<id>/uncropped` serves. */
  uncroppedWidth: number;
  uncroppedHeight: number;
  /** What the user has done to the photo in photon; null for an untouched one. For an
   *  edited photo `width`, `height`, `orientation`, `faces` and `unnamedFaces` describe the picture as
   *  shown, because the edit is rendered into every image the backend serves. */
  edit: ItemEdit | null;
  /** Every date the photo has, for the info panel. */
  dates: ItemDates;
  /** Where the photo was taken, in decimal degrees (north and east positive); null when
   *  its EXIF does not say. */
  gps: { lat: number; lon: number } | null;
  /** Pixels at each brightness step, darkest first, of the photo as shown; null for a
   *  video and until the thumbnail it is counted from exists. */
  histogram: number[] | null;
}
/** Two clocks, two units: the camera's dates are its wall clock in naive SECONDS, like
 *  `takenAt`; the file's are real instants in MILLISECONDS. Null where the file has none -
 *  `taken` is null for a photo dated by its mtime, and `fileCreatedMs` wherever the
 *  filesystem keeps no birth time. */
export interface ItemDates {
  /** EXIF DateTimeOriginal; a video's creation date. */
  taken: number | null;
  /** EXIF DateTimeDigitized. */
  digitized: number | null;
  /** EXIF DateTime: when the camera or some software last wrote the file. */
  edited: number | null;
  fileCreatedMs: number | null;
  fileModifiedMs: number;
}
/** `crop` is `[left, top, right, bottom]` in 1/65535ths of the turned picture. */
export interface ItemEdit { turns: number; crop: [number, number, number, number] | null }
export type CopyKind = 'identical' | 'similar';
export interface ItemCopy {
  id: number;
  path: string;
  kind: CopyKind;
  width: number;
  height: number;
}
/** `key` is `p:<id>` for a named person or `c:<hash>` for a Picasa contact no person is linked to. */
export interface Person { key: string; name: string; count: number }
/** What one export came to. Mirrors `ExportReport` in `commands.rs`. `failed` counts a photo
 *  that has gone from the library since the grid was built as well as one that could not be
 *  read; `reason` is the first of those failures, for a message that can say why. */
export interface ExportReport {
  written: number;
  failed: number;
  reason: string | null;
}

/** How far an export has got. Mirrors `ExportProgress` in `events.rs`; `done` counts every
 *  photo finished with, written or not, so `done === total` is always the end. */
export interface ExportProgress {
  done: number;
  total: number;
  failed: number;
}

/** Mirrors `events::FacePhase`: which step of the face pass a progress event counts. */
export type FacePhase = 'detecting' | 'recognising';
/** Mirrors `events::FaceProgress`. Detecting: live images the detector has looked at, of all
 *  live images. Recognising: faces on live images the recogniser has looked at, of all of
 *  them. `running` is false on a pass's last event, which carries the phase of its last step. */
export interface FaceProgress {
  phase: FacePhase;
  checked: number;
  total: number;
  running: boolean;
}

/** A photo a naming skipped. Mirrors `library::SkippedItem`. */
export interface SkippedItem { id: number; fileName: string }
/** One kind of skipped photo: the first twenty and how many there were. Mirrors `library::Skipped`. */
export interface Skipped { items: SkippedItem[]; count: number }
/** What naming photos did. Mirrors `library::NamedItems`; `person` is null when nothing was
 *  named and no person of that name exists, `name` the name as stored. */
export interface NamedItems {
  person: number | null
  name: string
  named: number
  already: Skipped
  several: Skipped
  none: Skipped
}
/** What taking photos from a person did. Mirrors `library::RemovedItems`. */
export interface RemovedItems { removed: number; keptByPicasa: number }

/** One face on the People page. Mirrors `library::PageFace`; `thumbKey` is the photo's
 *  thumbnail key, which with the face id names the face's crop; `personId` is the face's
 *  group, how a single face is named. */
export interface PageFace { id: number; itemId: number; thumbKey: string; confirmed: boolean; personId: number | null }
/** Picasa's name for a group: the name to offer, its contact, and how many of the group's
 *  faces sit on that contact's faces. Mirrors `library::Offer`. */
export interface Offer { name: string; contact: string; faces: number }
/** A group or a person on the People page: the section's count of visible faces and the
 *  first of them. Mirrors `library::PageGroup`. */
export interface PageGroup { id: number; name: string | null; faceCount: number; faces: PageFace[]; offer: Offer | null }
/** Mirrors `library::PeoplePage`. Every named person is in `people`, with `faceCount` 0 and
 *  no faces when none of their confirmed faces is visible. */
export interface PeoplePage {
  /** The largest unnamed groups, at most 200 (`LISTED_GROUPS`); `unnamedCount` counts them all. */
  unnamed: PageGroup[];
  unnamedCount: number;
  singleFaces: PageFace[];
  singleCount: number;
  suggestions: PageGroup[];
  people: PageGroup[];
  ignoredGroups: PageGroup[];
  ignoredFaces: PageFace[];
}
/** Which of a person's faces `personFaces` pages through. Mirrors `library::FaceFilter`. */
export type FaceFilter = 'all' | 'confirmed' | 'unconfirmed';
/** What switching face detection off would delete that the user made. Mirrors
 *  `FaceDataSummary` in `commands.rs`. */
export interface FaceDataSummary { namedPeople: number }

/** What one keyword write to a selection came to. Mirrors `TagWrite` in `commands.rs`.
 *  `count` can be short of the selection: a photo purged or gone missing since the grid was
 *  built is skipped, not refused. */
export interface TagWrite {
  tag: string;
  count: number;
}

/** `count` is the photos carrying the keyword that are not hidden (the sidebar's number, and
 *  0 hides it there); `total` includes hidden photos (the tag manager's number). */
export interface TagCount { tag: string; count: number; total: number }
/** A tag the user renamed (`target` set) or removed (`target` null). photon applies it
 *  when reading tags; the photo files keep their keywords. */
export interface TagRule { tag: string; target: string | null }
export interface Album { id: number; name: string; createdMs: number }
/** `picasa`: mirrored from Picasa's INI by the scan - listed and viewable, never edited. */
export interface AlbumSummary { id: number; name: string; count: number; picasa: boolean }
/** A named query in the sidebar. No count: see `library/searches.rs` for why one would
 *  cost a full library pass per row on every change. */
export interface SavedSearch { id: number; name: string; query: string; createdMs: number }
export interface ScanProgressEvent {
  watchedId: number;
  filesSeen: number;
  added: number;
  changed: number;
  done: boolean;
  cancelled: boolean;
}
export interface FolderStatus { watchedId: number; online: boolean; degraded: boolean }
/** `dataChanged`: the data may have moved since the last event, not only the view, the sort
 *  or the search - what the sidebar's collections are refetched on (`Engine::data_dirty`). */
export interface LibraryChanged { version: number; len: number; dataChanged: boolean }
export interface AppError { kind: string; message: string }
export interface VideoJob { id: number; key: string }
export type VideoFailure = 'unsupported' | 'decode' | 'timeout';

export const api = {
  listFolders: () => invoke<FolderList>('list_folders'),
  addFolder: (path: string) => invoke<WatchedFolder>('add_folder', { path }),
  removeFolder: (watchedId: number) => invoke<void>('remove_folder', { watchedId }),
  rescanFolder: (watchedId: number) => invoke<void>('rescan_folder', { watchedId }),
  watchedFolderStats: () => invoke<WatchedFolderStats[]>('watched_folder_stats'),
  libraryStats: () => invoke<LibraryStats>('library_stats'),
  appInfo: () => invoke<AppInfo>('app_info'),
  memoryUsage: () => invoke<MemoryUsage>('memory_usage'),
  /** Reveals a watched root itself; unlike `revealFolder` it works for a root that has no
   *  folder row yet (offline, or never scanned). */
  revealWatched: (watchedId: number) => invoke<void>('reveal_watched', { watchedId }),
  revealLibrary: () => invoke<void>('reveal_library'),
  gridInfo: (knownLayout: number | null) => invoke<GridInfo>('grid_info', { knownLayout }),
  gridRows: (offset: number, count: number) => invoke<GridRows>('grid_rows', { offset, count }),
  gridOffsetOfFolder: (folderId: number) => invoke<number | null>('grid_offset_of_folder', { folderId }),
  gridFolderIdsAt: (offset: number) => invoke<FolderIds | null>('grid_folder_ids_at', { offset }),
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
  slideshowShuffle: () => invoke<boolean>('slideshow_shuffle'),
  setSlideshowShuffle: (shuffle: boolean) => invoke<void>('set_slideshow_shuffle', { shuffle }),
  similarDistance: () => invoke<number>('similar_distance'),
  /** Resolves to the clamped value the backend stored. */
  setSimilarDistance: (distance: number) => invoke<number>('set_similar_distance', { distance }),
  faceDetection: () => invoke<boolean>('face_detection'),
  setFaceDetection: (enabled: boolean) => invoke<void>('set_face_detection', { enabled }),
  faceDataSummary: () => invoke<FaceDataSummary>('face_data_summary'),
  peopleToName: () => invoke<number>('people_to_name'),
  peoplePage: (strip: number) => invoke<PeoplePage>('people_page', { strip }),
  personFaces: (person: number, which: FaceFilter, offset: number, limit: number) =>
    invoke<PageFace[]>('person_faces', { person, which, offset, limit }),
  /** Resolves to the person the group ended in: another one when the name is taken. */
  namePerson: (person: number, name: string) => invoke<number>('name_person', { person, name }),
  renamePerson: (person: number, name: string) => invoke<number>('rename_person', { person, name }),
  confirmFaces: (faces: number[]) => invoke<void>('confirm_faces', { faces }),
  rejectFaces: (faces: number[]) => invoke<void>('reject_faces', { faces }),
  /** Resolves to the person the faces ended in; null when none of them exists any more. */
  nameFaces: (faces: number[], name: string) => invoke<number | null>('name_faces', { faces, name }),
  nameItems: (items: number[], name: string) => invoke<NamedItems>('name_items', { items, name }),
  removeFromPerson: (person: number, items: number[]) =>
    invoke<RemovedItems>('remove_from_person', { person, items }),
  mergePeople: (from: number, into: number) => invoke<void>('merge_people', { from, into }),
  ignorePerson: (person: number, ignored: boolean) => invoke<void>('ignore_person', { person, ignored }),
  ignoreFaces: (faces: number[], ignored: boolean) => invoke<void>('ignore_faces', { faces, ignored }),
  deletePerson: (person: number) => invoke<void>('delete_person', { person }),
  theme: () => invoke<ThemeChoice>('theme'),
  setTheme: (choice: ThemeChoice) => invoke<void>('set_theme', { choice }),
  gridTile: () => invoke<GridTile>('grid_tile'),
  setGridTile: (tile: GridTile) => invoke<void>('set_grid_tile', { tile }),
  /** The native title bar's scheme; null hands it back to the desktop. Granted in
   *  `capabilities/default.json`. */
  setWindowTheme: (theme: 'light' | 'dark' | null) => getCurrentWindow().setTheme(theme),
  /** The view setters answer with the grid version that shows the state they moved to, or
   *  null when the backend cannot vouch for one; see `LibraryStore.refreshAfter`. */
  setGridView: (view: GridView) => invoke<number | null>('set_grid_view', { view }),
  setSort: (sort: Sort) => invoke<number | null>('set_sort', { sort }),
  setSearchQuery: (query: string) => invoke<number | null>('set_search_query', { query }),
  setPersonView: (person: string) => invoke<number | null>('set_person_view', { person }),
  setAlbumView: (albumId: number) => invoke<number | null>('set_album_view', { albumId }),
  setTagView: (tag: string) => invoke<number | null>('set_tag_view', { tag }),
  copyCount: (id: number) => invoke<number>('copy_count', { id }),
  setCopiesView: (id: number) => invoke<number | null>('set_copies_view', { id }),
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
  /** Adds one keyword to a whole selection. The name that comes back is the one stored,
   *  which a rename rule can make different from what was typed. */
  /** Copies photos into `dest`. A destination inside a watched folder is refused. */
  /** `maxEdge` scales down the photos whose long edge exceeds it; null keeps every size. */
  exportItems: (ids: number[], dest: string, applyEdits: boolean, maxEdge: number | null) =>
    invoke<ExportReport>('export_items', { ids, dest, applyEdits, maxEdge }),
  /** Rejects a destination photon will not write into - today, one inside a watched folder. */
  checkExportDest: (dest: string) => invoke<void>('check_export_dest', { dest }),
  exportApplyEdits: () => invoke<boolean>('export_apply_edits'),
  setExportApplyEdits: (apply: boolean) => invoke<void>('set_export_apply_edits', { apply }),
  addItemsTag: (ids: number[], tag: string) => invoke<TagWrite>('add_items_tag', { ids, tag }),
  removeItemsTag: (ids: number[], tag: string) => invoke<TagWrite>('remove_items_tag', { ids, tag }),
  listAlbums: () => invoke<AlbumSummary[]>('list_albums'),
  createAlbum: (name: string) => invoke<Album>('create_album', { name }),
  renameAlbum: (albumId: number, name: string) => invoke<void>('rename_album', { albumId, name }),
  deleteAlbum: (albumId: number) => invoke<void>('delete_album', { albumId }),
  addToAlbum: (albumId: number, itemIds: number[]) => invoke<void>('add_to_album', { albumId, itemIds }),
  removeFromAlbum: (albumId: number, itemIds: number[]) => invoke<void>('remove_from_album', { albumId, itemIds }),
  listSavedSearches: () => invoke<SavedSearch[]>('list_saved_searches'),
  saveSearch: (name: string, query: string) => invoke<SavedSearch>('save_search', { name, query }),
  renameSavedSearch: (searchId: number, name: string) =>
    invoke<void>('rename_saved_search', { searchId, name }),
  deleteSavedSearch: (searchId: number) => invoke<void>('delete_saved_search', { searchId }),
  setVisible: (ids: number[]) => invoke<void>('set_visible', { ids }),
  viewerItem: (id: number) => invoke<ViewerItem>('viewer_item', { id }),
  /** Sets or clears a star. Written into the folder's Picasa INI first, then mirrored into
   *  the library; the grid rebuilds and `library-changed` follows. */
  setStar: (id: number, starred: boolean) => invoke<void>('set_star', { id, starred }),
  /** Stars or unstars several photos at once, answering how many landed: a folder whose
   *  `.picasa.ini` cannot be written is skipped, and the caller says so. */
  setStars: (ids: number[], starred: boolean) => invoke<number>('set_stars', { ids, starred }),
  /** Hides or unhides photos, returning how many changed. Nothing is written to the files. */
  setItemsHidden: (ids: number[], hidden: boolean) => invoke<number>('set_items_hidden', { ids, hidden }),
  /** Hides or unhides a folder: its photos now, and any added to it later. Not its subfolders. */
  setFolderHidden: (folderId: number, hidden: boolean) => invoke<number>('set_folder_hidden', { folderId, hidden }),
  setFolderAlias: (folderId: number, alias: string | null) => invoke<boolean>('set_folder_alias', { folderId, alias }),
  /** Turns the photo a quarter; the crop goes round with it. Nothing is written to the file. */
  rotateItem: (id: number, clockwise: boolean) => invoke<void>('rotate_item', { id, clockwise }),
  /** Replaces the photo's edit; no turns and no crop is the original again. */
  setItemEdit: (id: number, turns: number, crop: [number, number, number, number] | null) =>
    invoke<void>('set_item_edit', { id, turns, crop }),
  neighbours: (id: number, radius: number) => invoke<number[]>('neighbours', { id, radius }),
  revealInFileManager: (id: number) => invoke<void>('reveal_in_file_manager', { id }),
  /** Opens the file itself in the system's app for it: photon's turns and crop do not go along. */
  openInDefaultApp: (id: number) => invoke<void>('open_in_default_app', { id }),
  /** Opens where the photo was taken on OpenStreetMap, in the system's browser. */
  openInMap: (id: number) => invoke<void>('open_in_map', { id }),
  /** Copies the photo, as shown and capped at 2560 px, to the clipboard as a picture. */
  copyPhoto: (itemId: number) => invoke<void>('copy_photo', { itemId }),
  revealFolder: (folderId: number) => invoke<void>('reveal_folder', { folderId }),
  mediaBase: () => invoke<string>('media_base'),
  videoSessionStart: (supported: boolean) => invoke<void>('video_session_start', { supported }),
  /** Long-polls: resolves with a job, or null after about 25 s with none. */
  nextVideoJob: () => invoke<VideoJob | null>('next_video_job'),
  /** The frame goes as the raw body, not JSON: a JSON number array of a JPEG is ~4x its size. */
  putVideoFrame: (id: number, key: string, jpeg: Uint8Array) =>
    invoke<void>('put_video_frame', jpeg, { headers: { 'x-photon-id': String(id), 'x-photon-key': key } }),
  videoFrameFailed: (id: number, key: string, reason: VideoFailure) =>
    invoke<void>('video_frame_failed', { id, key, reason }),
};

export const events = {
  onLibraryChanged: (cb: (e: LibraryChanged) => void): Promise<UnlistenFn> =>
    listen<LibraryChanged>('library-changed', (e) => cb(e.payload)),
  onScanProgress: (cb: (e: ScanProgressEvent) => void): Promise<UnlistenFn> =>
    listen<ScanProgressEvent>('scan-progress', (e) => cb(e.payload)),
  onFolderStatus: (cb: (e: FolderStatus) => void): Promise<UnlistenFn> =>
    listen<FolderStatus>('folder-status', (e) => cb(e.payload)),
  onExportProgress: (cb: (e: ExportProgress) => void): Promise<UnlistenFn> =>
    listen<ExportProgress>('export-progress', (e) => cb(e.payload)),
  onFaceProgress: (cb: (e: FaceProgress) => void): Promise<UnlistenFn> =>
    listen<FaceProgress>('face-progress', (e) => cb(e.payload)),
  /** Files and folders dragged from another program over photon's window: the system's
   *  own drag, which the webview reports with real paths. `over` is left out - it fires per
   *  pointer move and says nothing `enter` did not. */
  onFileDrag: (cb: (e: FileDrag) => void): Promise<UnlistenFn> =>
    getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === 'enter' || p.type === 'drop') cb({ type: p.type, paths: p.paths });
      else if (p.type === 'leave') cb({ type: 'leave' });
    }),
};

export function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
  return String(e);
}
