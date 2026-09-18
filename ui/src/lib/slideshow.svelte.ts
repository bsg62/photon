/** The slideshow's state machine, apart from the viewer that hosts it so its timing can be
 *  tested with fake timers; the component only reports what happened to the photo.
 *
 *  The countdown belongs to a photo that is *on screen*, not to the clock: it starts when the
 *  viewer reports `shown()` and dies on `changed()`. A plain interval would count a slow
 *  decode against the photo, so a large file on a network drive would flash past or be
 *  skipped altogether. */

/** How long the pointer must rest before the viewer's controls fade out. */
export const CHROME_IDLE_MS = 2500;

export interface SlideshowDeps {
  /** Move to the next photo. Doing nothing (a view of one photo) simply ends the countdowns. */
  advance: () => void;
  /** Seconds to hold each photo, read once per start so a change in Settings applies to the
   *  next slideshow rather than shifting under a running one. */
  interval: () => Promise<number>;
  fullscreen: {
    get: () => Promise<boolean>;
    set: (on: boolean) => Promise<void>;
  };
}

export const FALLBACK_INTERVAL_S = 4;

export function createSlideshow(deps: SlideshowDeps) {
  let active = $state(false);
  let playing = $state(false);
  let idle = $state(false);
  let shown = false;
  let intervalMs = FALLBACK_INTERVAL_S * 1000;
  /** Whether the window was already fullscreen, so stopping does not drop the user out of a
   *  fullscreen they chose themselves. Null while we have not changed anything. */
  let wasFullscreen: boolean | null = null;
  /** Bumped by every start and stop, so a start whose awaits are still in flight when the
   *  user has already stopped does not go fullscreen afterwards. */
  let run = 0;
  let countdown: ReturnType<typeof setTimeout> | null = null;
  let idleTimer: ReturnType<typeof setTimeout> | null = null;

  function disarm() {
    if (countdown !== null) clearTimeout(countdown);
    countdown = null;
  }

  function arm() {
    disarm();
    if (!active || !playing || !shown) return;
    countdown = setTimeout(() => {
      countdown = null;
      deps.advance();
    }, intervalMs);
  }

  function poke() {
    idle = false;
    if (idleTimer !== null) clearTimeout(idleTimer);
    idleTimer = active ? setTimeout(() => (idle = true), CHROME_IDLE_MS) : null;
  }

  return {
    /** Slideshow mode is on, playing or paused. */
    get active() {
      return active;
    },
    get playing() {
      return playing;
    },
    /** The pointer has rested long enough that the controls should be out of the way. */
    get idle() {
      return active && idle;
    },

    /** `alreadyShown` is whether the photo on screen has finished loading, which is the
     *  usual case: the slideshow starts from a photo the user is looking at, and no `shown()`
     *  will arrive for it. */
    async start(alreadyShown: boolean) {
      if (active) return;
      const mine = ++run;
      active = true;
      playing = true;
      shown = alreadyShown;
      poke();
      // The countdown does not wait for the two calls below: a failed or slow fullscreen
      // switch must not hold the first photo forever.
      arm();
      try {
        const seconds = await deps.interval();
        if (mine !== run) return;
        if (seconds > 0) intervalMs = seconds * 1000;
        arm();
      } catch {
        // Keep the fallback.
      }
      try {
        const was = await deps.fullscreen.get();
        if (mine !== run) return;
        wasFullscreen = was;
        if (!was) await deps.fullscreen.set(true);
        // Stopped while the switch was in flight: `stop` saw nothing to restore.
        if (mine !== run && !was) await deps.fullscreen.set(false);
      } catch {
        // A windowed slideshow is still a slideshow.
      }
    },

    stop() {
      if (!active) return;
      run++;
      active = false;
      playing = false;
      disarm();
      poke();
      const was = wasFullscreen;
      wasFullscreen = null;
      if (was === false) deps.fullscreen.set(false).catch(() => {});
    },

    toggle() {
      if (!active) return;
      playing = !playing;
      if (playing) arm();
      else disarm();
    },

    /** The photo on screen is being replaced, by the countdown or by hand. */
    changed() {
      shown = false;
      disarm();
    },

    /** The photo is on screen (or has failed for good): its countdown starts now. */
    shown() {
      shown = true;
      arm();
    },

    /** The pointer moved: bring the controls back and restart the wait. */
    poke,
  };
}

export type Slideshow = ReturnType<typeof createSlideshow>;
