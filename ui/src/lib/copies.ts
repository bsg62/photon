import type { CopiesOf, GridView } from './api';

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
 *  the name it was opened with rather than going blank under the user. `gone` always comes
 *  from `next`, never from `prev`: it is not a name, and carrying it forward would freeze
 *  the view on "still here" after the very purge that made it stale. */
export function keepCopiesName(prev: CopiesOf | null, next: CopiesOf | null): CopiesOf | null {
  if (next && next.fileName === '' && prev && prev.id === next.id) return { ...next, fileName: prev.fileName };
  return next;
}

export function showCopiesLabel(n: number): string {
  return n === 1 ? 'Show 1 duplicate' : `Show ${n.toLocaleString()} duplicates`;
}

/** What the Copies view says about itself, in the `.empty` overlay and the `.lone` line.
 *
 *  Once the anchor photo itself is gone, the filter (keyed off the anchor's own row) matches
 *  nothing regardless of what its copies are doing, so `len` says nothing useful - the notice
 *  is about the anchor, not the count. Otherwise there is something to say only when the view
 *  has shrunk to the anchor alone or emptied outright: `len === 1` with a *different* photo
 *  as the sole entry (impossible today, since a live anchor is always its own first copy, but
 *  not a case this function should silently paper over) says nothing. */
export function copiesNotice(copiesOf: CopiesOf | null, len: number, firstId: number | undefined): string | null {
  if (!copiesOf) return null;
  if (copiesOf.gone) {
    return `${copiesOf.fileName || 'This photo'} is no longer in the library; its copies are under Duplicates.`;
  }
  if (len === 0 || (len === 1 && firstId === copiesOf.id)) {
    return `No other copies of ${copiesOf.fileName || 'this photo'} any more.`;
  }
  return null;
}

/** Whether a tile draws its copies mark. Not in Duplicates or Copies: every photo there has
 *  copies (that is what put it there), so a mark on every tile would say nothing. */
export function copiesMarkShown(hasCopies: boolean, view: GridView): boolean {
  return hasCopies && view !== 'duplicates' && view !== 'copies';
}
