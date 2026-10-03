import { describe, expect, it, vi } from 'vitest';
import type { NamedItems } from './api';
import {
  faceUrl,
  nameChoice,
  namedFaceMessage,
  namedItemsMessage,
  openFacePhoto,
  removedMessage,
  switchOffWarning,
  takenOffMessage,
} from './people';

const people = [
  { id: 3, name: 'Anna' },
  { id: 5, name: 'Émile' },
  { id: 9, name: null },
];

describe('nameChoice', () => {
  it('is empty for nothing but spaces', () => {
    expect(nameChoice('   ', people)).toEqual({ kind: 'empty' });
  });
  it('is a new person for a name nobody has, trimmed', () => {
    expect(nameChoice('  Ben ', people)).toEqual({ kind: 'new', name: 'Ben' });
  });
  it('merges into a person whose name matches without case', () => {
    expect(nameChoice('anna', people)).toEqual({ kind: 'merge', id: 3, name: 'Anna' });
  });
  it('compares case beyond ASCII, as the backend does in Rust', () => {
    expect(nameChoice('ÉMILE', people)).toEqual({ kind: 'merge', id: 5, name: 'Émile' });
  });
  it('is the same person when renaming to exactly their own name', () => {
    expect(nameChoice(' Anna ', people, 3)).toEqual({ kind: 'same' });
  });
  it('is a rename when only the case of their own name changes', () => {
    expect(nameChoice('ANNA', people, 3)).toEqual({ kind: 'new', name: 'ANNA' });
  });
  it('never matches an unnamed group', () => {
    expect(nameChoice('null', people)).toEqual({ kind: 'new', name: 'null' });
  });
});

describe('switchOffWarning', () => {
  it('counts the people', () => {
    expect(switchOffWarning(1)).toBe('This deletes 1 person you named and everything photon found.');
    expect(switchOffWarning(4)).toBe('This deletes 4 people you named and everything photon found.');
  });
});

describe('faceUrl', () => {
  it('names the face and its picture', () => {
    expect(faceUrl(7, '00ab', false)).toBe('photon://localhost/face/7/00ab');
    expect(faceUrl(7, '00ab', true)).toBe('http://photon.localhost/face/7/00ab');
  });
});

describe('openFacePhoto', () => {
  const deps = (offsets: (number | null)[]) => {
    const queue = [...offsets];
    return {
      offsetOf: vi.fn(async () => queue.shift() ?? null),
      cancelSearch: vi.fn(),
      showAll: vi.fn(async () => {}),
      open: vi.fn(),
      notify: vi.fn(),
    };
  };
  it('opens the photo where the grid already has it', async () => {
    const d = deps([4]);
    await openFacePhoto(11, d);
    expect(d.open).toHaveBeenCalledWith(4);
    expect(d.showAll).not.toHaveBeenCalled();
  });
  it('switches to All photos when the current view does not hold it', async () => {
    const d = deps([null, 9]);
    await openFacePhoto(11, d);
    expect(d.cancelSearch).toHaveBeenCalled();
    expect(d.showAll).toHaveBeenCalled();
    expect(d.open).toHaveBeenCalledWith(9);
  });
  it('says so when the photo is in no view', async () => {
    const d = deps([null, null]);
    await openFacePhoto(11, d);
    expect(d.open).not.toHaveBeenCalled();
    expect(d.notify).toHaveBeenCalled();
  });
});

describe('toast wording', () => {
  const zero = { items: [], count: 0 };
  const r = (over: Partial<NamedItems> = {}): NamedItems => ({
    person: 3,
    name: 'Anna',
    named: 5,
    already: zero,
    several: zero,
    none: zero,
    ...over,
  });
  const files = (...names: string[]) => names.map((fileName, i) => ({ id: i, fileName }));

  it('names a face', () => {
    expect(namedFaceMessage('Anna')).toBe('This is Anna.');
  });

  it('takes a person off a photo', () => {
    expect(takenOffMessage('Anna')).toBe('Anna taken off this photo.');
  });

  it('says what was added', () => {
    expect(namedItemsMessage(r())).toBe('Added 5 photos to Anna.');
    expect(namedItemsMessage(r({ named: 1 }))).toBe('Added 1 photo to Anna.');
  });

  it('lists the photos with several unnamed faces', () => {
    const several = { items: files('IMG_1.jpg', 'IMG_2.jpg'), count: 2 };
    expect(namedItemsMessage(r({ several }))).toBe(
      'Added 5 photos to Anna. 2 have more than one unnamed face: IMG_1.jpg, IMG_2.jpg \u2014 open them to choose the face.',
    );
  });

  it('says how many more than the listed ones there were', () => {
    const several = { items: files('a.jpg', 'b.jpg'), count: 5 };
    expect(namedItemsMessage(r({ several }))).toContain('5 have more than one unnamed face: a.jpg, b.jpg, and 3 more \u2014');
  });

  it('lists no names when none were kept', () => {
    expect(namedItemsMessage(r({ several: { items: [], count: 5 } }))).toBe(
      'Added 5 photos to Anna. 5 have more than one unnamed face \u2014 open them to choose the face.',
    );
  });

  it('says how many were already the person or had no face', () => {
    expect(namedItemsMessage(r({ already: { items: files('a'), count: 1 } }))).toBe(
      "Added 5 photos to Anna. 1 is already Anna's.",
    );
    expect(namedItemsMessage(r({ none: { items: files('a'), count: 1 } }))).toBe(
      'Added 5 photos to Anna. 1 has no face photon found.',
    );
  });

  it('says so when nothing was added, then why', () => {
    expect(namedItemsMessage(r({ named: 0, none: { items: files('a'), count: 1 } }))).toBe(
      'Nothing was added to Anna. 1 has no face photon found.',
    );
  });

  it('words taking photos from a person', () => {
    expect(removedMessage('Anna', { removed: 3, keptByPicasa: 0 })).toBe('Removed 3 photos from Anna.');
    expect(removedMessage('Anna', { removed: 1, keptByPicasa: 2 })).toBe(
      'Removed 1 photo from Anna. 2 stay: Picasa names Anna on them.',
    );
    expect(removedMessage('Anna', { removed: 0, keptByPicasa: 2 })).toBe(
      '2 stay with Anna: Picasa names Anna on them.',
    );
  });
});
