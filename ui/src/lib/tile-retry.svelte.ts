/** How long a tile waits before its first retry after a failed thumbnail request - a
 *  thumbnail that was still queued when the tile scrolled out answers 503, and both
 *  attempts can fall in that window, so a short quiet retry clears the common case without
 *  ever showing the broken icon. */
export const TILE_RETRY_MS = 2000;

/** How long a broken tile waits between retries after its first one has also failed.
 *
 *  A photo whose decode is backing off the crash-loop guard's suspect lock
 *  (`SUSPECT_BACKOFF_START`, doubling up to `SUSPECT_BACKOFF_MAX = 30s`, in
 *  `photon_core::thumbs::service`) can keep answering 503 for tens of seconds after a
 *  tile's one quick retry (`TILE_RETRY_MS`) is already spent. Nothing else tells a broken
 *  tile to try again - `library.pageTick` only fires on an unrelated library change, which
 *  a backing-off decode by itself never causes - so without a retry of its own here the
 *  tile would stay broken until the user happens to cause one (a scan, a star, a scroll
 *  that loads a new page) even though the photo eventually does get its thumbnail.
 *  Retrying at this slower, steady pace instead is what makes the tile eventually show it
 *  without the user doing anything. */
export const TILE_BROKEN_RETRY_MS = 5000;

export type TileStatus = 'loading' | 'loaded' | 'broken';

/** Owns one tile's load/retry state machine, independent of the DOM.
 *
 *  The component pairs this with an `<img>`: `failed()` and `loaded()` are its `onerror`
 *  and `onload`, `attempt` feeds a cache-busting query param so a retry actually reissues
 *  the request, and `status` decides what the tile shows. */
export function createTileRetry() {
  let status = $state<TileStatus>('loading');
  let attempt = $state(0);
  let timer: ReturnType<typeof setTimeout> | undefined;

  function clear() {
    clearTimeout(timer);
    timer = undefined;
  }

  return {
    /** What the tile currently shows. */
    get status() {
      return status;
    },

    /** Bumped on every retry after the first, so the `<img src>` actually changes and the
     *  browser reissues the request rather than reusing a cached failure. */
    get attempt() {
      return attempt;
    },

    /** The image loaded. */
    loaded() {
      status = 'loaded';
    },

    /** A different photo, or a fresh attempt forced from outside (the library moved on
     *  while this tile was broken - see `Tile.svelte`'s `pageTick` effect). Cancels any
     *  retry already scheduled for whatever this tile was showing before. */
    reset() {
      clear();
      status = 'loading';
      attempt = 0;
    },

    /** The current request failed. The first failure retries once, quickly, without ever
     *  showing broken - see `TILE_RETRY_MS`. Every failure after that marks the tile
     *  broken but keeps retrying at `TILE_BROKEN_RETRY_MS` rather than giving up for good -
     *  see that constant's own doc for why. */
    failed() {
      if (attempt === 0) {
        timer = setTimeout(() => (attempt = 1), TILE_RETRY_MS);
        return;
      }
      status = 'broken';
      timer = setTimeout(() => {
        status = 'loading';
        attempt += 1;
      }, TILE_BROKEN_RETRY_MS);
    },

    /** Drops any pending retry without changing `status` - the tile has unmounted, or
     *  moved on to a different photo before this one settled. */
    cancel() {
      clear();
    },
  };
}

export type TileRetry = ReturnType<typeof createTileRetry>;
