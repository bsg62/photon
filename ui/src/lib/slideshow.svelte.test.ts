import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CHROME_IDLE_MS, createSlideshow } from './slideshow.svelte';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

function setup(opts: { seconds?: number; fullscreen?: boolean } = {}) {
  const advance = vi.fn();
  let fullscreen = opts.fullscreen ?? false;
  const set = vi.fn(async (on: boolean) => {
    fullscreen = on;
  });
  const show = createSlideshow({
    advance,
    interval: async () => opts.seconds ?? 4,
    fullscreen: { get: async () => fullscreen, set },
  });
  return { show, advance, set, isFullscreen: () => fullscreen };
}

/** Lets `start`'s awaits settle without moving the clock. */
const settle = () => vi.advanceTimersByTimeAsync(0);

describe('createSlideshow', () => {
  it('advances one interval after the photo is shown, not after the last advance', async () => {
    const { show, advance } = setup({ seconds: 4 });
    await show.start(true);
    await settle();

    vi.advanceTimersByTime(3999);
    expect(advance).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(advance).toHaveBeenCalledTimes(1);

    // The next photo takes ten seconds to decode. A plain interval would have fired twice
    // by now and skipped it; the countdown has not started.
    show.changed();
    vi.advanceTimersByTime(10_000);
    expect(advance).toHaveBeenCalledTimes(1);
    show.shown();
    vi.advanceTimersByTime(4000);
    expect(advance).toHaveBeenCalledTimes(2);
  });

  it('does not count a photo that is still loading, on start or on resume', async () => {
    const { show, advance } = setup();
    // Started on a photo whose full image has not decoded yet.
    await show.start(false);
    await settle();
    vi.advanceTimersByTime(60_000);
    expect(advance).not.toHaveBeenCalled();

    // Paused and resumed while it is still loading: resuming must not start the clock.
    show.toggle();
    show.toggle();
    vi.advanceTimersByTime(60_000);
    expect(advance).not.toHaveBeenCalled();

    show.shown();
    vi.advanceTimersByTime(4000);
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it('uses the stored interval', async () => {
    const { show, advance } = setup({ seconds: 9 });
    await show.start(true);
    await settle();
    vi.advanceTimersByTime(8999);
    expect(advance).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it('holds while paused and gives a resumed photo a full interval', async () => {
    const { show, advance } = setup();
    await show.start(true);
    await settle();
    vi.advanceTimersByTime(3000);
    show.toggle();
    expect(show.playing).toBe(false);
    vi.advanceTimersByTime(60_000);
    expect(advance).not.toHaveBeenCalled();

    show.toggle();
    vi.advanceTimersByTime(3999);
    expect(advance).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it('restarts the countdown when the user steps by hand', async () => {
    const { show, advance } = setup();
    await show.start(true);
    await settle();
    vi.advanceTimersByTime(3000);
    show.changed();
    show.shown();
    vi.advanceTimersByTime(3000);
    expect(advance).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it('never advances after stop', async () => {
    const { show, advance } = setup();
    await show.start(true);
    await settle();
    show.stop();
    show.shown();
    vi.advanceTimersByTime(60_000);
    expect(advance).not.toHaveBeenCalled();
    expect(show.active).toBe(false);
  });

  it('goes fullscreen for the show and leaves it afterwards', async () => {
    const { show, isFullscreen } = setup();
    await show.start(true);
    expect(isFullscreen()).toBe(true);
    show.stop();
    await settle();
    expect(isFullscreen()).toBe(false);
  });

  it('leaves alone a fullscreen the user was already in', async () => {
    const { show, set, isFullscreen } = setup({ fullscreen: true });
    await show.start(true);
    show.stop();
    await settle();
    expect(set).not.toHaveBeenCalled();
    expect(isFullscreen()).toBe(true);
  });

  it('does not strand the window fullscreen when stopped while still starting', async () => {
    const { show, isFullscreen } = setup();
    const starting = show.start(true);
    show.stop();
    await starting;
    await settle();
    expect(isFullscreen()).toBe(false);
  });

  it('undoes a fullscreen switch that lands after the stop', async () => {
    // The window manager takes its time; the user presses Escape meanwhile. `stop` has
    // nothing recorded to restore yet, so the late switch has to undo itself.
    let fullscreen = false;
    let release = () => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    const show = createSlideshow({
      advance: vi.fn(),
      interval: async () => 4,
      fullscreen: {
        get: async () => fullscreen,
        set: async (on) => {
          if (on) await gate;
          fullscreen = on;
        },
      },
    });
    const starting = show.start(true);
    await settle();
    show.stop();
    release();
    await starting;
    await settle();
    expect(fullscreen).toBe(false);
  });

  it('plays on in a window when fullscreen is refused', async () => {
    const advance = vi.fn();
    const show = createSlideshow({
      advance,
      interval: async () => 4,
      fullscreen: {
        get: async () => {
          throw new Error('denied');
        },
        set: async () => {},
      },
    });
    await show.start(true);
    vi.advanceTimersByTime(4000);
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it('hides the controls once the pointer rests, and only during a slideshow', async () => {
    const { show } = setup();
    show.poke();
    vi.advanceTimersByTime(CHROME_IDLE_MS * 2);
    expect(show.idle).toBe(false);

    await show.start(true);
    vi.advanceTimersByTime(CHROME_IDLE_MS - 1);
    expect(show.idle).toBe(false);
    show.poke();
    vi.advanceTimersByTime(CHROME_IDLE_MS - 1);
    expect(show.idle).toBe(false);
    vi.advanceTimersByTime(1);
    expect(show.idle).toBe(true);

    show.stop();
    expect(show.idle).toBe(false);
  });
});
