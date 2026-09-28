import { describe, expect, it } from 'vitest';
import { buildFailure, gridBuilt, photoCount, showEmptyNotice } from './grid-state';

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
