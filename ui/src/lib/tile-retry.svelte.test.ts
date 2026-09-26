import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  TILE_BROKEN_RETRY_ATTEMPTS,
  TILE_BROKEN_RETRY_MAX_MS,
  TILE_BROKEN_RETRY_START_MS,
  TILE_RETRY_MS,
  createTileRetry,
  tileProblem,
} from './tile-retry.svelte';

/** The same growth formula `failed` uses internally, so tests can advance exactly one
 *  scheduled wait at a time without hardcoding the sequence twice. */
function brokenRetryWait(n: number): number {
  return Math.min(TILE_BROKEN_RETRY_START_MS * 2 ** n, TILE_BROKEN_RETRY_MAX_MS);
}

describe('createTileRetry', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('starts loading and reports loaded once the image loads', () => {
    const r = createTileRetry();
    expect(r.status).toBe('loading');

    r.loaded();

    expect(r.status).toBe('loaded');
  });

  it('retries once quickly on the first failure, without entering retrying', () => {
    const r = createTileRetry();

    r.failed();
    expect(r.status).toBe('loading');
    expect(r.attempt).toBe(0);

    vi.advanceTimersByTime(TILE_RETRY_MS);
    expect(r.status).toBe('loading');
    expect(r.attempt).toBe(1);
  });

  // A thumbnail still queued when the tile scrolled out answers 503 for both the initial
  // request and the quick retry - the common case the quick retry alone was built for.
  it('loads on the quick retry if the photo is ready by then', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);

    r.loaded();

    expect(r.status).toBe('loaded');
  });

  // The core fix: before this, each background retry's timer set `status` back to
  // `'loading'`, which made `Tile.svelte` drop the broken icon and show a blank,
  // opacity-0 `<img>` until the next `onerror` brought the icon back - a flicker on every
  // retry. Probe: in `failed`'s scheduled timer callback, set `status = 'loading'` before
  // bumping `attempt` (the old shape) - this goes RED, `status` reads `'loading'`
  // immediately after the timer fires instead of staying `'retrying'`.
  it('keeps the broken icon up across a background retry - status never flickers back to loading', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);

    r.failed(); // second failure: enters retrying
    expect(r.status).toBe('retrying');

    vi.advanceTimersByTime(brokenRetryWait(0));
    expect(r.status).toBe('retrying');
    expect(r.attempt).toBe(2); // a new attempt number, so the <img> actually refetches

    // That retry fails too - still no flicker.
    r.failed();
    expect(r.status).toBe('retrying');
    vi.advanceTimersByTime(brokenRetryWait(1));
    expect(r.status).toBe('retrying');
    expect(r.attempt).toBe(3);
  });

  // The other tests above advance by `brokenRetryWait(n)`, which computes the same doubling
  // formula `failed` itself uses - so if that formula regressed to a constant pace, this
  // helper would regress to compute the same (wrong) constant, and every test that only
  // checks state *after* advancing by "however long the schedule says" would still pass:
  // advancing the fake clock by N also fires any timer scheduled for less than N, so a
  // constant, shorter-than-expected wait still elapses within a longer computed advance.
  // This test hardcodes the real schedule instead, and checks each boundary from both
  // sides, so a wrong pace - faster or slower - is caught rather than silently absorbed.
  //
  // Margin matters here: `TILE_BROKEN_RETRY_ATTEMPTS` retries growing 5s/10s/20s/30s/30s
  // sum to 95s, which is what the doc on `TILE_BROKEN_RETRY_ATTEMPTS` claims outlasts
  // `SUSPECT_BACKOFF_MAX` (30s) with margin; a regression to a constant 5s pace only sums
  // to 25s, under the 31s that bound needs to clear - exactly what this test exists to
  // catch. Probe: replace `TILE_BROKEN_RETRY_START_MS * 2 ** retries` in `failed` with a
  // constant `TILE_BROKEN_RETRY_START_MS` - RED, at the second step's "not yet" assertion
  // (the `schedule[1] = 10000` boundary): a constant 5000ms relative delay and the real
  // 10000ms one first diverge there, so `attempt` has already bumped 1ms before that mark,
  // and `expect` throws before the third step is ever reached.
  it('grows the retry pace exactly 5s, 10s, 20s, 30s, 30s - never earlier, never later', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);
    r.failed(); // enters retrying; the first background retry (5s) is now scheduled

    const schedule = [5000, 10000, 20000, 30000, 30000];
    expect(schedule.length).toBe(TILE_BROKEN_RETRY_ATTEMPTS);

    for (const wait of schedule) {
      const before = r.attempt;
      vi.advanceTimersByTime(wait - 1);
      expect(r.attempt).toBe(before); // not yet
      vi.advanceTimersByTime(1);
      expect(r.attempt).toBe(before + 1); // now
      r.failed(); // that retry failed too - schedules the next one, or gives up on the last
    }

    expect(r.status).toBe('broken'); // the budget (TILE_BROKEN_RETRY_ATTEMPTS) is spent
  });

  it('shows the photo if a later background retry succeeds', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);
    r.failed();
    vi.advanceTimersByTime(brokenRetryWait(0));
    expect(r.status).toBe('retrying');

    r.loaded();

    expect(r.status).toBe('loaded');
  });

  // Probe: raise the `retries >= TILE_BROKEN_RETRY_ATTEMPTS` bound (or remove it) and this
  // goes RED - `status` never reaches `'broken'`, or reaches it later than the assertions
  // below expect, within the fixed number of failures this test drives.
  it('gives up after TILE_BROKEN_RETRY_ATTEMPTS retries and stops scheduling', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);
    r.failed(); // second failure: first background retry scheduled (retries 0 -> 1)

    for (let n = 0; n < TILE_BROKEN_RETRY_ATTEMPTS; n++) {
      expect(r.status).toBe('retrying');
      vi.advanceTimersByTime(brokenRetryWait(n));
      r.failed();
    }

    // The loop above drives exactly TILE_BROKEN_RETRY_ATTEMPTS scheduled retries; its last
    // `r.failed()` call is the one that finds the budget spent and gives up for good.
    expect(r.status).toBe('broken');

    const attemptAtGiveUp = r.attempt;
    vi.advanceTimersByTime(TILE_BROKEN_RETRY_MAX_MS * 5);
    expect(r.status).toBe('broken');
    expect(r.attempt).toBe(attemptAtGiveUp); // nothing left scheduled
  });

  it('reset cancels a pending retry outright, so no leftover timer fires', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);
    r.failed(); // now retrying, with a background timer scheduled
    expect(r.status).toBe('retrying');

    r.reset();
    expect(r.status).toBe('loading');
    expect(r.attempt).toBe(0);

    // `status` alone can't tell a cancelled timer apart from one that fired and then
    // set `status` back to `'loading'` itself - `reset` already forces that. `attempt` is
    // only ever touched by the scheduled timer, so a leftover one (were `clear()` missing
    // from `reset`) would show up here instead. Probe: remove `clear()` from `reset` - RED,
    // `attempt` reads 1, not 0, once the stale timer's wait has passed.
    vi.advanceTimersByTime(TILE_BROKEN_RETRY_MAX_MS * 10);
    expect(r.attempt).toBe(0);
  });

  it('cancel drops a pending retry without changing status', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);
    r.failed();
    expect(r.status).toBe('retrying');

    r.cancel();

    vi.advanceTimersByTime(TILE_BROKEN_RETRY_MAX_MS * 3);
    expect(r.status).toBe('retrying'); // no more retries fire, but the icon stays as is
  });
});

describe('tileProblem', () => {
  // A suspect backing off in the backend is `'retrying'` for up to 95 seconds; telling the
  // user it can't be shown then is wrong more often than not, since most of those load.
  it('says a retrying tile is still being tried, and only a broken one cannot be shown', () => {
    expect(tileProblem('retrying')).toBe("Couldn't load this photo yet. Trying again…");
    expect(tileProblem('broken')).toBe("This photo can't be shown");
  });

  it('has nothing to say while loading or once loaded', () => {
    expect(tileProblem('loading')).toBeUndefined();
    expect(tileProblem('loaded')).toBeUndefined();
  });

  it('says video for a video tile, not photo', () => {
    expect(tileProblem('retrying', 'video')).toBe("Couldn't load this video yet. Trying again…");
    expect(tileProblem('broken', 'video')).toBe("This video can't be shown");
  });
});
