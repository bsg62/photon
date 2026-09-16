import { describe, expect, it, vi } from 'vitest';
import { createStarToggle } from './star-toggle.svelte';

function deferred() {
  let resolve!: () => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('createStarToggle', () => {
  it('takes the bound photo’s current state', () => {
    const star = createStarToggle(() => Promise.resolve());
    star.bind(7, true);
    expect(star.starred).toBe(true);
    star.bind(8, false);
    expect(star.starred).toBe(false);
  });

  it('flips immediately and sends the new value for the bound photo', async () => {
    const call = deferred();
    const setStar = vi.fn(() => call.promise);
    const star = createStarToggle(setStar);
    star.bind(7, false);

    const done = star.toggle();

    expect(star.starred).toBe(true);
    expect(star.busy).toBe(true);
    expect(setStar).toHaveBeenCalledExactlyOnceWith(7, true);
    call.resolve();
    await done;
    expect(star.starred).toBe(true);
    expect(star.busy).toBe(false);
  });

  it('drops a second toggle while one is in flight', async () => {
    const call = deferred();
    const setStar = vi.fn(() => call.promise);
    const star = createStarToggle(setStar);
    star.bind(7, false);

    const first = star.toggle();
    await star.toggle();

    expect(setStar).toHaveBeenCalledTimes(1);
    expect(star.starred).toBe(true);
    call.resolve();
    await first;
  });

  it('reverts and rethrows when the write fails', async () => {
    const call = deferred();
    const star = createStarToggle(() => call.promise);
    star.bind(7, false);

    const done = star.toggle();
    call.reject(new Error('read-only'));

    await expect(done).rejects.toThrow('read-only');
    expect(star.starred).toBe(false);
    expect(star.busy).toBe(false);
  });

  it('does not revert onto a photo bound after the toggle was sent', async () => {
    // ArrowRight during the write: the revert belongs to the old photo, which is no longer
    // on screen, and must not flip the star of the one that is.
    const call = deferred();
    const star = createStarToggle(() => call.promise);
    star.bind(7, false);

    const done = star.toggle();
    star.bind(8, true);
    call.reject(new Error('read-only'));

    await expect(done).rejects.toThrow();
    expect(star.starred).toBe(true);
  });

  it('does nothing before a photo is bound', async () => {
    const setStar = vi.fn(() => Promise.resolve());
    const star = createStarToggle(setStar);
    await star.toggle();
    expect(setStar).not.toHaveBeenCalled();
  });
});
