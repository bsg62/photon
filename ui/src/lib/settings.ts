import type { ScanProgressEvent, WatchedFolder } from './api';

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
