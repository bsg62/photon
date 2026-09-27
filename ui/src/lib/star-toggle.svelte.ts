/** The viewer's star button, separated from the button that renders it.
 *
 *  The flip is optimistic: the star on screen changes on the click, not when the INI write
 *  comes back, because the write goes through a temp file and a rename and a button that
 *  lags its own click reads as broken. What makes optimism safe is the revert on failure,
 *  and what makes the revert safe is the id check: the user can navigate while the call is
 *  in flight, and a revert landing on the *next* photo would flip a star nobody touched.
 *
 *  `setStar` is injected so the machine can be tested without the backend. */
export function createStarToggle(setStar: (id: number, starred: boolean) => Promise<void>) {
  let starred = $state(false);
  /** Photos with a write in flight. Replaced whole, never mutated, so a plain `$state`
   *  wakes readers without proxying the set. */
  let inFlight = $state<ReadonlySet<number>>(new Set());
  /** The photo the button currently belongs to. Plain, not `$state`: nothing renders from
   *  it, and it is only read alongside a change it is already waking for. */
  let bound: number | null = null;

  return {
    get starred() {
      return starred;
    },

    /** A call is in flight for the bound photo. A second click on the same photo in that
     *  window is dropped rather than queued: queuing would only stack up flips that land
     *  after the user has stopped looking. Per photo, not global: the call includes the
     *  grid rebuild after the write, which is a few hundred milliseconds on a large
     *  library, and "star, next photo, star" is a normal pace for culling - a global flag
     *  dropped the second star without a word. */
    get busy() {
      return bound !== null && inFlight.has(bound);
    },

    /** Called when the viewer loads a photo, with that photo's current state. */
    bind(id: number, current: boolean) {
      bound = id;
      starred = current;
    },

    /** Flips the bound photo's star. Rejects with the backend's error after reverting, so
     *  the caller can report it. */
    async toggle(): Promise<void> {
      if (bound === null || inFlight.has(bound)) return;
      const id = bound;
      const next = !starred;
      starred = next;
      inFlight = new Set(inFlight).add(id);
      try {
        await setStar(id, next);
      } catch (e) {
        if (bound === id) starred = !next;
        throw e;
      } finally {
        const rest = new Set(inFlight);
        rest.delete(id);
        inFlight = rest;
      }
    },
  };
}

export type StarToggle = ReturnType<typeof createStarToggle>;
