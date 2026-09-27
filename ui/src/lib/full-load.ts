/** How long the viewer must rest on a photo reached by hand before its full-size file is
 *  requested, along with the neighbour preload.
 *
 *  The preview thumbnail is on screen at once and covers the gap. Long enough that a held
 *  arrow key, which repeats every 30-50ms, asks for none of the photos it passes; short
 *  enough to be lost in the preview-to-full swap when the user stops. */
export const FULL_LOAD_SETTLE_MS = 130;

/** When the viewer may start the full-size load for the photo it has just landed on.
 *
 *  The full file is 10-30 MB, decoded to around 100 MB of pixels, and once asked for the
 *  webview gives no way to cancel the fetch on the Rust side. Requested on every step,
 *  holding an arrow key queued dozens of them - and their neighbours' preloads - and the
 *  photo the user finally stopped on waited behind them all.
 *
 *  So a step by hand waits `FULL_LOAD_SETTLE_MS` first, and the next step cancels the wait.
 *  Three cases do not wait, because nothing is being flicked past:
 *  - the first photo the viewer opens on (there is no step to settle from, and a delay would
 *    only slow the open);
 *  - a reload of the offset already loaded (an edit, a rewritten file);
 *  - a slideshow, whose countdown and crossfade are timed from the photo being shown, and
 *    which changes photo once an interval, never in a burst. */
export function createFullLoad() {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let settle: ((go: boolean) => void) | undefined;
  let last: number | undefined;

  function cancel() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    settle?.(false);
    settle = undefined;
  }

  return {
    /** The viewer has landed on grid offset `at`. Resolves true when the full-size load may
     *  start, or false if another `wait` or a `cancel` came first. */
    wait(at: number, opts: { slideshow: boolean }): Promise<boolean> {
      cancel();
      const stepped = last !== undefined && last !== at;
      last = at;
      if (!stepped || opts.slideshow) return Promise.resolve(true);
      return new Promise((resolve) => {
        settle = resolve;
        timer = setTimeout(() => {
          timer = undefined;
          settle = undefined;
          resolve(true);
        }, FULL_LOAD_SETTLE_MS);
      });
    },

    /** The photo loaded at the last offset is now numbered `at` - a rebuild renumbered it,
     *  and the viewer kept it on screen without loading anything. A later reload of it is
     *  then still a reload, not a step from its old number. */
    renumbered(at: number) {
      last = at;
    },

    /** Abandons a pending wait: the viewer moved on, or closed. */
    cancel,
  };
}

export type FullLoad = ReturnType<typeof createFullLoad>;

/** The viewer's load in flight, held apart from the effect that starts it.
 *
 *  The loader effect re-runs whenever `current` changes, and a rebuild that renumbers the
 *  photo on screen changes it too - without the photo changing. Were the load's teardown
 *  the effect's own cleanup, Svelte would run it on that re-run: the full-size image still
 *  fetching or decoding would be aborted, a playing video's source stripped, and the
 *  re-run, rightly starting nothing for a photo already on screen, would leave the viewer
 *  on the preview for good. During an import scan, rebuilding every 250ms and shifting
 *  offsets as it goes, that was any photo opened - and a slideshow, whose countdown waits
 *  on the full image, stalled.
 *
 *  So the teardown lives here: `begin` runs it only for a run that is not a renumbering,
 *  and `end` on unmount. */
export function createLoadSlot() {
  let rebound: number | null = null;
  let teardown: (() => void) | null = null;

  function end() {
    const t = teardown;
    teardown = null;
    t?.();
  }

  return {
    /** A rebuild renumbered the photo on screen to `at`: the run that follows for `at` is
     *  the same photo, not a step to another. */
    rebind(at: number) {
      rebound = at;
    },

    /** The photo on screen must load again at its offset (its picture changed): the next
     *  run is a reload even if a renumbering to that offset was pending. */
    forget() {
      rebound = null;
    },

    /** The loader is running for `at`. False for a renumbering, leaving the load in flight
     *  running, since it is the right photo's. Otherwise tears that load down and answers
     *  true: the caller starts the new one and hands its teardown to `hold`. */
    begin(at: number): boolean {
      const renumbered = rebound === at;
      // Consumed either way: a renumbering overtaken by a step must not match a later run
      // that happens to land on the same offset.
      rebound = null;
      if (renumbered) return false;
      end();
      return true;
    },

    /** How to tear down the load `begin` just allowed. */
    hold(t: () => void) {
      teardown = t;
    },

    /** Tears down the load in flight: the viewer is closing. */
    end,
  };
}
