import { library } from './library.svelte';
import { debounce, SEARCH_DEBOUNCE_MS } from './search';

/** The search box's state machine, separated from the input that renders it.
 *
 *  The box lives in the top bar, but the folder tree has to be able to cancel a pending
 *  debounced send: a jump or the Starred click switches the view, and a debounce that fires
 *  afterwards would re-enter Search with its captured text and replace the grid the user
 *  just asked for. Two components needing the same `cancel` is why this is a module rather
 *  than component state.
 *
 *  The box's text is only ever written from this side. The backend clears its query on any
 *  view switch, and every view switch is asked for by the UI, so the switch empties the box
 *  itself (`leave`, run by `LibraryStore.switchView` as the switch is issued) rather than
 *  the box watching the backend's query and trying to tell its own echo from a change made
 *  elsewhere - a guess that took three attempts and still swallowed characters.
 *
 *  `send` is injected so the machine can be tested without the backend. */
export function createSearchBox(send: (query: string) => Promise<void>) {
  let query = $state('');

  const run = debounce((q: string) => void send(q), SEARCH_DEBOUNCE_MS);

  return {
    get query() {
      return query;
    },
    set query(q: string) {
      query = q;
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

    /** Runs `q` now, as though it had been typed and the debounce had elapsed: what a link
     *  elsewhere in the UI (the info panel's camera and lens) calls. A pending debounced
     *  send is dropped first, or the text the user had half-typed would land after this
     *  and replace it. */
    search(q: string) {
      run.cancel();
      query = q;
      void send(q);
    },

    clear() {
      // Cancel first: a pending debounced call would otherwise land after the clear and put
      // the backend straight back into the search view. A call already dispatched can't be
      // cancelled; `LibraryStore` applies it before this one instead.
      run.cancel();
      query = '';
      void send('');
    },

    /** The view is switching away, which clears the backend's query: the box empties to
     *  match, and a pending send is dropped so it cannot re-enter Search behind the switch.
     *  Returns how to put the text back if the switch is refused - but only while the box
     *  is still empty, since anything typed since is what the user wants now. */
    leave(): () => void {
      run.cancel();
      const left = query;
      query = '';
      return () => {
        if (query === '') query = left;
      };
    },
  };
}

export type SearchBox = ReturnType<typeof createSearchBox>;

export const searchBox = createSearchBox((q) => library.setSearchQuery(q));
library.onViewSwitch(() => searchBox.leave());
