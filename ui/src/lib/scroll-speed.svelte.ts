/** Scrolling faster than this, in pixels per millisecond, counts as a fast scroll: about
 *  three screens a second. A reading pace with the wheel stays well under it; a flick's
 *  momentum, a scrollbar drag and a timeline scrub are well over. */
export const FAST_SCROLL_PX_PER_MS = 3;

/** How long without a scroll event before a fast scroll counts as over. Also the longest
 *  gap two events can have and still be measured against each other: the first event after
 *  a pause - a single jump such as End or a folder click - has nothing recent to be a speed
 *  relative to, and a jump of any size is one render, not a stream of them. */
export const SCROLL_SETTLE_MS = 150;

/** Whether the grid is scrolling fast, measured from the scroll events themselves.
 *
 *  Speed is distance over the time between two events' own timestamps, never distance per
 *  event: scroll events arrive once a frame, so a count of pixels per event would call the
 *  same gesture twice as fast on a 60Hz screen as on a 120Hz one. */
export function createScrollSpeed() {
  let fast = $state(false);
  let lastTop: number | null = null;
  let lastAt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function settle() {
    clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      fast = false;
      lastTop = null;
    }, SCROLL_SETTLE_MS);
  }

  return {
    get fast() {
      return fast;
    },

    /** One scroll event: the scroll position it left, and its `timeStamp`. */
    sample(top: number, at: number) {
      const elapsed = at - lastAt;
      if (lastTop !== null && elapsed > 0 && elapsed <= SCROLL_SETTLE_MS) {
        fast = Math.abs(top - lastTop) / elapsed > FAST_SCROLL_PX_PER_MS;
      }
      // Two events with the same timestamp say nothing about speed; the later position is
      // still the one the next event is measured from.
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
