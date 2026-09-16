import { describe, expect, it, vi } from 'vitest';
import { createAlbumMembership } from './album-membership.svelte';

function deferred() {
  let resolve!: () => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('createAlbumMembership', () => {
  it('flips optimistically and calls add or remove for the bound photo', async () => {
    const add = vi.fn(async () => {});
    const remove = vi.fn(async () => {});
    const m = createAlbumMembership({ add, remove });
    m.bind(7, [1]);
    expect(m.has(1)).toBe(true);
    expect(m.has(2)).toBe(false);

    const p = m.toggle(2);
    expect(m.has(2)).toBe(true);
    expect(m.busy(2)).toBe(true);
    await p;
    expect(add).toHaveBeenCalledWith(2, [7]);
    expect(m.busy(2)).toBe(false);

    await m.toggle(1);
    expect(remove).toHaveBeenCalledWith(1, [7]);
    expect(m.has(1)).toBe(false);
  });

  it('reverts on failure only if the same photo is still bound', async () => {
    const gate = deferred();
    const m = createAlbumMembership({ add: () => gate.promise, remove: async () => {} });
    m.bind(7, []);
    const p = m.toggle(3);
    expect(m.has(3)).toBe(true);

    // The user moved on; the failure must not touch the next photo's boxes.
    m.bind(8, [3]);
    gate.reject(new Error('nope'));
    await expect(p).rejects.toThrow('nope');
    expect(m.has(3)).toBe(true);

    const failing = createAlbumMembership({
      add: async () => {
        throw new Error('nope');
      },
      remove: async () => {},
    });
    failing.bind(9, []);
    await expect(failing.toggle(3)).rejects.toThrow('nope');
    expect(failing.has(3)).toBe(false);
    expect(failing.busy(3)).toBe(false);
  });

  it('drops a second click on an album whose call is still in flight', async () => {
    const gate = deferred();
    const add = vi.fn(() => gate.promise);
    const m = createAlbumMembership({ add, remove: async () => {} });
    m.bind(7, []);
    const p = m.toggle(3);
    await m.toggle(3);
    expect(add).toHaveBeenCalledTimes(1);
    expect(m.has(3)).toBe(true);
    gate.resolve();
    await p;
  });

  it('does nothing before a photo is bound', async () => {
    const add = vi.fn(async () => {});
    const m = createAlbumMembership({ add, remove: async () => {} });
    await m.toggle(1);
    expect(add).not.toHaveBeenCalled();
  });
});
