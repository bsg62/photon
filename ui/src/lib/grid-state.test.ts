import { describe, expect, it } from 'vitest';
import { buildFailure, emptyLibrary, gridBuilt, noPhotosLine, photoCount, showEmptyNotice } from './grid-state';

// Literal versions, not `NOT_BUILT`: 0 is the backend's number (`engine::NOT_BUILT`), and a
// test written against the constant would follow it anywhere.
describe('grid state', () => {
  const read = { buildError: null };

  it('reads the store placeholder and the engine’s unbuilt index as not built', () => {
    expect(gridBuilt({ version: -1 })).toBe(false);
    expect(gridBuilt({ version: 0 })).toBe(false);
    expect(gridBuilt({ version: 1 })).toBe(true);
  });

  it('says nothing about an empty grid that has not been built', () => {
    expect(showEmptyNotice({ version: -1, len: 0, ...read })).toBe(false);
    expect(showEmptyNotice({ version: 0, len: 0, ...read })).toBe(false);
    expect(photoCount({ version: 0, len: 0, ...read })).toBeNull();
  });

  it('says a built grid is empty, and counts one that is not', () => {
    expect(showEmptyNotice({ version: 1, len: 0, ...read })).toBe(true);
    expect(showEmptyNotice({ version: 1, len: 3, ...read })).toBe(false);
    expect(photoCount({ version: 1, len: 0, ...read })).toBe('0 photos');
    expect(photoCount({ version: 7, len: 1234, ...read })).toBe(`${(1234).toLocaleString()} photos`);
    expect(buildFailure(read)).toBeNull();
  });

  // The backend publishes an empty grid when it could not read the library at startup, so
  // the window leaves the not-built state. "No photos yet. Add a folder to get started" and
  // "0 photos" said of that grid are false about a library full of photos.
  it('says the library could not be read, not that it is empty, when the first build failed', () => {
    const failed = { version: 1, len: 0, buildError: 'no such column: file_name' };
    expect(buildFailure(failed)).toBe('photon could not read the library: no such column: file_name');
    expect(showEmptyNotice(failed)).toBe(false);
    expect(photoCount(failed)).toBeNull();
  });
});

describe('what an empty library says', () => {
  it('offers to add a folder when none is watched', () => {
    expect(emptyLibrary(true, 0, false)).toBe('first-run');
  });

  // "No photos yet. Add a folder to get started" was said of a folder added a second ago,
  // while its first scan was still walking it.
  it('says it is looking while a watched folder is being scanned', () => {
    expect(emptyLibrary(true, 1, true)).toBe('scanning');
    expect(emptyLibrary(true, 3, true)).toBe('scanning');
  });

  it('says the watched folders hold no photos once no scan is running', () => {
    expect(emptyLibrary(true, 1, false)).toBe('no-photos');
  });

  // The grid and the folder list are fetched side by side, and the grid can land first:
  // with no list yet, "none is watched" is not known, and a user with folders was told to
  // add one for as long as the list took.
  it('says nothing until the watched folders have been read', () => {
    expect(emptyLibrary(false, 0, false)).toBeNull();
  });

  // On a first run the backend watches the Pictures folder by itself and scans it, and the
  // list the UI read at launch may be from before that: a scan is running in a folder the
  // list does not hold. That is no time to say "add a folder".
  it('says it is looking whenever a scan is running, whatever the list holds', () => {
    expect(emptyLibrary(true, 0, true)).toBe('scanning');
    expect(emptyLibrary(false, 0, true)).toBe('scanning');
  });
});

describe('what is said of watched folders with no photos', () => {
  const on = (path: string) => ({ path, online: true });
  const off = (path: string) => ({ path, online: false });

  it('names the one folder photon looked in', () => {
    expect(noPhotosLine([on('/home/ada/Pictures')])).toBe('photon looked in /home/ada/Pictures and found no photos or videos.');
  });

  it('counts several', () => {
    expect(noPhotosLine([on('/a'), on('/b'), on('/c')])).toBe('photon looked in the 3 folders it watches and found no photos or videos.');
  });

  // An unplugged drive has not been looked in: "found no photos" would be said of a folder
  // that may hold thousands.
  it('says a folder it cannot reach is that, not empty', () => {
    expect(noPhotosLine([off('/mnt/photos')])).toBe('photon cannot reach /mnt/photos right now.');
    expect(noPhotosLine([off('/a'), off('/b')])).toBe('photon cannot reach the 2 folders it watches right now.');
  });

  it('says both when it can reach only some', () => {
    expect(noPhotosLine([on('/a'), off('/b'), off('/c')])).toBe(
      'photon found no photos or videos in the folders it can reach; 2 of the 3 it watches cannot be reached right now.',
    );
  });
});
