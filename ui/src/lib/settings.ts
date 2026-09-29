import type { MemoryUsage, ScanProgressEvent, WatchedFolder } from './api';

export type SettingsSection = 'folders' | 'appearance' | 'tags' | 'slideshow' | 'duplicates' | 'about';

export type FolderStatus =
  | { kind: 'scanning'; label: string }
  | { kind: 'degraded'; label: string }
  | { kind: 'offline'; label: string }
  | { kind: 'online'; label: string };

/** One label for a watched root in Settings. A running scan says the most about what the
 *  user is waiting for, so it wins; a degraded watcher only matters while the drive is
 *  there to watch, so offline beats it. */
export function folderStatus(
  watched: WatchedFolder,
  scan: ScanProgressEvent | undefined,
  degraded: boolean,
): FolderStatus {
  if (scan && !scan.done) {
    return { kind: 'scanning', label: `Scanning… ${scan.filesSeen.toLocaleString()} files` };
  }
  if (!watched.online) return { kind: 'offline', label: 'Offline' };
  if (degraded) return { kind: 'degraded', label: 'Live updates limited' };
  return { kind: 'online', label: 'Online' };
}

export function photoCountLabel(count: number): string {
  return count === 1 ? '1 photo' : `${count.toLocaleString()} photos`;
}

/** How often the About section re-reads the figure while it is open. */
export const MEMORY_POLL_MS = 2000;

/** Whole megabytes below a gigabyte, GB with one decimal above; binary units, as `formatSize`. */
export function memoryAmount(bytes: number): string {
  if (bytes < 1_073_741_824) return `${Math.round(bytes / 1_048_576)} MB`;
  return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
}

/** What the figure covers. On macOS it is photon's process alone, and saying so is the point:
 *  the web view is often the larger part, and a bare number would read as the whole. */
export function memoryScope(usage: MemoryUsage): string {
  if (!usage.includesWebview) return "photon's own process; the web view's processes are not included on macOS";
  return `photon and its web view, ${usage.processes} processes`;
}
