import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createVideoThumbnailer, JOB_TIMEOUT_MS, IDLE_MS } from './video-thumbnailer.svelte';
import { MediaDecodeError, MediaUnsupported } from './video';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());
const settle = () => vi.advanceTimersByTimeAsync(0);

function setup(jobs: ({ id: number; key: string } | null)[], grab: (url: string, signal: AbortSignal) => Promise<Uint8Array>) {
  const queue = [...jobs];
  const deps = {
    nextJob: vi.fn(async () => (queue.length ? queue.shift()! : new Promise<never>(() => {}))),
    put: vi.fn(async () => {}),
    fail: vi.fn(async () => {}),
    url: (id: number) => `u/${id}`,
    grab: vi.fn(grab),
  };
  return { t: createVideoThumbnailer(deps), deps };
}

describe('createVideoThumbnailer', () => {
  it('draws each job and hands the frame back under its key', async () => {
    const { t, deps } = setup([{ id: 1, key: 'k1' }, { id: 2, key: 'k2' }], async () => new Uint8Array([1]));
    t.start();
    await settle();
    expect(deps.put.mock.calls).toEqual([[1, 'k1', new Uint8Array([1])], [2, 'k2', new Uint8Array([1])]]);
    expect(deps.grab.mock.calls.map((c) => c[0])).toEqual(['u/1', 'u/2']);
  });

  it('reports unsupported, decode and timeout as three different reasons', async () => {
    const outcomes = [new MediaUnsupported(), new MediaDecodeError('x'), null];
    const { t, deps } = setup(
      [{ id: 1, key: 'a' }, { id: 2, key: 'b' }, { id: 3, key: 'c' }],
      (_url, signal) => {
        const o = outcomes.shift();
        if (o) return Promise.reject(o);
        return new Promise((_, reject) => signal.addEventListener('abort', () => reject(signal.reason)));
      },
    );
    t.start();
    await settle();
    await vi.advanceTimersByTimeAsync(JOB_TIMEOUT_MS);
    expect(deps.fail.mock.calls).toEqual([[1, 'a', 'unsupported'], [2, 'b', 'decode'], [3, 'c', 'timeout']]);
  });

  it('does not ask again at once after an empty answer', async () => {
    const { t, deps } = setup([null, null], async () => new Uint8Array());
    t.start();
    await settle();
    expect(deps.nextJob).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(deps.nextJob).toHaveBeenCalledTimes(2);
  });

  it('stops taking jobs when stopped', async () => {
    const { t, deps } = setup([null, { id: 1, key: 'a' }], async () => new Uint8Array());
    t.start();
    await settle();
    t.stop();
    await vi.advanceTimersByTimeAsync(IDLE_MS * 3);
    expect(deps.grab).not.toHaveBeenCalled();
  });

  // The test above stops only once `nextJob` has already resolved, so it never exercises
  // the guard that discards a job answered *after* `stop()` was called - `nextJob`'s own
  // promise is in flight (a long-poll can take up to 25s) and only settles once the loop's
  // generation has already moved on. Left out, that gap passed every existing test while
  // still drawing a job for a thumbnailer that had been told to stop.
  it('ignores a job that arrives after stop, while nextJob was still in flight', async () => {
    let resolveJob!: (job: { id: number; key: string } | null) => void;
    const deps = {
      nextJob: vi.fn(() => new Promise<{ id: number; key: string } | null>((r) => (resolveJob = r))),
      put: vi.fn(async () => {}),
      fail: vi.fn(async () => {}),
      url: (id: number) => `u/${id}`,
      grab: vi.fn(async () => new Uint8Array()),
    };
    const t = createVideoThumbnailer(deps);
    t.start();
    await settle();
    t.stop();
    resolveJob({ id: 1, key: 'a' });
    await settle();
    expect(deps.grab).not.toHaveBeenCalled();
  });

  // Fix round 1: stop() is a disposal, not a pause. A start() that lands after dispose
  // (App.svelte's async setup awaits mediaBase()/videoSessionStart() before calling
  // start(), and onMount's cleanup can run stop() first) must never spin up a loop nothing
  // is left to stop.
  it('start() after stop() never calls nextJob', async () => {
    const { t, deps } = setup([{ id: 1, key: 'a' }], async () => new Uint8Array());
    t.stop();
    t.start();
    await settle();
    expect(deps.nextJob).not.toHaveBeenCalled();
  });

  // Fix round 1: stop() must not leave a job's <video> loading and its 15s timer armed
  // after the page that wanted the frame is gone. It aborts the in-flight grab; the
  // reason reported is 'unsupported' (not 'timeout'), since the file is fine and a new
  // session will simply retry it - a real timeout must still read 'timeout'.
  it('stop() during a pending grab aborts its signal and reports unsupported, not timeout, with no timer left afterwards', async () => {
    let capturedSignal: AbortSignal | undefined;
    const deps = {
      nextJob: vi.fn(async () => ({ id: 1, key: 'a' })),
      put: vi.fn(async () => {}),
      fail: vi.fn(async () => {}),
      url: (id: number) => `u/${id}`,
      grab: vi.fn((_url: string, signal: AbortSignal) => {
        capturedSignal = signal;
        return new Promise<Uint8Array>((_, reject) => signal.addEventListener('abort', () => reject(signal.reason)));
      }),
    };
    const t = createVideoThumbnailer(deps);
    t.start();
    await settle();
    expect(vi.getTimerCount()).toBeGreaterThan(0);
    t.stop();
    await settle();
    expect(capturedSignal?.aborted).toBe(true);
    expect(deps.fail.mock.calls).toEqual([[1, 'a', 'unsupported']]);
    expect(vi.getTimerCount()).toBe(0);
  });
});
