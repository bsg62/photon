import type { SavedSearch } from './api';

/** The saved search the active query already is, if any.
 *
 *  The comparison is exact after trimming, and deliberately case-sensitive: the search
 *  grammar reads `OR` and `AND` as operators only in capitals, so `lake or pond` and
 *  `lake OR pond` are two different searches and must be savable separately. Trimming
 *  matches what `create_saved_search` stores, or a query typed with a trailing space would
 *  look unsaved right after being saved. */
export function savedSearchFor(
  searches: SavedSearch[],
  query: string,
): SavedSearch | undefined {
  const wanted = query.trim();
  if (wanted === '') return undefined;
  return searches.find((s) => s.query === wanted);
}

/** What the bookmark button offers as a name: the query itself, which is what the sidebar
 *  then shows. Renaming it to something friendlier is a right-click away, and picking the
 *  query means the button never has to stop and ask. */
export function defaultSearchName(query: string): string {
  return query.trim();
}

/** Whether the bookmark button can save what is in the box: there has to be a query, and
 *  it must not already be saved. A saved query leaves the button filled and inert rather
 *  than removing it - deleting is done from the sidebar, which asks first. */
export function canSaveSearch(searches: SavedSearch[], query: string): boolean {
  return query.trim() !== '' && savedSearchFor(searches, query) === undefined;
}

/** The search behind the sidebar's "On this day": photos taken on `now`'s month and day in
 *  any year, in the grammar of `photon-core`'s `search::Query`. The day is the machine's
 *  own - "today" is where the user is - while a capture date is the camera's wall clock, so
 *  the two meet as plain calendar days with no zone between them. */
export function onThisDayQuery(now: Date): string {
  const two = (n: number) => String(n).padStart(2, '0');
  return `on:${two(now.getMonth() + 1)}-${two(now.getDate())}`;
}

/** "14 July", for the row's tooltip. `locale` is for tests. */
export function onThisDayLabel(now: Date, locale?: string): string {
  return now.toLocaleDateString(locale, { day: 'numeric', month: 'long' });
}
