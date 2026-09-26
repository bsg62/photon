import { describe, expect, it } from 'vitest';
import { nextStill } from './slideshow-order';

const kinds = (s: string) => async (i: number) => (s[i] === 'v' ? 'video' : 'image') as 'image' | 'video';

describe('nextStill', () => {
  it('skips videos and wraps', async () => {
    expect(await nextStill(0, 4, kinds('pvvp'))).toBe(3);
    expect(await nextStill(3, 4, kinds('pvvp'))).toBe(0);
  });
  it('comes back to itself when it is the only photo', async () => {
    expect(await nextStill(1, 3, kinds('vpv'))).toBe(1);
  });
  it('is null when there is no photo at all', async () => {
    expect(await nextStill(0, 3, kinds('vvv'))).toBeNull();
  });
  it('skips an offset whose kind is unknown, as one gone from a shrunk view reports', async () => {
    // Offset 1 answers `undefined`: neither `'image'` nor `'video'`, the way a photo that
    // left the view between the grid shrinking and this call reads.
    const kindAt = async (i: number) => (i === 1 ? undefined : i === 2 ? 'video' : 'image') as 'image' | 'video' | undefined;
    expect(await nextStill(0, 4, kindAt)).toBe(3);
  });
});
