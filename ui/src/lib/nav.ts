import type { SectionLike } from './layout';

export type NavKey = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' | 'Home' | 'End';

function sectionIndexOf(sections: SectionLike[], offset: number): number {
  let lo = 0;
  let hi = sections.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (sections[mid].offset <= offset) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** The grid offset selected after pressing `key`. Vertical moves follow the on-screen
 *  rows, which restart at every section. */
export function move(offset: number, key: NavKey, sections: SectionLike[], columns: number): number {
  const lastSection = sections[sections.length - 1];
  const len = lastSection ? lastSection.offset + lastSection.count : 0;
  if (len === 0) return 0;
  switch (key) {
    case 'ArrowLeft':
      return Math.max(0, offset - 1);
    case 'ArrowRight':
      return Math.min(len - 1, offset + 1);
    case 'Home':
      return 0;
    case 'End':
      return len - 1;
  }
  const si = sectionIndexOf(sections, offset);
  const s = sections[si];
  const local = offset - s.offset;
  const column = local % columns;
  if (key === 'ArrowDown') {
    if (local + columns < s.count) return offset + columns;
    const lastRowStart = Math.floor((s.count - 1) / columns) * columns;
    if (local < lastRowStart) return s.offset + s.count - 1;
    const next = sections[si + 1];
    return next ? next.offset + Math.min(column, next.count - 1) : offset;
  }
  if (local - columns >= 0) return offset - columns;
  const prev = sections[si - 1];
  if (!prev) return offset;
  const prevLastRow = Math.floor((prev.count - 1) / columns) * columns;
  return prev.offset + Math.min(prevLastRow + column, prev.count - 1);
}
