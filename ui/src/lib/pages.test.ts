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
});
