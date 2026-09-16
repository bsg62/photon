import type { GridInfo, GridView } from './api';

export const SEARCH_DEBOUNCE_MS = 150;

/** Calls `fn` once the caller stops calling for `ms`. `cancel` drops a pending call rather
 *  than firing it: firing-then-clearing (a "flush") would send two un-ordered async calls
 *  and re-enter whatever view the flushed call requested — the same race the awaited
 *  `setView` in `jumpToFolder` exists to prevent. */
export function debounce<T extends (...args: never[]) => void>(
  fn: T,
  ms: number,
): ((...args: Parameters<T>) => void) & { cancel(): void } {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const debounced = (...args: Parameters<T>) => {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      fn(...args);
    }, ms);
  };
  debounced.cancel = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  };
  return debounced;
}

/** Whether the box should adopt a query that arrived from the backend.
 *
 *  Two separate things have to be true, and an earlier version of this shipped with only
 *  one of them — the box swallowed characters while typing as a result.
 *
 *  `outstanding` is the ordering half. Sends overlap (`LibraryStore.setSearchQuery`
 *  serialises them, so a second can already be queued behind a first), and their echoes
 *  arrive one at a time; adopting one while another is still in flight would take a
 *  since-superseded value for the current one.
 *
 *  `lastSent` is the identity half, and is the one that was missing. A count reaching zero
 *  says every send has landed — not that the value that landed came from anywhere but us.
 *  Typing `b`, waiting for it to settle, and typing `each` meanwhile leaves the backend
 *  holding `b` while the box holds `beach`: the count is zero and the values differ, so
 *  without this clause the effect adopts `b` and the user watches `each` disappear.
 *
 *  Comparing against `lastSent` draws the line where it belongs. After the chain drains the
 *  backend holds exactly what we last sent, so our own echo is declined; a folder jump or
 *  the Starred click clears the query server-side to something we never sent, so that is
 *  adopted — which is the whole reason this sync exists. */
export function shouldAdoptBackendQuery(
  backend: string,
  current: string,
  lastSent: string | null,
  outstanding: number,
): boolean {
  return outstanding === 0 && backend !== lastSent && backend !== current;
}

/** Whether the grid is showing a different set of photos than it was. Used to decide
 *  whether to reset scroll: keying on `view` alone would miss a refined query staying
 *  within the Search view (spec §5), or one album replacing another. */
export function resultsChanged(
  prev: { view: GridView; query: string },
  next: { view: GridView; query: string },
): boolean {
  return prev.view !== next.view || prev.query !== next.query;
}

/** The view and its argument as `resultsChanged` compares them: the query for Search, the
 *  contact for Person, the album id for Album, the keyword for Tag, and nothing else. */
export function viewKey(info: Pick<GridInfo, 'view' | 'searchQuery' | 'person' | 'album' | 'tag'>): {
  view: GridView;
  query: string;
} {
  const query =
    info.view === 'search' ? info.searchQuery
    : info.view === 'person' ? (info.person ?? '')
    : info.view === 'album' ? String(info.album ?? '')
    : info.view === 'tag' ? (info.tag ?? '')
    : '';
  return { view: info.view, query };
}
