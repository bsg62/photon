import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createScrollSpeed, SCROLL_SETTLE_MS } from './scroll-speed.svelte';

const VIEWPORT = 800;

describe('createScrollSpeed', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('measures speed by time and not by event', () => {
    // The same distance per event at 60Hz and at 240Hz: four times the speed.
    const slow = createScrollSpeed();
    const quick = createScrollSpeed();
    for (let i = 0; i < 5; i++) {
      slow.sample(i * 40, 1000 + i * 16, VIEWPORT);
      quick.sample(i * 40, 1000 + i * 4, VIEWPORT);
    }
    expect(slow.motion).toEqual({ kind: 'scroll', direction: 1, speed: 2.5, peak: 2.5 });
    expect(quick.motion).toEqual({ kind: 'scroll', direction: 1, speed: 10, peak: 10 });
  });

  it('calls a flick a scroll however fast it goes, while each frame overlaps the last', () => {
    // 40px/ms at 60Hz is 640px a frame: fast, but continuous - the rows the next frame
    // shows are partly the ones this frame showed, so a lead is still worth mounting.
    const speed = createScrollSpeed();
    for (let i = 0; i < 5; i++) speed.sample(10_000 - i * 640, 1000 + i * 16, VIEWPORT);
    expect(speed.motion).toEqual({ kind: 'scroll', direction: -1, speed: 40, peak: 40 });
  });

  it('calls a move of a viewport or more in one event a jump', () => {
    // A scrollbar drag: each event lands where nothing on screen was on screen before.
    const speed = createScrollSpeed();
    speed.sample(100, 1000, VIEWPORT);
    speed.sample(100 + VIEWPORT, 1016, VIEWPORT);
    expect(speed.motion).toEqual({ kind: 'jump', stream: true });
  });

  it('calls a jump after a pause a jump on its own', () => {
    // End, a folder click: nothing recent, so no drag the next event will continue.
    const speed = createScrollSpeed();
    speed.sample(100, 1000, VIEWPORT);
    vi.advanceTimersByTime(SCROLL_SETTLE_MS);
    speed.sample(1_000_000, 1000 + SCROLL_SETTLE_MS + 1, VIEWPORT);
    expect(speed.motion).toEqual({ kind: 'jump', stream: false });
  });

  it('remembers where the grid stopped across a pause, so a small move after one is no jump', () => {
    // The position is still known after the settle; only the time is forgotten.
    const speed = createScrollSpeed();
    speed.sample(50_000, 1000, VIEWPORT);
    vi.advanceTimersByTime(SCROLL_SETTLE_MS);
    speed.sample(50_100, 5000, VIEWPORT);
    expect(speed.motion).toEqual({ kind: 'scroll', direction: 1, speed: 0, peak: 0 });
  });

  it('keeps the peak while a flick slows down, and starts again on a reversal', () => {
    const speed = createScrollSpeed();
    speed.sample(0, 1000, VIEWPORT);
    speed.sample(160, 1016, VIEWPORT); // 10px/ms
    speed.sample(200, 1032, VIEWPORT); // 2.5px/ms
    expect(speed.motion).toEqual({ kind: 'scroll', direction: 1, speed: 2.5, peak: 10 });

    speed.sample(168, 1048, VIEWPORT); // back up at 2px/ms
    expect(speed.motion).toEqual({ kind: 'scroll', direction: -1, speed: 2, peak: 2 });
  });

  it('settles once the events stop', () => {
    const speed = createScrollSpeed();
    speed.sample(0, 0, VIEWPORT);
    speed.sample(200, 16, VIEWPORT);
    expect(speed.motion.kind).toBe('scroll');

    vi.advanceTimersByTime(SCROLL_SETTLE_MS - 1);
    expect(speed.motion.kind).toBe('scroll');
    vi.advanceTimersByTime(1);
    expect(speed.motion).toEqual({ kind: 'still' });
  });

  it('keeps the measured speed across two events with the same timestamp', () => {
    const speed = createScrollSpeed();
    speed.sample(0, 0, VIEWPORT);
    speed.sample(32, 16, VIEWPORT);
    speed.sample(500, 16, VIEWPORT);
    expect(speed.motion).toEqual({ kind: 'scroll', direction: 1, speed: 2, peak: 2 });
  });
});
