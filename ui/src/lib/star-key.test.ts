import { describe, expect, it } from 'vitest';
import { decidingPhoto } from './star-key';

const a = { id: 1, starred: true };
const b = { id: 2, starred: false };
const c = { id: 3, starred: true };
const selected = (...ids: number[]) => (id: number) => ids.includes(id);

describe('decidingPhoto', () => {
  it('is the lead while the lead is selected', () => {
    expect(decidingPhoto(a, [b, c], selected(1, 2, 3))).toBe(a);
    expect(decidingPhoto(b, [a, c], selected(1, 2))).toBe(b);
  });

  it('is never a photo outside the selection', () => {
    // Click b, Ctrl+click a, Ctrl+click a again: b alone is selected and the lead is on a.
    // Read from a, the key unstarred an unstarred b for ever.
    expect(decidingPhoto(a, [a, b, c], selected(2))).toBe(b);
    // The stars the other way round: c alone, lead on b. Read from b, c could not be unstarred.
    expect(decidingPhoto(b, [a, b, c], selected(3))).toBe(c);
  });

  it('is the first selected photo on screen when the lead is not one', () => {
    expect(decidingPhoto(undefined, [a, b, c], selected(2, 3))).toBe(b);
    // A tile whose page has not arrived is passed over.
    expect(decidingPhoto(undefined, [undefined, c], selected(3))).toBe(c);
  });

  it('is nothing when no selected photo is at hand', () => {
    expect(decidingPhoto(a, [a, b], selected(9))).toBeUndefined();
    expect(decidingPhoto(undefined, [], selected(1))).toBeUndefined();
  });
});
