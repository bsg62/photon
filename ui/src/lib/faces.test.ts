import { describe, expect, it } from 'vitest';
import { containedBox, faceAt, faceBox, toLayer, unnamedFacesLabel } from './faces';

describe('containedBox', () => {
  it('fits a landscape photo to the frame width and centres it vertically', () => {
    expect(containedBox(4000, 3000, 800, 800)).toEqual({ left: 0, top: 100, width: 800, height: 600 });
  });

  it('fits a portrait photo to the frame height and centres it horizontally', () => {
    // The oriented dimensions are the caller's: a 4000×3000 file shot on its side is
    // handed in as 3000×4000, and so fits by height like any portrait photo.
    expect(containedBox(3000, 4000, 800, 800)).toEqual({ left: 100, top: 0, width: 600, height: 800 });
  });

  it('gives an empty box for degenerate input rather than NaN', () => {
    expect(containedBox(0, 0, 800, 600)).toEqual({ left: 0, top: 0, width: 0, height: 0 });
    expect(containedBox(4000, 3000, 0, 0)).toEqual({ left: 0, top: 0, width: 0, height: 0 });
  });
});

describe('faceBox', () => {
  it('maps Picasa fractions into the contained image, not the frame', () => {
    const image = { left: 0, top: 100, width: 800, height: 600 };
    expect(faceBox({ left: 0.25, top: 0.5, right: 0.5, bottom: 1 }, image)).toEqual({
      left: 200,
      top: 400,
      width: 200,
      height: 300,
    });
  });
});

describe('unnamedFacesLabel', () => {
  it('says nothing for none', () => {
    expect(unnamedFacesLabel(0)).toBeNull();
  });
  it('counts one and many', () => {
    expect(unnamedFacesLabel(1)).toBe('1 face not named');
    expect(unnamedFacesLabel(3)).toBe('3 faces not named');
    expect(unnamedFacesLabel(1200)).toBe(`${(1200).toLocaleString()} faces not named`);
  });
});

describe('toLayer', () => {
  it('maps a point through a scaled, offset rectangle', () => {
    expect(toLayer(500, 350, { left: 100, top: 50, width: 800, height: 600 }, 400, 300)).toEqual({ x: 200, y: 150 });
  });
  it('gives a point no box contains for a zero-sized rectangle', () => {
    expect(toLayer(5, 5, { left: 0, top: 0, width: 0, height: 0 }, 400, 300)).toEqual({ x: -1, y: -1 });
  });
});

describe('toLayer with one zero dimension', () => {
  it('is out of every box', () => {
    expect(toLayer(5, 5, { left: 0, top: 0, width: 0, height: 10 }, 4, 3)).toEqual({ x: -1, y: -1 });
    expect(toLayer(5, 5, { left: 0, top: 0, width: 10, height: 0 }, 4, 3)).toEqual({ x: -1, y: -1 });
  });
});

describe('faceAt', () => {
  const big = { left: 0, top: 0, width: 100, height: 100 };
  const small = { left: 40, top: 40, width: 20, height: 20 };
  it('picks the smallest box under the point', () => {
    expect(faceAt(50, 50, [big, small])).toBe(1);
    expect(faceAt(10, 10, [big, small])).toBe(0);
  });
  it('includes edges, breaks ties to the first and gives -1 for none', () => {
    expect(faceAt(100, 100, [big])).toBe(0);
    expect(faceAt(50, 50, [big, { ...big }])).toBe(0);
    expect(faceAt(150, 50, [big, small])).toBe(-1);
    expect(faceAt(40, 50, [small])).toBe(0);
    expect(faceAt(50, 40, [small])).toBe(0);
    expect(faceAt(39, 50, [small])).toBe(-1);
    expect(faceAt(50, 39, [small])).toBe(-1);
    expect(faceAt(61, 50, [small])).toBe(-1);
    expect(faceAt(50, 61, [small])).toBe(-1);
  });
});
