import { library } from './library.svelte';
import { debounce, SEARCH_DEBOUNCE_MS, shouldAdoptBackendQuery } from './search';

/** The search box's state machine, separated from the input that renders it.
 *
 *  The box lives in the top bar, but the folder tree has to be able to cancel a pending
 *  debounced send: a jump or the Starred click switches the view, and a debounce that fires
 *  afterwards would re-enter Search with its captured text and replace the grid the user
 *  just asked for. Two components needing the same `cancel` is why this is a module rather
 *  than component state.
 *
 *  `send` is injected so the machine can be tested without the backend. */
export function createSearchBox(send: (query: string) => Promise<void>, initial = '') {
  let query = $state(initial);

  /** How many sends of ours have been dispatched but not yet settled. `LibraryStore`
   *  serialises them, so more than one can be outstanding at once (a second send already
   *  queued behind a first that's still in flight); while any is outstanding, an echo
   *  arriving on `library.info.searchQuery` might belong to an older, since-superseded send
   *  rather than the latest one, so `syncFromBackend` must not adopt anything until the
   *  count reaches zero. `$state` so the effect watching it re-runs once it does. */
  let outstanding = $state(0);

  /** The last value we sent. The backend echoes it back, and `syncFromBackend` has to tell
   *  that echo from a change made anywhere else. Plain, not `$state`: only that method reads
   *  it, and always alongside a change it is already waking for. */
  let lastSent: string | null = null;

  function dispatch(q: string): Promise<void> {
    lastSent = q;
    outstanding++;
    return send(q).finally(() => outstanding--);
  }

  const run = debounce((q: string) => void dispatch(q), SEARCH_DEBOUNCE_MS);

  return {
    get query() {
      return query;
    },
    set query(q: string) {
      query = q;
    },

    /** Exposed so the component's sync effect can depend on it: every echo is declined
     *  until this reaches zero, so the effect has to wake when it does. */
    get outstanding() {
      return outstanding;
    },

    /** Debounced send of the current box contents. */
    run(q: string) {
      run(q);
    },

    /** Drops a pending debounced send rather than firing it. Callers that switch the view
     *  must do this first; see the module comment. */
    cancel() {
      run.cancel();
    },

    clear() {
      // Cancel first: a pending debounced call would otherwise land after the clear and put
      // the backend straight back into the search view. Cancelling only stops a call that
      // hasn't fired yet; a call already dispatched can't be cancelled, which is why
      // `outstanding` exists — `syncFromBackend` declines every echo until all dispatched
      // calls, including this one, have settled in order.
      run.cancel();
      query = '';
      void dispatch('');
    },

    /** The backend is the source of truth for the active query (spec §5): clicking Starred
     *  or a folder clears it server-side, and without this the box would keep displaying
     *  text that no longer filters anything. `shouldAdoptBackendQuery` is what tells an
     *  external change (adopt it) from our own echo of a keystroke (must not be adopted —
     *  doing so would snap the box back to stale text while the user is still typing ahead
     *  of it). Both `lastSent` and `outstanding` are needed: the count alone says every send
     *  has landed, not that what landed came from anywhere but us, and shipping only the
     *  count is what made the box swallow characters. */
    syncFromBackend(backend: string) {
      if (shouldAdoptBackendQuery(backend, query, lastSent, outstanding)) query = backend;
    },
  };
}

export type SearchBox = ReturnType<typeof createSearchBox>;

export const searchBox = createSearchBox((q) => library.setSearchQuery(q), library.info.searchQuery);
