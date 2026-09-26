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
});
