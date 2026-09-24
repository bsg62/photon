import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TILE_BROKEN_RETRY_MS, TILE_RETRY_MS, createTileRetry } from './tile-retry.svelte';

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

  it('retries once quickly on the first failure, without showing broken', () => {
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

  // This is the behaviour a backing-off suspect (`SUSPECT_BACKOFF_START`, up to
  // `SUSPECT_BACKOFF_MAX = 30s`) needs: a second failure must not be the end of retrying.
  // Before `TileRetry` existed, `Tile.svelte`'s own `onerror` set `status = 'broken'` here
  // and never scheduled another attempt - the tile only ever recovered if `library.pageTick`
  // happened to fire for an unrelated reason. Probe: comment out the `timer = setTimeout(...)`
  // call in `failed`'s broken branch (restoring that old behaviour) and this goes RED - status
  // stays `'broken'` forever instead of returning to `'loading'`.
  it('shows broken after a second failure but keeps retrying on its own', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);

    r.failed();
    expect(r.status).toBe('broken');

    vi.advanceTimersByTime(TILE_BROKEN_RETRY_MS);
    expect(r.status).toBe('loading');
    // A new attempt number, so the `<img src>` actually changes and the browser reissues
    // the request instead of reusing a cached failure.
    expect(r.attempt).toBe(2);
  });

  it('keeps retrying at the slower pace indefinitely for a suspect backing off well past two failures', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);

    for (let i = 0; i < 5; i++) {
      r.failed();
      expect(r.status).toBe('broken');
      vi.advanceTimersByTime(TILE_BROKEN_RETRY_MS);
      expect(r.status).toBe('loading');
    }

    // The suspect's decode finally won the lock.
    r.loaded();
    expect(r.status).toBe('loaded');
  });

  it('reset cancels a pending retry and starts over at attempt 0', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);
    r.failed();
    expect(r.status).toBe('broken');

    r.reset();

    expect(r.status).toBe('loading');
    expect(r.attempt).toBe(0);
    vi.advanceTimersByTime(TILE_BROKEN_RETRY_MS * 10);
    expect(r.status).toBe('loading'); // no leftover timer fires
  });

  it('cancel drops a pending retry without changing status', () => {
    const r = createTileRetry();
    r.failed();
    vi.advanceTimersByTime(TILE_RETRY_MS);
    r.failed();
    expect(r.status).toBe('broken');

    r.cancel();

    vi.advanceTimersByTime(TILE_BROKEN_RETRY_MS * 3);
    expect(r.status).toBe('broken');
  });
});
