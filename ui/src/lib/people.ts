/** Pure pieces of the People page: what a typed name will do, the switch-off warning, a
 *  face's crop URL, and opening a face's photo. */

import type { NamedItems, RemovedItems, Skipped } from './api';
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

/** "1 photo" / "12 photos". */
function photos(n: number): string {
  return n === 1 ? '1 photo' : `${n.toLocaleString()} photos`;
}

/** The file names a skipped list shows, then how many more there were. */
function listed(s: Skipped): string {
  const names = s.items.map((i) => i.fileName);
  const more = s.count - s.items.length;
  if (more > 0) names.push(`and ${more.toLocaleString()} more`);
  return names.join(', ');
}

/** The toast after naming a face from the viewer. */
export function namedFaceMessage(name: string): string {
  return `This is ${name}.`;
}

/** The toast after "Not Anna" in the viewer: the face leaves her, and in her view the photo
 *  leaves the grid while the viewer stays on it. */
export function takenOffMessage(name: string): string {
  return `${name} taken off this photo.`;
}

/** The toast after naming photos: what was done, then why the rest were not, each reason on
 *  its own, so a user who selected forty photos can tell which to look at. */
export function namedItemsMessage(r: NamedItems): string {
  const parts: string[] = [
    r.named > 0 ? `Added ${photos(r.named)} to ${r.name}.` : `Nothing was added to ${r.name}.`,
  ];
  const { several, rejected, already, none } = r;
  /** A kind the user settles in the viewer: how many, why, which, and what to do there. */
  const toOpen = (s: Skipped, why: string) => {
    if (s.count === 0) return;
    const one = s.count === 1;
    parts.push(
      `${s.count.toLocaleString()} ${one ? 'has' : 'have'} ${why}${s.items.length ? `: ${listed(s)}` : ''} \u2014 open ${one ? 'it' : 'them'} to choose the face.`,
    );
  };
  toOpen(several, 'more than one unnamed face');
  toOpen(rejected, `a face you said is not ${r.name}`);
  if (already.count > 0) {
    parts.push(`${already.count.toLocaleString()} ${already.count === 1 ? 'is' : 'are'} already ${r.name}'s.`);
  }
  // "Unnamed", not "no face photon found": a photo whose faces are all someone's already, or
  // all ignored, or all Picasa's under a name, has faces and none to name.
  if (none.count > 0) {
    parts.push(`${none.count.toLocaleString()} ${none.count === 1 ? 'has' : 'have'} no unnamed face.`);
  }
  return parts.join(' ');
}

/** The toast after taking photos from a person. A photo Picasa names them on stays theirs,
 *  since photon never writes Picasa's names. */
export function removedMessage(name: string, r: RemovedItems): string {
  const kept = r.keptByPicasa;
  if (kept === 0) {
    return r.removed > 0 ? `Removed ${photos(r.removed)} from ${name}.` : `Nothing was removed from ${name}.`;
  }
  if (r.removed === 0) {
    return kept === 1
      ? `1 stays with ${name}: Picasa names ${name} on it.`
      : `${kept.toLocaleString()} stay with ${name}: Picasa names ${name} on them.`;
  }
  return `Removed ${photos(r.removed)} from ${name}. ${kept.toLocaleString()} ${kept === 1 ? 'stays' : 'stay'}: Picasa names ${name} on ${kept === 1 ? 'it' : 'them'}.`;
}
