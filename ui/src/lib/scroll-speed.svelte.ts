/** How long without a scroll event before a scroll counts as over. Also the longest gap two
 *  events can have and still be measured against each other: the first event after a pause
 *  has nothing recent to be a speed relative to. */
export const SCROLL_SETTLE_MS = 150;

/** What the grid's scroll is doing, as the render window needs to know it.
 *
 *  - `still`: no scroll event for `SCROLL_SETTLE_MS`.
 *  - `scroll`: continuous movement - each event moved the grid less than a viewport, so what
 *    the next frame shows overlaps what this one showed. `direction` is 1 down, -1 up.
 *    `speed` is the latest measured speed in pixels per millisecond, 0 when there was no
 *    recent event to measure against; `peak` is the fastest this run has gone in this
 *    direction, which is what the lead is sized by, so a flick slowing down does not
 *    unmount the rows it already mounted ahead of itself only to mount them again.
 *  - `jump`: one event moved the grid a viewport or more - End, a folder jump, a scrollbar
 *    or timeline drag. Nothing on screen before it is on screen after it. `stream` is true
 *    when it came within `SCROLL_SETTLE_MS` of the previous event - a drag, where the next
 *    jump is already on its way - and false for a jump on its own. */
export type Motion =
  | { kind: 'still' }
  | { kind: 'scroll'; direction: 1 | -1; speed: number; peak: number }
  | { kind: 'jump'; stream: boolean };

export const STILL: Motion = { kind: 'still' };

/** Follows the grid's scroll events and says what kind of movement they are.
 *
 *  Speed is distance over the time between two events' own timestamps, never distance per
 *  event: scroll events arrive once a frame, so a count of pixels per event would call the
 *  same gesture twice as fast on a 60Hz screen as on a 120Hz one. A jump *is* measured per
 *  event, on purpose: it is a statement about two consecutive renders - whether the second
 *  shares any rows with the first - not about how fast anything is moving. */
export function createScrollSpeed() {
  let motion = $state.raw<Motion>(STILL);
  // The scroll position is known across a pause - the grid stays where it was - so a jump
  // after one is still measured from it. Only the time is forgotten.
  let lastTop = 0;
  let lastAt: number | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function settle() {
    clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      motion = STILL;
      lastAt = null;
    }, SCROLL_SETTLE_MS);
  }

  return {
    get motion() {
      return motion;
    },

    /** One scroll event: the scroll position it left, its `timeStamp`, and the viewport's
     *  height, which is what decides whether it was a jump. */
    sample(top: number, at: number, viewport: number) {
      const moved = top - lastTop;
      const elapsed = lastAt === null ? null : at - lastAt;
      const recent = elapsed !== null && elapsed <= SCROLL_SETTLE_MS;
      if (moved !== 0) {
        if (Math.abs(moved) >= viewport) {
          motion = { kind: 'jump', stream: recent };
        } else {
          const direction = moved > 0 ? 1 : -1;
          const same = motion.kind === 'scroll' && motion.direction === direction ? motion : null;
          // Two events with the same timestamp say nothing about speed; the run's own speed
          // stands until one that does.
          const speed = elapsed !== null && recent && elapsed > 0 ? Math.abs(moved) / elapsed : (same?.speed ?? 0);
          motion = { kind: 'scroll', direction, speed, peak: Math.max(speed, same?.peak ?? 0) };
        }
      }
      lastTop = top;
      lastAt = at;
      settle();
    },

    /** Stops listening for the settle, for a component that is going away. */
    dispose() {
      clearTimeout(timer);
      timer = undefined;
    },
  };
}

export type ScrollSpeed = ReturnType<typeof createScrollSpeed>;
