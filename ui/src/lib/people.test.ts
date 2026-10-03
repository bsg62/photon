import { describe, expect, it, vi } from 'vitest';
import { faceUrl, nameChoice, openFacePhoto, switchOffWarning } from './people';

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
  it('is the same person when renaming to their own name in another case', () => {
    expect(nameChoice('ANNA', people, 3)).toEqual({ kind: 'same' });
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
