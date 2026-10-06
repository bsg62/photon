import type { GridView } from './api';
import { lastIndexAtOrBefore, type SectionLike } from './layout';

export type NavKey = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' | 'Home' | 'End';

/** Zoom runs from "fit the window" to 400%, the same ceiling Picasa used. */
export const MIN_ZOOM = 1;
export const MAX_ZOOM = 4;

/** How much wheel delta adds up to one photo. A mouse notch is ~100, so one notch is one
 *  photo; a trackpad's stream of small deltas is rate-limited instead of flying through
 *  the folder. */
export const WHEEL_THRESHOLD = 100;

/** `MouseEvent.button` numbers the mouse's buttons 0 left, 1 middle, 2 right, 3 back,
 *  4 forward. Only the back button closes the viewer: forward has nowhere to go, since
 *  photon keeps no history to walk, and claiming the button would silently swallow a press
 *  some mice send by accident. */
export const MOUSE_BACK_BUTTON = 3;

/** Whether a mouse button should close the viewer, mirroring Escape and Backspace. */
export function closesViewer(button: number): boolean {
  return button === MOUSE_BACK_BUTTON;
}

/** The part of a keydown target that decides who owns Ctrl/Cmd+A. Structural rather than
 *  `Element`, so the rule can be tested: vitest runs in node, where there is no DOM. */
export interface SelectAllTarget {
  tagName?: string;
  isContentEditable?: boolean;
}

/** Whether photon, rather than the webview, answers for a Ctrl/Cmd+A on this target.
 *
 *  Left to the webview, Ctrl+A runs its own select-all over the document and paints the
 *  whole application — sidebar, status bar, folder names — in selection highlight, the way
 *  a browser treats a page. photon is not a page, so every keystroke outside a text entry
 *  is the application's to answer, even when it answers by doing nothing: the grid's own
 *  handler covers the case where something *should* happen.
 *
 *  A text entry keeps it, because there Ctrl+A means "select this field's text" — the
 *  search box and Settings' rename fields would otherwise lose an editing key that every
 *  text field everywhere has. */
export function ownsSelectAll(target: SelectAllTarget | null): boolean {
  if (!target) return true;
  if (target.isContentEditable) return false;
  const tag = target.tagName?.toUpperCase();
  return tag !== 'INPUT' && tag !== 'TEXTAREA';
}

/** The keys that move a slider. */
const SLIDER_KEYS = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown'];

/** Whether a key pressed with the focus in a form field is the field's to answer, rather
 *  than the viewer's. A text field or a checkbox keeps every key. A slider keeps only the
 *  keys that move it: a click on the zoom slider leaves the focus there, and with every key
 *  the slider's, `0`, `+` and the star were dead until something else was clicked - at the
 *  very moment the zoom keys are most likely to be reached for. */
export function fieldOwnsKey(field: { type: string }, key: string): boolean {
  return field.type !== 'range' || SLIDER_KEYS.includes(key);
}

export interface FolderPosition {
  /** 1-based position within the section (a folder, or a day, month or year), or 0 when
   *  there is nothing to number. */
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
  return lastIndexAtOrBefore(sections, offset, (s) => s.offset);
}

/** The grid offset selected after pressing `key`. Vertical moves follow the on-screen
 *  rows, which restart at every section.
 *
 *  A null `offset` means nothing is selected yet — clicking the grid background clears the
 *  selection. Navigation starts from the first photo in that case, so each key still answers
 *  for itself: End reaches the last photo rather than jumping to the top of the library. */
export function move(offset: number | null, key: NavKey, sections: SectionLike[], columns: number): number {
  return moveFrom(offset ?? 0, key, sections, columns);
}

