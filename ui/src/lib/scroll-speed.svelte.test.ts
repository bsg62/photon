import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createScrollSpeed, FAST_SCROLL_PX_PER_MS, SCROLL_SETTLE_MS } from './scroll-speed.svelte';

describe('createScrollSpeed', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('calls a scroll fast only past the speed, by time and not by event', () => {
    const slow = createScrollSpeed();
    // Just under the speed: 60Hz frames, each moving a little less than the limit allows.
    const frame = 16;
    const under = FAST_SCROLL_PX_PER_MS * frame - 4;
    for (let i = 0; i < 5; i++) slow.sample(i * under, i * frame);
    expect(slow.fast).toBe(false);

    // The same distance per event at 240Hz is four times the speed.
    const quick = createScrollSpeed();
    for (let i = 0; i < 5; i++) quick.sample(i * under, i * (frame / 4));
    expect(quick.fast).toBe(true);
  });

  it('does not call a single jump fast, however far it goes', () => {
    // End, a folder jump: one event, a pause's length after the last one.
    const speed = createScrollSpeed();
    speed.sample(0, 1000);
    speed.sample(1_000_000, 1000 + SCROLL_SETTLE_MS + 1);
    expect(speed.fast).toBe(false);
  });

  it('settles once the events stop', () => {
    const speed = createScrollSpeed();
    speed.sample(0, 0);
    speed.sample(2000, 16);
    expect(speed.fast).toBe(true);

    vi.advanceTimersByTime(SCROLL_SETTLE_MS - 1);
    expect(speed.fast).toBe(true);
    vi.advanceTimersByTime(1);
    expect(speed.fast).toBe(false);
  });

  it('ignores two events with the same timestamp', () => {
    const speed = createScrollSpeed();
    speed.sample(0, 0);
    speed.sample(10, 16);
    speed.sample(5000, 16);
    expect(speed.fast).toBe(false);
  });
});
