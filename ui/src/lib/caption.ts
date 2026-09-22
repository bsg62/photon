/** The viewer's caption line, in Picasa's order: name, capture time, resolution, size and
 *  the position in parentheses. Pure, so the exact text is pinned by a test. */

import type { FolderPosition } from './nav';

export interface CaptionItem {
  fileName: string;
  /** Capture time in seconds; the camera's wall-clock time stored as if it were UTC. */
  takenAt: number;
  width: number;
  height: number;
  orientation: number;
  /** Bytes. */
  size: number;
}

const SEPARATOR = ' · ';

/** KB below a megabyte, MB with one decimal above. Binary units, matching what the file
 *  managers photon sits beside report. */
export function formatSize(bytes: number): string {
  if (bytes < 1_048_576) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

/** A capture time as photon shows it, wherever it shows one.
 *
 *  Rendered in UTC on purpose. `takenAt` is the camera's naive local time stored as UTC
 *  seconds (see metadata.rs), so formatting it through the machine's zone would shift the
 *  hour by that zone's offset — a photo taken at 12:30 would read 14:30 in Berlin. Shared
 *  rather than repeated, so a second surface showing a capture time cannot show a different
 *  one for the same photo.
 *
 *  `locale` is for tests; callers leave it undefined to get the user's own. */
export function formatTaken(takenAt: number, locale?: string): string {
  return new Date(takenAt * 1000).toLocaleString(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
    timeZone: 'UTC',
  });
}

/** A photo's dimensions as the person sees them, wherever photon prints a pair of them.
 *
 *  `viewer_item` reports `width`/`height` straight off the row when the edit is identity,
 *  which is almost every photo, so a portrait frame from a camera that writes EXIF
 *  orientation arrives as its stored 6000x4000 with `orientation: 6`. Only an *edited*
 *  photo is normalised by the backend, and that path reports `orientation: 1`, so this is a
 *  no-op there. Shared rather than repeated, because two surfaces printing different
 *  dimensions for the same photo is what happened when it was not.
 *
 *  5..8 are the four EXIF orientations that involve a quarter turn. */
export function orientedDims(width: number, height: number, orientation: number): [number, number] {
  const quarterTurn = orientation >= 5 && orientation <= 8;
  return quarterTurn ? [height, width] : [width, height];
}

/** `locale` is for tests; the viewer leaves it undefined to get the user's own. */
export function formatCaption(item: CaptionItem, position: FolderPosition, locale?: string): string {
  const when = formatTaken(item.takenAt, locale);
  const [w, h] = orientedDims(item.width, item.height, item.orientation);
  const parts = [item.fileName, when, `${w} × ${h}`, formatSize(item.size)];
  if (position.count) parts.push(`(${position.index} / ${position.count})`);
  return parts.join(SEPARATOR);
}
