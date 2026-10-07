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

/** What the library says in place of photos while it holds none: an offer to add the first
 *  folder, that a scan is still looking, or that the watched folders have no photos. */
export type EmptyLibrary = 'first-run' | 'scanning' | 'no-photos';

/** Which of them applies, or null when nothing can be said yet.
 *
 *  A running scan comes first, whatever the folder list holds: on a first run the backend
 *  watches the Pictures folder by itself and scans it, and the list the UI read at launch
 *  may be from before that. The old notice said "Add a folder to get started" all through
 *  that scan, and of any folder added a second ago.
 *
 *  Otherwise nothing while the watched folders have not been read (`known`): the grid and
 *  the list are fetched side by side, and until the list lands "none is watched" is not
 *  something the UI knows - said anyway, a user with folders was told to add one for as
 *  long as the list took. */
export function emptyLibrary(known: boolean, watched: number, scanning: boolean): EmptyLibrary | null {
  if (scanning) return 'scanning';
  if (!known) return null;
  return watched === 0 ? 'first-run' : 'no-photos';
}

/** What is said of watched folders that gave no photos (`'no-photos'`). A folder on a drive
 *  that is not connected has not been looked in, and is said to be out of reach rather than
 *  empty: it may hold every photo the user has. */
export function noPhotosLine(watched: Pick<WatchedFolder, 'path' | 'online'>[]): string {
  const away = watched.filter((w) => !w.online).length;
  const all = watched.length;
  if (away === 0) {
    return all === 1
      ? `photon looked in ${watched[0].path} and found no photos or videos.`
      : `photon looked in the ${all} folders it watches and found no photos or videos.`;
  }
  if (away === all) {
    return all === 1 ? `photon cannot reach ${watched[0].path} right now.` : `photon cannot reach the ${all} folders it watches right now.`;
  }
  return `photon found no photos or videos in the folders it can reach; ${away} of the ${all} it watches cannot be reached right now.`;
}
