/** The year strip beside the grid. Pure geometry, like `layout.ts`: the component only
 *  wires pointer events to these.
 *
 *  The strip is the grid's canvas scaled to the strip's height, so a year takes the share of
 *  the strip its photos take of the scroll, and a point on the strip is a scroll position.
 *  It reads each section's `takenAtMin` — the value the sidebar groups folders by — so the
 *  strip, the sidebar and the grid stay on one axis. */

import type { Section } from './api';
import { yearOf } from './folders';
import { lastIndexAtOrBefore, type Row } from './layout';

export interface YearMark {
  year: number;
  /** Canvas y of the header of the first section of this run of the year. */
  top: number;
}

/** One mark wherever the year changes from the section before.
 *
 *  Not one mark per distinct year: folder-first views run newest year to oldest, but a
 *  search places a folder by its oldest photo while `takenAtMin` is its oldest *matching*
 *  one, so years can come back. A repeated year gets a second mark, which is the truth of
 *  what scrolling there shows. */
export function yearMarks(sections: Section[], rows: Row[]): YearMark[] {
  const marks: YearMark[] = [];
  for (const row of rows) {
    if (row.kind !== 'header') continue;
    const section = sections[row.section];
    if (!section) continue;
    const year = yearOf(section.takenAtMin);
    if (marks[marks.length - 1]?.year !== year) marks.push({ year, top: row.top });
  }
  return marks;
}

/** The year showing at canvas position `y`, or null with no marks. */
export function yearAt(marks: YearMark[], y: number): number | null {
  if (marks.length === 0) return null;
  return marks[lastIndexAtOrBefore(marks, y, (m) => m.top)].year;
}

/** Where a mark sits on a strip `strip` pixels tall, for a canvas `total` pixels tall. */
export function stripY(top: number, total: number, strip: number): number {
  return total > 0 ? (top / total) * strip : 0;
}

/** The marks that get a printed label: greedily from the top, each at least `minGap` strip
 *  pixels below the last one kept. A decade of small years would otherwise print as one
 *  smear; the ones skipped are still reachable, since the hover bubble reads `yearAt`. */
export function labelledMarks(marks: YearMark[], total: number, strip: number, minGap: number): YearMark[] {
  const kept: YearMark[] = [];
  let last = -Infinity;
  for (const mark of marks) {
    const y = stripY(mark.top, total, strip);
    if (y - last < minGap) continue;
    kept.push(mark);
    last = y;
  }
  return kept;
}

/** The scroll position for a pointer `y` pixels down the strip. A label sits at its year's
 *  own `top` scaled, so pressing on a label lands exactly on that year's first header —
 *  until the clamp, which is what the last screenful of any scroll does. */
export function scrollTopFor(y: number, strip: number, total: number, viewport: number): number {
  if (strip <= 0) return 0;
  const target = (Math.min(Math.max(y, 0), strip) / strip) * total;
  return Math.max(0, Math.min(target, total - viewport));
}
