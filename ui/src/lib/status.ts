/** What the status bar shows for a running scan. Pure, so the wording and the arithmetic
 *  are pinned by a test. */

import type { ScanProgressEvent, WatchedFolder } from './api';

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
