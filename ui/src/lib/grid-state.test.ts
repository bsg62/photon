import { describe, expect, it } from 'vitest';
import { gridBuilt, photoCount, showEmptyNotice } from './grid-state';

// Literal versions, not `NOT_BUILT`: 0 is the backend's number (`engine::NOT_BUILT`), and a
// test written against the constant would follow it anywhere.
describe('grid state', () => {
  it('reads the store placeholder and the engine’s unbuilt index as not built', () => {
    expect(gridBuilt({ version: -1 })).toBe(false);
    expect(gridBuilt({ version: 0 })).toBe(false);
    expect(gridBuilt({ version: 1 })).toBe(true);
  });

  it('says nothing about an empty grid that has not been built', () => {
    expect(showEmptyNotice({ version: -1, len: 0 })).toBe(false);
    expect(showEmptyNotice({ version: 0, len: 0 })).toBe(false);
    expect(photoCount({ version: 0, len: 0 })).toBeNull();
  });

  it('says a built grid is empty, and counts one that is not', () => {
    expect(showEmptyNotice({ version: 1, len: 0 })).toBe(true);
    expect(showEmptyNotice({ version: 1, len: 3 })).toBe(false);
    expect(photoCount({ version: 1, len: 0 })).toBe('0 photos');
    expect(photoCount({ version: 7, len: 1234 })).toBe(`${(1234).toLocaleString()} photos`);
  });
});
