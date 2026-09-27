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
