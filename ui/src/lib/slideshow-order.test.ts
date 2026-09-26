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
  // Each case here answers differently forward: the first draft's cases all happened to
  // agree with the forward answer, and passed with the direction ignored altogether.
  it('goes backward, skipping videos and wrapping round the start', async () => {
    expect(await nextStill(1, 4, kinds('pppp'), -1)).toBe(0);
    expect(await nextStill(3, 4, kinds('ppvp'), -1)).toBe(1);
    expect(await nextStill(0, 4, kinds('pvpv'), -1)).toBe(2);
  });
  it('comes round the start and the end alike', async () => {
    expect(await nextStill(3, 4, kinds('pvvp'), -1)).toBe(0);
    expect(await nextStill(0, 4, kinds('pvvp'), -1)).toBe(3);
    expect(await nextStill(0, 5, kinds('ppvvv'), -1)).toBe(1);
  });
  it('backward comes back to itself as the only photo, and is null with none', async () => {
    expect(await nextStill(1, 3, kinds('vpv'), -1)).toBe(1);
    expect(await nextStill(2, 3, kinds('vvv'), -1)).toBeNull();
  });
});
