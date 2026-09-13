import type { GridView } from './api';

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
 *  While any send of ours is still outstanding, decline everything: echoes arrive one at a
 *  time and an older one would otherwise overwrite what the user has since typed. Once the
 *  chain drains, the backend is authoritative and the box syncs to it — which is what makes
 *  a folder jump or the Starred click (both of which clear the query server-side) reach the
 *  box, the whole reason this sync exists.
 *
 *  A single "last value sent" is not enough once sends can overlap in flight (they do:
 *  `LibraryStore.setSearchQuery` serialises calls, so a second send can already be queued
 *  behind the first): the first send's echo arrives while the counter has already advanced
 *  to the second value, so it looks like an external change and gets adopted, snapping the
 *  box backwards; the second, correct echo then matches and is wrongly declined — and
 *  nothing ever resyncs it. Counting outstanding sends instead of remembering only the last
 *  one covers every ordering. */
export function shouldAdoptBackendQuery(backend: string, current: string, outstanding: number): boolean {
  return outstanding === 0 && backend !== current;
}

/** Whether the grid is showing a different set of photos than it was. Used to decide
 *  whether to reset scroll: keying on `view` alone would miss a refined query staying
 *  within the Search view (spec §5). */
export function resultsChanged(
  prev: { view: GridView; query: string },
  next: { view: GridView; query: string },
): boolean {
  return prev.view !== next.view || prev.query !== next.query;
}
