import { describe, expect, it } from 'vitest';
import { nextStill, shuffleStride } from './slideshow-order';

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

describe('nextStill with a stride', () => {
  it('steps by the stride, wrapping, and backward undoes forward', async () => {
    const all = kinds('pppppppppp');
    expect(await nextStill(0, 10, all, 1, 3)).toBe(3);
    expect(await nextStill(9, 10, all, 1, 3)).toBe(2);
    expect(await nextStill(2, 10, all, -1, 3)).toBe(9);
  });
  it('skips a video by taking the next step of the same walk, not its neighbour', async () => {
    // From 0 by 3: offset 3 is a video, so the answer is 6 - not 4, the video's neighbour.
    expect(await nextStill(0, 10, kinds('pppvpppppp'), 1, 3)).toBe(6);
    expect(await nextStill(6, 10, kinds('pppvpppppp'), -1, 3)).toBe(0);
  });
  it('visits every photo once before any comes round again', async () => {
    const len = 10;
    const seen: number[] = [];
    let at = 0;
    for (let n = 0; n < len; n++) {
      at = (await nextStill(at, len, kinds('pppppppppp'), 1, 3)) as number;
      seen.push(at);
    }
    expect([...seen].sort((a, b) => a - b)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(at).toBe(0);
  });
});

describe('shuffleStride', () => {
  const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));

  it('shares no factor with the length, whatever the length and the draw', () => {
    for (let len = 1; len <= 400; len++) {
      for (const r of [0, 0.25, 0.5, 0.75, 0.999]) {
        const stride = shuffleStride(len, () => r);
        expect(gcd(stride, len), `len ${len}, r ${r}`).toBe(1);
        expect(stride, `len ${len}, r ${r}`).toBeGreaterThanOrEqual(1);
        expect(stride, `len ${len}, r ${r}`).toBeLessThan(Math.max(len, 2));
      }
    }
  });

  it('is neither the view\'s own order nor its reverse, wherever another exists', () => {
    // 1, 2, 3, 4 and 6 have no step but the neighbour's that shares no factor with them.
    for (let len = 5; len <= 400; len++) {
      if (len === 6) continue;
      for (const r of [0, 0.5, 0.999]) {
        const stride = shuffleStride(len, () => r);
        expect(stride, `len ${len}, r ${r}`).toBeGreaterThan(1);
        expect(stride, `len ${len}, r ${r}`).toBeLessThan(len - 1);
      }
    }
    for (const len of [0, 1, 2, 3, 4, 6]) expect(shuffleStride(len, () => 0.5)).toBe(1);
  });

  it('is a golden-ratio share of the view, from one end or the other', () => {
    const share = (r: number[]) => {
      const draws = [...r];
      return shuffleStride(100_000, () => draws.shift() as number) / 100_000;
    };
    // The first draw places it within the window, the second picks the end.
    expect(share([0, 0])).toBeCloseTo(0.37, 3);
    expect(share([0.999, 0])).toBeCloseTo(0.395, 3);
    expect(share([0, 0.9])).toBeCloseTo(0.63, 3);
    expect(share([0.999, 0.9])).toBeCloseTo(0.605, 3);
  });

  it('keeps the slides after the next one apart too, not only neighbours', () => {
    // A stride of half the view passes its own first test - each slide is far from the
    // last - and then plays 0, 50, 100, 49, 99: every second slide is next door. Within
    // four steps no slide may come within a tenth of the view of where the walk started.
    // The small lengths are the ones where the *nearest* stride sharing no factor fails
    // this - for 20 photos it is 7, which comes back next door every third slide - so they
    // are what holds the choice among the nearest to the best-spreading one.
    for (const len of [20, 22, 26, 28, 50, 101, 365, 1000, 99_991]) {
      for (const r of [0, 0.5, 0.999]) {
        for (const end of [0, 0.9]) {
          const draws = [r, end];
          const stride = shuffleStride(len, () => draws.shift() as number);
          for (let k = 1; k <= 4; k++) {
            const at = (k * stride) % len;
            const apart = Math.min(at, len - at);
            expect(apart, `len ${len}, stride ${stride}, step ${k}`).toBeGreaterThanOrEqual(Math.floor(len / 10));
          }
        }
      }
    }
  });

  it('follows the draw, so two shows differ', () => {
    expect(shuffleStride(1000, () => 0.1)).not.toBe(shuffleStride(1000, () => 0.9));
  });

  it('keeps the stride in use when the view changes length and it still fits', () => {
    // 381 shares no factor with 1001, so a photo arriving mid-show does not restart the cycle.
    expect(shuffleStride(1001, () => 0.5, 381)).toBe(381);
    // It shares 3 with 1002, and is the view's reverse at 382: a new one is drawn.
    expect(shuffleStride(1002, () => 0.5, 381)).not.toBe(381);
    expect(shuffleStride(382, () => 0.5, 381)).not.toBe(381);
  });
});
