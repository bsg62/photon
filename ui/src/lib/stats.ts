/** The Statistics section of Settings, as rows to draw. Pure, so the spelling and the
 *  searches behind the links are pinned by a test. */

import type { LibraryStats } from './api';
import { cameraName, fieldQuery } from './exif';

/** One counted thing: a year, a camera, a lens. */
export interface StatRow {
  label: string;
  count: number;
  /** The row's share of the largest row in its list, 0..1, for the bar beside it. Of the
   *  largest rather than of the total: the bars compare rows, and a library spread over
   *  twenty years would otherwise draw twenty slivers. */
  share: number;
  /** The search that shows the row's photos, in the grammar of `search::Query`. */
  search: string;
}

function rows(items: { label: string; count: number; search: string }[]): StatRow[] {
  const most = Math.max(1, ...items.map((i) => i.count));
  return items.map((i) => ({ ...i, share: i.count / most }));
}

/** "1 photo" / "1,234 photos". */
function counted(n: number, one: string, many: string, locale?: string): string {
  return n === 1 ? `1 ${one}` : `${n.toLocaleString(locale)} ${many}`;
}

/** A library's size on disk: whole megabytes below a gigabyte, GB with one decimal, TB
 *  with two. Binary units, as `formatSize` and the file managers use; it differs from
 *  `formatSize` only in going on past megabytes, which one photo never needs. */
export function librarySize(bytes: number): string {
  const mb = bytes / 1024 ** 2;
  if (mb < 1024) return `${Math.round(mb)} MB`;
  const gb = mb / 1024;
  return gb < 1024 ? `${gb.toFixed(1)} GB` : `${(gb / 1024).toFixed(2)} TB`;
}

/** The line under the heading: "12,034 photos and 310 videos · 48.2 GB · 2004 to 2026".
 *  Videos are named only when there are some, and the years only when there is a photo to
 *  date; a library of one year says the year once. `locale` is for tests. */
export function statsSummary(stats: LibraryStats, locale?: string): string {
  const parts = [counted(stats.photos, 'photo', 'photos', locale)];
  if (stats.videos > 0) parts[0] += ` and ${counted(stats.videos, 'video', 'videos', locale)}`;
  parts.push(librarySize(stats.bytes));
  // The capture times are a camera's wall clock in naive seconds, so the year is read in UTC.
  const year = (seconds: number) => new Date(seconds * 1000).getUTCFullYear();
  if (stats.oldest !== null && stats.newest !== null) {
    const [from, to] = [year(stats.oldest), year(stats.newest)];
    parts.push(from === to ? String(from) : `${from} to ${to}`);
  }
  return parts.join(' · ');
}

/** The years, newest first as the backend sends them. A year's link is the date range of
 *  exactly that year. */
export function yearRows(stats: LibraryStats): StatRow[] {
  return rows(
    stats.years.map((y) => ({ label: String(y.year), count: y.count, search: `from:${y.year} to:${y.year}` })),
  );
}

/** The cameras, named and searched for the way the info panel names and searches them.
 *
 *  The backend counts by make and model as the files spell them, and `cameraName` folds
 *  some of those pairs into one name: "Canon EOS 5D" with the make "Canon" and with none
 *  are one camera to anyone reading the list. Their counts are added and the list sorted
 *  again, so a name appears once - which is also what lets a row be keyed by its search.
 *
 *  A row's link is a search of the camera field, word by word, so it can show more than
 *  the row counts: "EOS 5D" also finds an "EOS 5D Mark IV". The count is of that exact
 *  camera; the link is the nearest thing the search grammar can say. */
export function cameraStatRows(stats: LibraryStats): StatRow[] {
  const byName = new Map<string, number>();
  for (const c of stats.cameras) {
    const name = cameraName(c.make, c.model);
    if (name) byName.set(name, (byName.get(name) ?? 0) + c.count);
  }
  // A stable sort: equal counts keep the backend's order, which is by name.
  const merged = [...byName].sort((a, b) => b[1] - a[1]);
  return rows(merged.map(([label, count]) => ({ label, count, search: fieldQuery('camera', label) })));
}

export function lensStatRows(stats: LibraryStats): StatRow[] {
  return rows(stats.lenses.map((l) => ({ label: l.lens, count: l.count, search: fieldQuery('lens', l.lens) })));
}
