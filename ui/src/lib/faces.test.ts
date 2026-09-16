import { describe, expect, it } from 'vitest';
import { containedBox, faceBox } from './faces';

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
