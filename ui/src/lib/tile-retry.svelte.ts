/** How long a tile waits before its first retry after a failed thumbnail request - a
 *  thumbnail that was still queued when the tile scrolled out answers 503, and both
 *  attempts can fall in that window, so a short quiet retry clears the common case without
 *  ever showing the broken icon. */
export const TILE_RETRY_MS = 2000;

/** How long the first background retry waits, once a second failure has shown the broken
 *  icon - see `TileStatus` and `TILE_BROKEN_RETRY_ATTEMPTS` for why there's more than one
 *  and why they eventually stop. Doubles on each further attempt up to
 *  `TILE_BROKEN_RETRY_MAX_MS`, the same shape as the backend's own
 *  `SUSPECT_BACKOFF_START`/`SUSPECT_BACKOFF_MAX` (`crates/photon-core/src/thumbs/service.rs`)
 *  - not read from there (there's no shared build step to enforce it), just answering the
 *  same question at the UI's own pace. */
export const TILE_BROKEN_RETRY_START_MS = 5000;

/** The cap the doubling in `TILE_BROKEN_RETRY_START_MS` grows into - matches the backend's
 *  own `SUSPECT_BACKOFF_MAX = 30s`: no reason for a tile to wait longer between retries
 *  than the backend ever will between its own. */
export const TILE_BROKEN_RETRY_MAX_MS = 30000;

/** How many background retries a broken tile makes before giving up on retrying by itself.
 *  Growing 5s, 10s, 20s, 30s, 30s sums to 95s - over three times `SUSPECT_BACKOFF_MAX`
 *  (30s), which is margin enough that a suspect stuck at the backend's own cap for a full
 *  cycle is still caught before the tile stops asking. Past that bound this gives up
 *  rather than retrying forever: `library.pageTick` (`Tile.svelte`'s own effect) is what
 *  brings it back after that, same as before this schedule existed. */
export const TILE_BROKEN_RETRY_ATTEMPTS = 5;

export type TileStatus =
  | 'loading'
  /** A background retry is queued or in flight. The broken icon is shown the whole time a
   *  tile is in this state - see `failed`'s own doc for why it must never flicker back to
   *  `'loading'` between retries. */
  | 'retrying'
  | 'loaded'
  /** Retries exhausted (`TILE_BROKEN_RETRY_ATTEMPTS`) or the request is genuinely done for
   *  (a 422 for a photo the crash-loop guard failed outright, a 404 for one that's gone) -
   *  `failed` cannot tell those apart from a suspect merely still backing off, so it
   *  schedules the same retries for all of them; only reaching the bound produces this
   *  terminal state. A reset (`reset()`, called by `Tile.svelte`'s `pageTick` effect among
   *  others) leaves it, but back at `'loading'`, not at another terminal state. */
  | 'broken';

/** What the tile's warning icon says, or nothing when it shows none. `'retrying'` does not
 *  say the photo can't be shown: that is usually a suspect waiting out the backend's
 *  back-off, and the retries that follow usually load it. It cannot promise that either -
 *  an `<img>` error carries no status, so a photo the crash guard has failed outright
 *  retries the same way - hence "yet". */
export function tileProblem(status: TileStatus): string | undefined {
  if (status === 'retrying') return "Couldn't load this photo yet. Trying again…";
  if (status === 'broken') return "This photo can't be shown";
  return undefined;
}

/** Owns one tile's load/retry state machine, independent of the DOM.
 *
 *  The component pairs this with an `<img>`: `failed()` and `loaded()` are its `onerror`
 *  and `onload`, `attempt` feeds a cache-busting query param so a retry actually reissues
 *  the request, and `status` decides what the tile shows. The `<img>` itself should stay
 *  mounted for every status, only made visible once `status` is `'loaded'` - so a
 *  background retry's fetch can actually run while it's hidden behind the broken icon.
 *  `Tile.svelte` renders it whenever there is a `src`, regardless of `status`, and overlays
 *  the broken icon only while `status` calls for it. */
export function createTileRetry() {
  let status = $state<TileStatus>('loading');
  let attempt = $state(0);
  // Not $state: nothing reads it reactively, it only decides the next scheduled wait.
  let retries = 0;
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

    /** Busts the cache on every retry, so the browser reissues the request instead of
     *  reusing a cached failure. */
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
      retries = 0;
    },

    /** The current request failed. The first failure retries once, quickly, without ever
     *  showing broken - see `TILE_RETRY_MS`. Every failure after that enters (or continues)
     *  `'retrying'`: broken icon up, another background retry scheduled at a growing pace,
     *  up to `TILE_BROKEN_RETRY_ATTEMPTS` of them, after which `status` becomes the
     *  terminal `'broken'` and nothing more is scheduled.
     *
     *  Crucially, a retry that also fails does *not* revert `status` to `'loading'` first -
     *  only the scheduled timer bumps `attempt`, `status` stays exactly `'retrying'` across
     *  every attempt. The previous shape (each retry's timer set `status = 'loading'`
     *  before the next `<img>` load either succeeded or failed) made `Tile.svelte` drop the
     *  broken icon and show a blank, opacity-0 `<img>` for the gap between the timer firing
     *  and the next `onerror` - icon, blank, icon, every retry. */
    failed() {
      if (status === 'loading' && attempt === 0) {
        timer = setTimeout(() => (attempt = 1), TILE_RETRY_MS);
        return;
      }
      if (retries >= TILE_BROKEN_RETRY_ATTEMPTS) {
        status = 'broken';
        return;
      }
      status = 'retrying';
      const wait = Math.min(
        TILE_BROKEN_RETRY_START_MS * 2 ** retries,
        TILE_BROKEN_RETRY_MAX_MS,
      );
      retries += 1;
      timer = setTimeout(() => {
        attempt += 1;
      }, wait);
    },

    /** Drops any pending retry without changing `status` - the tile has unmounted, or
     *  moved on to a different photo before this one settled. */
    cancel() {
      clear();
    },
  };
}

export type TileRetry = ReturnType<typeof createTileRetry>;
