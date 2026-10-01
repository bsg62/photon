import { describe, expect, it } from 'vitest';
import { histogramPath, histogramPeak } from './histogram';

describe('histogramPeak', () => {
  it('is the tallest step between the ends, so clipping does not flatten the curve', () => {
    expect(histogramPeak([900, 10, 40, 20, 5])).toBe(40);
    expect(histogramPeak([5, 10, 40, 20, 900])).toBe(40);
  });

  it('falls back to the ends when nothing lies between them', () => {
    expect(histogramPeak([30, 0, 0, 0, 70])).toBe(70);
    expect(histogramPeak([0, 0, 0])).toBe(0);
    expect(histogramPeak([])).toBe(0);
  });
});

describe('histogramPath', () => {
  it('draws one flat-topped column per step, scaled to the peak', () => {
    // Peak 40: 10 is a quarter of the height, 20 half, 40 all of it.
    expect(histogramPath([0, 10, 40, 20, 0])).toBe('M0 100V100H1V75H2V0H3V50H4V100H5V100Z');
  });

  it('draws a clipped end full height rather than off the top', () => {
    expect(histogramPath([900, 10, 40, 20, 5])).toBe('M0 100V0H1V75H2V0H3V50H4V87.5H5V100Z');
  });

  it('has nothing to draw for an empty histogram', () => {
    expect(histogramPath([0, 0, 0, 0])).toBeNull();
    expect(histogramPath([])).toBeNull();
  });
});
