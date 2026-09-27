import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createThumbRequest, TILE_SETTLE_MS } from './thumb-request.svelte';

describe('createThumbRequest', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('requests the first photo a tile shows without waiting', () => {
    const req = createThumbRequest();

    req.show('7/abc');

    expect(req.requested).toBe('7/abc');
  });

  it('defers the first photo when asked, until the tile holds it', () => {
    // A tile mounted mid-flick: it is on screen for a frame or two, and asking for its
    // thumbnail then is a render nobody sees.
    const req = createThumbRequest();

    req.show('1/a', { defer: true });
    vi.advanceTimersByTime(TILE_SETTLE_MS - 1);
    expect(req.requested).toBeNull();

    vi.advanceTimersByTime(1);
    expect(req.requested).toBe('1/a');
  });

  it('asks for nothing when a deferred tile goes away before it settles', () => {
    const req = createThumbRequest();

    req.show('1/a', { defer: true });
    req.cancel();
    vi.advanceTimersByTime(TILE_SETTLE_MS * 10);

    expect(req.requested).toBeNull();
  });

  it('requests only the photo a reused tile settles on', () => {
    // Tiles are keyed by grid offset, so a fast scroll changes the photo under a live tile
    // every frame. Each `src` swap is a round trip the webview gives us no way to cancel,
    // and the Rust queue holds every abandoned one until it times out.
    const req = createThumbRequest();
    req.show('1/a');
    const seen: (string | null)[] = [];

    for (const key of ['2/b', '3/c', '4/d']) {
      req.show(key);
      seen.push(req.requested);
    }
    vi.advanceTimersByTime(TILE_SETTLE_MS);

    expect(seen).toEqual(['1/a', '1/a', '1/a']);
    expect(req.requested).toBe('4/d');
  });

  it('drops a pending request when the tile goes away', () => {
    const req = createThumbRequest();
    req.show('1/a');
    req.show('2/b');

    req.cancel();
    vi.advanceTimersByTime(TILE_SETTLE_MS * 10);

    expect(req.requested).toBe('1/a');
  });

  it('ignores a repeat of the photo already requested', () => {
    const req = createThumbRequest();
    req.show('1/a');
    req.show('2/b');
    vi.advanceTimersByTime(TILE_SETTLE_MS);

    req.show('2/b');
    vi.advanceTimersByTime(TILE_SETTLE_MS);

    expect(req.requested).toBe('2/b');
  });
});
