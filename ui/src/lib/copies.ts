import type { CopiesOf } from './api';

/** "Show duplicates" from the tile menu: lands the grid on the Copies view with the photo
 *  selected. The same order as `locateItem`, for the same reason: an offset only means
 *  something against the index it was looked up in, so the view switches first. */
export async function showCopies(
  itemId: number,
  deps: {
    cancelSearch: () => void;
    setCopiesView: (itemId: number) => Promise<void>;
    offsetOfItem: (itemId: number) => Promise<number | null>;
    select: (offset: number, itemId: number) => void;
  },
): Promise<void> {
  deps.cancelSearch();
  await deps.setCopiesView(itemId);
  const at = await deps.offsetOfItem(itemId);
  if (at === null) return;
  deps.select(at, itemId);
}

/** The backend reports an empty name once the photo has been purged; the sidebar row keeps
 *  the name it was opened with rather than going blank under the user. */
export function keepCopiesName(prev: CopiesOf | null, next: CopiesOf | null): CopiesOf | null {
  if (next && next.fileName === '' && prev && prev.id === next.id) return { ...next, fileName: prev.fileName };
  return next;
}

export function showCopiesLabel(n: number): string {
  return n === 1 ? 'Show 1 duplicate' : `Show ${n.toLocaleString()} duplicates`;
}
