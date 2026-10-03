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

/** Whether the grid is showing a different set of photos than it was. Used to decide
 *  whether to reset scroll: keying on `view` alone would miss a refined query staying
 *  within the Search view (spec §5), or one album replacing another. */
export function resultsChanged(prev: ViewKey, next: ViewKey): boolean {
  return prev.view !== next.view || prev.query !== next.query || prev.order !== next.order;
}

/** What `resultsChanged` compares. `order` is the sort, as one string: the same photos in
 *  another order are another list, and a scroll position in one is arbitrary in the other. */
export interface ViewKey {
  view: GridView;
  query: string;
  order: string;
}

/** The view and its argument as `resultsChanged` compares them: the query for Search, the
 *  person's key (`Person.key`) for Person, the album id for Album, the keyword for Tag, the
 *  anchor photo id for Copies, and nothing else - plus the sort, which reorders every view. */
export function viewKey(
  info: Pick<GridInfo, 'view' | 'sort' | 'searchQuery' | 'person' | 'album' | 'tag' | 'copiesOf'>,
): ViewKey {
  const query =
    info.view === 'search' ? info.searchQuery
    : info.view === 'person' ? (info.person ?? '')
    : info.view === 'album' ? String(info.album ?? '')
    : info.view === 'tag' ? (info.tag ?? '')
    : info.view === 'copies' ? String(info.copiesOf?.id ?? '')
    : '';
  return { view: info.view, query, order: `${info.sort.reverse ? '-' : ''}${info.sort.key}` };
}
