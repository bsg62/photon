import type { AlbumSummary } from './api';

/** photon's own albums: the ones a photo can be added to or taken out of, and that can be
 *  renamed and deleted. Picasa's are mirrored from its INI, and the backend refuses every
 *  one of those writes on them. */
export function ownAlbums(albums: AlbumSummary[]): AlbumSummary[] {
  return albums.filter((a) => !a.picasa);
}

/** Whether `albumId` is one of photon's own albums in `albums`. Asked this way round, not as
 *  "is it Picasa's": a Picasa album that just lost its last photo has left the list, and an
 *  id that is not listed must not read as editable. */
export function isOwnAlbum(albums: AlbumSummary[], albumId: number | null): boolean {
  return albums.some((a) => a.id === albumId && !a.picasa);
}

/** The Picasa albums a photo is in, in the list's order, for the info panel's read-only rows. */
export function picasaAlbumsOf(albums: AlbumSummary[], memberOf: number[]): AlbumSummary[] {
  const member = new Set(memberOf);
  return albums.filter((a) => a.picasa && member.has(a.id));
}
