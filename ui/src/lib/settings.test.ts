import { describe, expect, it } from 'vitest';
import type { ScanProgressEvent, WatchedFolder } from './api';
import { folderStatus, photoCountLabel } from './settings';

const online: WatchedFolder = { id: 1, path: '/p', online: true };
const offline: WatchedFolder = { ...online, online: false };
const scan = (done: boolean): ScanProgressEvent => ({
  watchedId: 1,
  filesSeen: 12,
  added: 0,
  changed: 0,
  done,
  cancelled: false,
});

describe('folderStatus', () => {
  it.each([
    ['a running scan beats everything', offline, scan(false), true, 'scanning'],
    ['offline beats a degraded watcher', offline, undefined, true, 'offline'],
    ['degraded when online', online, undefined, true, 'degraded'],
    ['a finished scan is not scanning', online, scan(true), false, 'online'],
    ['online otherwise', online, undefined, false, 'online'],
  ] as const)('%s', (_, watched, s, degraded, kind) => {
    expect(folderStatus(watched, s, degraded).kind).toBe(kind);
  });

  it('shows how far a scan has got', () => {
    expect(folderStatus(online, scan(false), false).label).toBe('Scanning… 12 files');
  });
});

describe('photoCountLabel', () => {
  it('singular and plural', () => {
    expect(photoCountLabel(1)).toBe('1 photo');
    expect(photoCountLabel(0)).toBe('0 photos');
  });
});
