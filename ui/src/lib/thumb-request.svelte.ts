import { debounce } from './search';

/** How long a tile must hold the same photo before its thumbnail is requested.
 *
 *  Short enough to be invisible when a scroll stops, long enough that a flick past a
 *  hundred rows asks for none of them. */
export const TILE_SETTLE_MS = 100;

/** Which photo a tile has actually asked the backend for, as distinct from the one it is
 *  currently assigned.
 *
 *  Tiles are keyed by grid offset rather than photo id, so the photo under a live tile can
 *  change without it unmounting: a rebuild during a scan moves other photos onto its offset.
 *  Swapping `<img src>` each time costs a round trip the webview gives the Rust side no way
 *  to cancel: every abandoned request sits in the thumbnail queue until it times out, and a
 *  run of them competes with the tiles that are finally on screen. Only the photo a tile
 *  settles on is requested.
 *
 *  The first photo a tile ever shows is requested immediately. There is no scroll to settle
 *  from at that point, and delaying it would stall the initial paint of the grid.
 *
 *  Unless the caller says `defer`. Rows are keyed by position, so a tile is mounted fresh
 *  for every row a scroll brings in, and its first photo is also the only one it ever shows:
 *  during a flick every tile passed would otherwise ask for its thumbnail on sight - on a
 *  fresh import, a blocking render each. The grid defers only while it scrolls fast; the
 *  initial paint and a slow scroll still ask at once. */
export function createThumbRequest() {
  let requested = $state<string | null>(null);
  const settle = debounce((key: string) => (requested = key), TILE_SETTLE_MS);

  return {
    /** The photo whose thumbnail has been requested, or null before the first one. */
    get requested() {
      return requested;
    },

    /** Assigns `key` to the tile, requesting it once the tile settles on it. `defer` makes
     *  the tile's first photo wait for the settle too. */
    show(key: string, options: { defer?: boolean } = {}) {
      if (requested === key) {
        settle.cancel();
        return;
      }
      if (requested === null && !options.defer) {
        requested = key;
        return;
      }
      settle(key);
    },

    /** Drops a request that hasn't fired yet — the tile has unmounted, or moved on. */
    cancel() {
      settle.cancel();
    },
  };
}

export type ThumbRequest = ReturnType<typeof createThumbRequest>;
