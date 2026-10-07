/** Whether the grid on hand is one the backend has built, and what may be said about it.
 *  Pure, so the rule is pinned by a test rather than left to the components' markup. */

import type { GridInfo, WatchedFolder } from './api';

/** The version the engine reports before its first grid is built (`engine::NOT_BUILT`).
 *  `Engine::open` leaves that build to the startup thread so the window can draw at once,
 *  and holds an empty index at this version until it lands; every publish adds one, so no
 *  built grid is ever at it. The store's own placeholder, before its first fetch, is below
 *  it and reads as not built too. */
export const NOT_BUILT = 0;

export function gridBuilt(info: Pick<GridInfo, 'version'>): boolean {
  return info.version > NOT_BUILT;
}

/** What the grid says in place of photos when the backend could not read the library at
 *  startup, or null when it could. The backend then publishes an empty grid so the window
 *  leaves the not-built state, and says why beside it (`GridInfo.buildError`). */
export function buildFailure(info: Pick<GridInfo, 'buildError'>): string | null {
  return info.buildError === null ? null : `photon could not read the library: ${info.buildError}`;
}

/** Whether the grid should say it is empty ("No photos yet", "No starred photos"...). Not
 *  before the first build: the index is empty then because nothing has been read, and on a
 *  large library that message stood on screen for as long as the read took. Nor when the
 *  first build failed: that grid is empty because nothing *could* be read, and "No photos
 *  yet. Add a folder" sent the user off to add a folder they already had. */
export function showEmptyNotice(info: Pick<GridInfo, 'version' | 'len' | 'buildError'>): boolean {
  return gridBuilt(info) && info.len === 0 && buildFailure(info) === null;
}

/** The status bar's photo count, or null before the first build, when "0 photos" would be
 *  the same misreading as the empty notice - and after a failed one, for the same reason. */
export function photoCount(info: Pick<GridInfo, 'version' | 'len' | 'buildError'>): string | null {
  return gridBuilt(info) && buildFailure(info) === null ? `${info.len.toLocaleString()} photos` : null;
}

/** What the library says in place of photos while it shows none: an offer to add the first
 *  folder, that a scan is looking, that the watched folders have given no photos, or that
 *  every photo there is has been hidden. */
export type EmptyLibrary = 'first-run' | 'scanning' | 'no-photos' | 'all-hidden';

/** Which of them applies, or null when nothing can be said yet.
 *
 *  Hidden photos first: with any, the library is not empty, and "photon has found no
 *  photos" beside a sidebar row reading "Hidden 240" is false - Hide folder on the only
 *  folder is all it takes.
 *
 *  Then a running scan, whatever the folder list holds: on a first run the backend watches
 *  the Pictures folder by itself and scans it, and the list the UI read at launch may be
 *  from before that. The old notice said "Add a folder to get started" all through that
 *  scan, and of any folder added a second ago.
 *
 *  Otherwise nothing while the watched folders have not been read (`known`): the grid and
 *  the list are fetched side by side, and until the list lands "none is watched" is not
 *  something the UI knows - said anyway, a user with folders was told to add one for as
 *  long as the list took.
 *
 *  `scanning` is what the UI has been told. A full scan says so as it starts, and the scans
 *  already running at launch are asked about (`library.readScanning`), so a first scan is
 *  "looking" from its first moment. It is still not the whole truth: the watcher's scan of
 *  one changed directory says nothing until its end, and a folder photon may not read is
 *  scanned and reported exactly like an empty one. So `'no-photos'` is never worded as a
 *  finished search (`noPhotosLine`). */
export function emptyLibrary(known: boolean, watched: number, scanning: boolean, hidden: number): EmptyLibrary | null {
  if (hidden > 0) return 'all-hidden';
  if (scanning) return 'scanning';
  if (!known) return null;
  return watched === 0 ? 'first-run' : 'no-photos';
}

/** What is said of watched folders that have given no photos (`'no-photos'`).
 *
 *  "Has found none", never "looked and found none": a folder photon is not allowed to read
 *  is scanned and reported by the backend exactly as an empty one is, and a directory the
 *  watcher is rescanning says nothing until it is done (`emptyLibrary`). A folder on a drive that
 *  is not connected is said to be out of reach rather than empty: it may hold every photo
 *  the user has. */
export function noPhotosLine(watched: Pick<WatchedFolder, 'path' | 'online'>[]): string {
  const away = watched.filter((w) => !w.online).length;
  const all = watched.length;
  if (away === 0) {
    return all === 1
      ? `photon watches ${watched[0].path} and has found no photos or videos there.`
      : `photon watches ${all} folders and has found no photos or videos in them.`;
  }
  if (away === all) {
    return all === 1 ? `photon cannot reach ${watched[0].path} right now.` : `photon cannot reach the ${all} folders it watches right now.`;
  }
  return `photon has found no photos or videos in the folders it can reach; ${away} of the ${all} it watches cannot be reached right now.`;
}
