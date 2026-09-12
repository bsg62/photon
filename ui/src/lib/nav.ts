import type { SectionLike } from './layout';

export type NavKey = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' | 'Home' | 'End';

/** Zoom runs from "fit the window" to 400%, the same ceiling Picasa used. */
export const MIN_ZOOM = 1;
export const MAX_ZOOM = 4;

/** How much wheel delta adds up to one photo. A mouse notch is ~100, so one notch is one
 *  photo; a trackpad's stream of small deltas is rate-limited instead of flying through
 *  the folder. */
export const WHEEL_THRESHOLD = 100;

export interface FolderPosition {
  /** 1-based position within the folder, or 0 when there is nothing to number. */
  index: number;
  count: number;
}

export interface WheelResult {
  accumulated: number;
  step: -1 | 0 | 1;
}

export interface Pan {
  x: number;
  y: number;
}

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

/** Where a grid offset sits *within its own folder*. The viewer counts photos per folder
 *  rather than across the whole library, so "3 / 40" means the third of forty in this
 *  folder — the number a person can check against their file manager. */
export function positionInFolder(sections: SectionLike[], offset: number): FolderPosition {
  if (sections.length === 0) return { index: 0, count: 0 };
  const s = sections[sectionIndexOf(sections, offset)];
  return { index: offset - s.offset + 1, count: s.count };
}

/** Folds one wheel event into a running total, emitting a step only once the total passes
 *  `threshold`. Returns the total to carry into the next event. */
export function wheelStep(
  accumulated: number,
  delta: number,
  threshold: number = WHEEL_THRESHOLD,
): WheelResult {
  // A reversal starts a new gesture: whatever the old direction banked up is dropped, or
  // the first event back the other way would immediately fire a step.
  const base = accumulated * delta < 0 ? 0 : accumulated;
  const next = base + delta;
  if (next >= threshold) return { accumulated: 0, step: 1 };
  if (next <= -threshold) return { accumulated: 0, step: -1 };
  return { accumulated: next, step: 0 };
}

export function clampZoom(zoom: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

/** Keeps a panned photo covering the viewport. Scaling by `zoom` overflows the viewport by
 *  `size * (zoom - 1)`, half of it on each side, which is exactly how far either edge can
 *  travel before it would pull into view. At fit the bound is zero, so this also pins the
 *  photo to the centre without a special case. */
export function clampPan(
  x: number,
  y: number,
  zoom: number,
  width: number,
  height: number,
): Pan {
  const maxX = (width * (zoom - 1)) / 2;
  const maxY = (height * (zoom - 1)) / 2;
  return {
    x: Math.min(maxX, Math.max(-maxX, x)),
    y: Math.min(maxY, Math.max(-maxY, y)),
  };
}
