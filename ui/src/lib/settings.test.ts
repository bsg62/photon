import { describe, expect, it } from 'vitest';
import type { ScanProgressEvent, WatchedFolder } from './api';
import { folderStatus, memoryAmount, memoryScope, photoCountLabel } from './settings';

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

describe('memory', () => {
  it.each([
    [512 * 1024 * 1024 - 1, '512 MB'],
    [1024 * 1024 * 1024 - 1, '1024 MB'],
    [1024 * 1024 * 1024, '1.0 GB'],
    [1.5 * 1024 * 1024 * 1024, '1.5 GB'],
  ])('%d bytes read as %s', (bytes, label) => {
    expect(memoryAmount(bytes)).toBe(label);
  });

  it('names the web view only when it is counted', () => {
    expect(memoryScope({ bytes: 1, processes: 4, includesWebview: true })).toBe('photon and its web view, 4 processes');
    expect(memoryScope({ bytes: 1, processes: 1, includesWebview: false })).toMatch(/not included on macOS/);
  });
});
