import { describe, expect, it } from 'vitest';
import type { AlbumSummary } from './api';
import { isOwnAlbum, ownAlbums, picasaAlbumsOf } from './albums';

const trip: AlbumSummary = { id: 1, name: 'Holiday', count: 2, picasa: false };
const holiday: AlbumSummary = { id: 2, name: 'Holiday', count: 5, picasa: true };
const wedding: AlbumSummary = { id: 3, name: 'Wedding', count: 9, picasa: true };
const all = [trip, holiday, wedding];

describe('ownAlbums', () => {
  it("keeps photon's albums only, even beside a Picasa album of the same name", () => {
    expect(ownAlbums(all)).toEqual([trip]);
  });
});

describe('isOwnAlbum', () => {
  it('is true only for a listed photon album', () => {
    expect(isOwnAlbum(all, 1)).toBe(true);
    expect(isOwnAlbum(all, 2)).toBe(false);
  });

  it('is false for an album no longer listed, and for no album', () => {
    // A Picasa album that lost its last photo leaves the list while it may still be open.
    expect(isOwnAlbum(all, 42)).toBe(false);
    expect(isOwnAlbum(all, null)).toBe(false);
  });
});

describe('picasaAlbumsOf', () => {
  it("lists the Picasa albums a photo is in, in the list's order, and none of photon's", () => {
    expect(picasaAlbumsOf(all, [3, 1, 2])).toEqual([holiday, wedding]);
    expect(picasaAlbumsOf(all, [1])).toEqual([]);
  });
});