function moveFrom(offset: number, key: NavKey, sections: SectionLike[], columns: number): number {
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

/** Where a grid offset sits within its own section, which is the number the viewer's caption
 *  shows. Grouped by folder that is its folder - "3 / 40" is the third of forty in this
 *  folder, the number a person can check against their file manager; under a date grouping
 *  it is the day, month or year. A view with no headers (Recent, No grouping, a sort other
 *  than date) is one section, so it is counted whole: the photo's place among them all. */
export function positionInSection(sections: SectionLike[], offset: number): FolderPosition {
  if (sections.length === 0) return { index: 0, count: 0 };
  const s = sections[sectionIndexOf(sections, offset)];
  // Clamped, because the offset can outrun the sections: the viewer holds its own offset and
  // a rescan that shrinks the library does not move it, so the caption had until the next
  // keypress to say "31 / 12". `sectionIndexOf` clamps to the last section, leaving the
  // index to run past its count.
  return { index: Math.min(offset - s.offset + 1, s.count), count: s.count };
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

/** One press of `+` or `-` in the viewer: a quarter more, or back by the same factor, so
 *  as many presses out as in end where they began. */
export const ZOOM_KEY_STEP = 1.25;

/** The most one wheel event zooms by, as a delta: see `wheelZoomFactor`. */
export const WHEEL_ZOOM_CAP = 20;

/** What one wheel event with Ctrl held multiplies the zoom by.
 *
 *  A trackpad pinch arrives as a stream of such events, each a small delta the engine made
 *  from the fingers' own scale - `-100 * ln(scale)` in Chromium - so `exp(-delta / 100)`
 *  gives the photo back the pinch as it was made. A mouse notch is a delta of about 100
 *  by the same road, which read the same way would be nearly three times per click; capped
 *  at `WHEEL_ZOOM_CAP` it is a step of a fifth. Exponential, so a pinch out and the same
 *  pinch back in end where they began. A delta that is not a number changes nothing. */
export function wheelZoomFactor(deltaY: number): number {
  if (!Number.isFinite(deltaY)) return 1;
  return Math.exp(-Math.max(-WHEEL_ZOOM_CAP, Math.min(WHEEL_ZOOM_CAP, deltaY)) / 100);
}

/** What a double-click zooms to when the photo is already shown at its own size or larger:
 *  there are no more pixels to see, but "closer" is still what was asked for. */
export const DOUBLE_CLICK_ZOOM = 2;

/** The zoom a double-click on a fitted photo goes to: one pixel of the photo on one pixel
 *  of the screen, which is what a photo is checked for sharpness at. `fitted` is the photo's
 *  width as drawn at fit, in CSS pixels, and `dpr` how many screen pixels one of those is.
 *  Clamped like every zoom, so a large photo in a small window stops at `MAX_ZOOM`. A photo
 *  that fills its fitted box with no pixels to spare - a small one, or anything not yet
 *  measured - gets `DOUBLE_CLICK_ZOOM` instead: a double-click that changed nothing would
 *  read as broken. */
export function actualSizeZoom(imageWidth: number, fitted: number, dpr: number): number {
  if (!(imageWidth > 0) || !(fitted > 0) || !(dpr > 0)) return DOUBLE_CLICK_ZOOM;
  const actual = imageWidth / (fitted * dpr);
  return actual > 1.1 ? clampZoom(actual) : DOUBLE_CLICK_ZOOM;
}

/** Zooms to `next` keeping the photo under `point` where it is. `point` and the pan are both
 *  measured from the viewport's centre, which is the stage's transform origin: the stage
 *  draws a point `q` of the fitted photo at `pan + zoom * q`, so the pan that leaves `point`
 *  showing the same `q` at the new zoom is `point - (point - pan) * next / zoom`. The result
 *  is clamped like any pan, so zooming out from a corner slides the photo back into the
 *  window rather than leaving black beside it, and at fit it is the centre whatever the
 *  point. `{ x: 0, y: 0 }` zooms about the middle of the window. */
export function zoomAt(
  zoom: number,
  pan: Pan,
  next: number,
  point: Pan,
  width: number,
  height: number,
): { zoom: number; pan: Pan } {
  const to = clampZoom(next);
  const by = to / zoom;
  return {
    zoom: to,
    pan: clampPan(point.x - (point.x - pan.x) * by, point.y - (point.y - pan.y) * by, to, width, height),
  };
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
