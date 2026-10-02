/** What the status bar shows for a running scan or face pass. Pure, so the wording and the arithmetic
 *  are pinned by a test. */

import type { FaceProgress, ScanProgressEvent, WatchedFolder } from './api';

export interface ScanStatus {
  watchedId: number;
  label: string;
  /** How far along the scan is, 0..1, or null when there is nothing to measure it against:
   *  a folder's first scan, which the bar shows as indeterminate. */
  fraction: number | null;
}

function lastSegment(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** A scan walks the disk and does not know its total ahead of time, so the count the
 *  folder had after its last scan stands in for one (`expected`). A rescan of an unchanged
 *  folder therefore runs to exactly 100%; one with new photos runs past it, which the
 *  bar clamps and the label shows honestly as "of ~N". Photos the scan has added or
 *  replaced are called out because they are the point of watching. */
export function scanStatus(
  watched: WatchedFolder,
  scan: ScanProgressEvent,
  expected: number | undefined,
): ScanStatus {
  const name = lastSegment(watched.path);
  const seen = scan.filesSeen.toLocaleString();
  const parts: string[] = [];
  let fraction: number | null = null;
  if (expected && expected > 0) {
    fraction = Math.min(1, scan.filesSeen / expected);
    parts.push(`${seen} of ~${expected.toLocaleString()} files (${Math.round(fraction * 100)}%)`);
  } else {
    parts.push(`${seen} files`);
  }
  const moved = scan.added + scan.changed;
  if (moved > 0) parts.push(`${moved.toLocaleString()} new or changed`);
  return { watchedId: watched.id, label: `Scanning ${name}… ${parts.join(', ')}`, fraction };
}

/** The status bar's line for a running face pass, or null when there is nothing to say:
 *  no pass, or a library with no photo to check. `total` counts photos whose thumbnail is
 *  not made yet, so the bar can wait short of its end while thumbnails are rendering. */
export function faceStatus(progress: FaceProgress | null): { label: string; fraction: number } | null {
  if (!progress || !progress.running || progress.total <= 0) return null;
  return {
    label: `Finding faces: ${progress.checked.toLocaleString()} of ${progress.total.toLocaleString()}`,
    fraction: Math.min(1, progress.checked / progress.total),
  };
}
