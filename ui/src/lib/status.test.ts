import { describe, expect, it } from 'vitest';
import type { ScanProgressEvent, WatchedFolder } from './api';
import { faceStatus, scanStatus } from './status';

const watched: WatchedFolder = { id: 3, path: '/home/dh/Pictures', online: true };
const scan = (filesSeen: number, added = 0, changed = 0): ScanProgressEvent => ({
  watchedId: 3,
  filesSeen,
  added,
  changed,
  done: false,
  cancelled: false,
});

describe('scanStatus', () => {
  it('measures a rescan against the count the folder had last time', () => {
    expect(scanStatus(watched, scan(1250), 5000)).toEqual({
      watchedId: 3,
      label: 'Scanning Pictures… 1,250 of ~5,000 files (25%)',
      fraction: 0.25,
    });
  });

  it('is indeterminate on a first scan, when there is no count to measure against', () => {
    expect(scanStatus(watched, scan(1250), undefined)).toEqual({
      watchedId: 3,
      label: 'Scanning Pictures… 1,250 files',
      fraction: null,
    });
    expect(scanStatus(watched, scan(1250), 0).fraction).toBeNull();
  });

  it('clamps the bar when new photos push the scan past the old count, and says so', () => {
    const status = scanStatus(watched, scan(5200, 200), 5000);
    expect(status.fraction).toBe(1);
    expect(status.label).toBe('Scanning Pictures… 5,200 of ~5,000 files (100%), 200 new or changed');
  });

  it('counts added and changed photos together', () => {
    expect(scanStatus(watched, scan(10, 2, 3), undefined).label).toBe('Scanning Pictures… 10 files, 5 new or changed');
  });
});

describe('faceStatus', () => {
  it('is nothing when no pass is running', () => {
    expect(faceStatus(null)).toBeNull();
    expect(faceStatus({ checked: 5, total: 10, running: false })).toBeNull();
  });

  it('counts photos checked of all photos', () => {
    expect(faceStatus({ checked: 12400, total: 98000, running: true })).toEqual({
      label: `Finding faces: ${(12400).toLocaleString()} of ${(98000).toLocaleString()}`,
      fraction: 12400 / 98000,
    });
  });

  // An empty library, or a first event before anything is counted, must not draw a bar.
  it('never shows 0 of 0', () => {
    expect(faceStatus({ checked: 0, total: 0, running: true })).toBeNull();
  });

  it('does not run past the end', () => {
    expect(faceStatus({ checked: 11, total: 10, running: true })?.fraction).toBe(1);
  });
});
