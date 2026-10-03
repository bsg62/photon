/** Pure pieces of the People page: what a typed name will do, the switch-off warning, a
 *  face's crop URL, and opening a face's photo. */

import { mediaUrl } from './url';

export type NameChoice =
  | { kind: 'empty' }
  | { kind: 'same' }
  | { kind: 'new'; name: string }
  | { kind: 'merge'; id: number; name: string };

/** What committing `typed` will do, by the backend's own rule (`library/people.rs`,
 *  `clean` and `same_name`): trimmed, compared without case - `toLowerCase` is Unicode's
 *  default mapping, as Rust's `to_lowercase` is, so "ÉMILE" is Émile on both sides. `self`
 *  is the person being renamed, whom their own name does not merge into. Typing their own
 *  name exactly is `same` (nothing to do); only its capitalisation changing is a rename,
 *  which the backend allows, so "anna" -> "Anna" is how a name's case is corrected. */
export function nameChoice(
  typed: string,
  people: readonly { id: number; name: string | null }[],
  self?: number,
): NameChoice {
  const name = typed.trim();
  if (!name) return { kind: 'empty' };
  const key = name.toLowerCase();
  const match = people.find((p) => p.name !== null && p.name.toLowerCase() === key);
  if (!match) return { kind: 'new', name };
  if (match.id === self) return match.name === name ? { kind: 'same' } : { kind: 'new', name };
  return { kind: 'merge', id: match.id, name: match.name as string };
}

/** The confirmation before switching "Find faces" off deletes the names. */
export function switchOffWarning(named: number): string {
  return `This deletes ${named} ${named === 1 ? 'person' : 'people'} you named and everything photon found.`;
}

/** A face's crop, served by `protocol.rs` from the photo's cached preview. */
export function faceUrl(faceId: number, thumbKey: string, windows?: boolean): string {
  return mediaUrl(`face/${faceId}/${thumbKey}`, windows);
}

export interface OpenFaceDeps {
  offsetOf(itemId: number): Promise<number | null>;
  cancelSearch(): void;
  showAll(): Promise<void>;
  open(offset: number): void;
  notify(message: string): void;
}

/** Opens a face's photo in the viewer: where the grid already holds it, or in All photos.
 *  The viewer addresses a photo by grid offset, so the photo has to be in the grid's view;
 *  the People page shows no hidden photo's face, so All holds every one it can show unless
 *  the photo has gone since the page was read. */
export async function openFacePhoto(itemId: number, deps: OpenFaceDeps): Promise<void> {
  let at = await deps.offsetOf(itemId);
  if (at === null) {
    deps.cancelSearch();
    await deps.showAll();
    at = await deps.offsetOf(itemId);
  }
  if (at === null) {
    deps.notify('That photo is no longer in the library.');
    return;
  }
  deps.open(at);
}
