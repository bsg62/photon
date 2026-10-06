/** The grid's grouping (`sort::Grouping`), as the UI reads it. Pure. */

import type { Grouping, Period, Sort } from './api';

/** The control's options. Each label says what it is on its own: a closed select shows only
 *  its value, and a bare "Folder" beside "Date taken" does not. */
export const GROUPINGS: { value: Grouping; label: string }[] = [
  { value: 'folder', label: 'By folder' },
  { value: 'day', label: 'By day' },
  { value: 'month', label: 'By month' },
  { value: 'year', label: 'By year' },
  { value: 'none', label: 'No grouping' },
];

/** Whether the grouping decides anything: only by date. By name, size or modification time
 *  every photo is sorted together and the stored grouping waits for the sort to come back. */
export function groupingApplies(sort: Sort): boolean {
  return sort.key === 'date';
}

/** Whether the grid runs folder by folder, each under its header. The place photon
 *  remembers in the library is a folder, so it is a place in this arrangement only: under
 *  any other a jump to that folder lands on one of its photos, somewhere, not where the
 *  user was. */
export function laidOutByFolder(sort: Sort): boolean {
  return sort.key === 'date' && sort.group === 'folder';
}

/** What a period's header says: "2026", "October 2026", "Sunday, October 4, 2026". Built
 *  from the period's own numbers as a local date, which is then formatted as the same local
 *  date - so the zone cancels out, where a timestamp would be read back shifted by it. */
export function periodLabel(period: Period, locale?: string): string {
  if (period.month === null) return String(period.year);
  const date = new Date(period.year, period.month - 1, period.day ?? 1);
  return period.day === null
    ? date.toLocaleDateString(locale, { month: 'long', year: 'numeric' })
    : date.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
}
