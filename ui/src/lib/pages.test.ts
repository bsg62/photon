import { describe, expect, it, vi } from 'vitest';
import { PAGE_SIZE, PageCache } from './pages';

function loader(version: number) {
  return vi.fn(async (offset: number, count: number) => ({
    version,
    rows: Array.from({ length: count }, (_, i) => offset + i),
  }));
}

describe('PageCache', () => {
  it('loads the pages covering a range once', async () => {
    const load = loader(1);
    const cache = new PageCache<number>(load);
    cache.reset(1);
    expect(await cache.ensure(150, 250)).toBe(true);
    expect(load).toHaveBeenCalledTimes(2);
    expect(cache.get(0)).toBe(0);
    expect(cache.get(PAGE_SIZE + 5)).toBe(PAGE_SIZE + 5);
    expect(await cache.ensure(0, 10)).toBe(false);
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('does not request a page twice while it is loading', async () => {
    const load = loader(1);
    const cache = new PageCache<number>(load);
    cache.reset(1);
    await Promise.all([cache.ensure(0, 10), cache.ensure(5, 20)]);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('drops pages from other versions', async () => {
    const onStale = vi.fn();
    const cache = new PageCache<number>(loader(2), onStale);
    cache.reset(1);
    expect(await cache.ensure(0, 10)).toBe(false);
    expect(cache.get(0)).toBeUndefined();
    expect(onStale).toHaveBeenCalledWith(2);
  });

  it('clears on reset and can retry failed loads', async () => {
    let fail = true;
    const cache = new PageCache<number>(async (offset, count) => {
      if (fail) throw new Error('boom');
      return { version: 1, rows: Array.from({ length: count }, (_, i) => offset + i) };
    });
    cache.reset(1);
    expect(await cache.ensure(0, 1)).toBe(false);
    fail = false;
    expect(await cache.ensure(0, 1)).toBe(true);
    cache.reset(2);
    expect(cache.get(0)).toBeUndefined();
  });

  it('prefetches a version without disturbing the current one, and installs it on reset', async () => {
    let version = 1;
    const onStale = vi.fn();
    const cache = new PageCache<number>(async (offset, count) => ({
      version,
      rows: Array.from({ length: count }, (_, i) => version * 1000 + offset + i),
    }), onStale);
    cache.reset(1);
    await cache.ensure(0, 10);

    version = 2;
    const seed = await cache.prefetch(2, 0, 10);
    expect(cache.get(0)).toBe(1000);
    cache.reset(2, seed);
    expect(cache.version).toBe(2);
    expect(cache.get(0)).toBe(2000);

    // A page answered by a still newer index is not installed as this one.
    version = 3;
    expect((await cache.prefetch(2, 0, 10)).size).toBe(0);
    expect(onStale).toHaveBeenCalledWith(3);
  });
});
