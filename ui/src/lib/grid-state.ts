/** Whether the grid on hand is one the backend has built, and what may be said about it.
 *  Pure, so the rule is pinned by a test rather than left to the components' markup. */

import type { GridInfo } from './api';

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
