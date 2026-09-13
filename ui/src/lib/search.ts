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

/** Whether the grid is showing a different set of photos than it was. Used to decide
 *  whether to reset scroll: keying on `view` alone would miss a refined query staying
 *  within the Search view (spec §5). */
export function resultsChanged(
  prev: { view: GridView; query: string },
  next: { view: GridView; query: string },
): boolean {
  return prev.view !== next.view || prev.query !== next.query;
}
