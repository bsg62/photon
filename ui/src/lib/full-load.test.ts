import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createFullLoad, FULL_LOAD_SETTLE_MS } from './full-load';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

/** Records when (and whether) a wait settled, without awaiting it. */
function track(p: Promise<boolean>) {
  const seen: { go: boolean | null } = { go: null };
  void p.then((go) => (seen.go = go));
  return seen;
}

const flush = () => vi.advanceTimersByTimeAsync(0);

describe('createFullLoad', () => {
  it('loads the photo the viewer opens on at once', async () => {
    const load = createFullLoad();

    const first = track(load.wait(5, { slideshow: false }));
    await flush();

    expect(first.go).toBe(true);
  });

  it('waits for the viewer to rest on a photo reached by hand', async () => {
    const load = createFullLoad();
    await load.wait(5, { slideshow: false });

    const next = track(load.wait(6, { slideshow: false }));
    await vi.advanceTimersByTimeAsync(FULL_LOAD_SETTLE_MS - 1);
    expect(next.go).toBeNull();
    await vi.advanceTimersByTimeAsync(1);

    expect(next.go).toBe(true);
  });

  it('requests none of the photos a held arrow key passes, only the last', async () => {
    const load = createFullLoad();
    await load.wait(0, { slideshow: false });

    const passed = [1, 2, 3, 4].map((at) => {
      const seen = track(load.wait(at, { slideshow: false }));
      vi.advanceTimersByTime(40); // a key repeat
      return seen;
    });
    const stopped = track(load.wait(5, { slideshow: false }));
    await vi.advanceTimersByTimeAsync(FULL_LOAD_SETTLE_MS);

    expect(passed.map((s) => s.go)).toEqual([false, false, false, false]);
    expect(stopped.go).toBe(true);
  });

  it('abandons a pending wait when the viewer closes', async () => {
    const load = createFullLoad();
    await load.wait(0, { slideshow: false });
    const next = track(load.wait(1, { slideshow: false }));

    load.cancel();
    await vi.advanceTimersByTimeAsync(FULL_LOAD_SETTLE_MS * 10);

    expect(next.go).toBe(false);
  });

  it('does not delay a slideshow, whose countdown runs from the photo being shown', async () => {
    const load = createFullLoad();
    await load.wait(0, { slideshow: false });

    const next = track(load.wait(1, { slideshow: true }));
    await flush();

    expect(next.go).toBe(true);
  });

  it('reloads the offset already loaded at once: an edit is not a step', async () => {
    const load = createFullLoad();
    await load.wait(3, { slideshow: false });

    const again = track(load.wait(3, { slideshow: false }));
    await flush();

    expect(again.go).toBe(true);
  });

  it('reloads a renumbered photo at once: a rebuild moving it is not a step', async () => {
    const load = createFullLoad();
    await load.wait(3, { slideshow: false });
    load.renumbered(4);

    const again = track(load.wait(4, { slideshow: false }));
    await flush();

    expect(again.go).toBe(true);
  });
});
