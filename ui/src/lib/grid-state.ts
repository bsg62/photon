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

/** Whether the grid should say it is empty ("No photos yet", "No starred photos"...). Not
 *  before the first build: the index is empty then because nothing has been read, and on a
 *  large library that message stood on screen for as long as the read took. */
export function showEmptyNotice(info: Pick<GridInfo, 'version' | 'len'>): boolean {
  return gridBuilt(info) && info.len === 0;
}

/** The status bar's photo count, or null before the first build, when "0 photos" would be
 *  the same misreading as the empty notice. */
export function photoCount(info: Pick<GridInfo, 'version' | 'len'>): string | null {
  return gridBuilt(info) ? `${info.len.toLocaleString()} photos` : null;
}
