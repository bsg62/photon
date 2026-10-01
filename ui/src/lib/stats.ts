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

/** The cameras, named and searched for the way the info panel names and searches them. */
export function cameraStatRows(stats: LibraryStats): StatRow[] {
  return rows(
    stats.cameras.flatMap((c) => {
      const name = cameraName(c.make, c.model);
      return name ? [{ label: name, count: c.count, search: fieldQuery('camera', name) }] : [];
    }),
  );
}

export function lensStatRows(stats: LibraryStats): StatRow[] {
  return rows(stats.lenses.map((l) => ({ label: l.lens, count: l.count, search: fieldQuery('lens', l.lens) })));
}
