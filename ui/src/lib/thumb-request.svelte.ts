import { debounce } from './search';

/** How long a tile must hold the same photo before its thumbnail is requested.
 *
 *  Short enough to be invisible when a scroll stops, long enough that a flick past a
 *  hundred rows asks for none of them. */
export const TILE_SETTLE_MS = 100;

/** Which photo a tile has actually asked the backend for, as distinct from the one it is
 *  currently assigned.
 *
 *  Tiles are keyed by grid offset rather than photo id, so a fast scroll changes the photo
 *  under a live tile every frame. Swapping `<img src>` each time costs a round trip the
 *  webview gives the Rust side no way to cancel: every abandoned request sits in the
 *  thumbnail queue until it times out, so a flick can leave hundreds of them competing with
 *  the tiles that are finally on screen. Only the photo a tile settles on is requested.
 *
 *  The first photo a tile ever shows is requested immediately. There is no scroll to settle
 *  from at that point, and delaying it would stall the initial paint of the grid. */
export function createThumbRequest() {
  let requested = $state<string | null>(null);
  const settle = debounce((key: string) => (requested = key), TILE_SETTLE_MS);

  return {
    /** The photo whose thumbnail has been requested, or null before the first one. */
    get requested() {
      return requested;
    },

    /** Assigns `key` to the tile, requesting it once the tile settles on it. */
    show(key: string) {
      if (requested === key) {
        settle.cancel();
        return;
      }
      if (requested === null) {
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
