/** Placing face rectangles over a photo shown with `object-fit: contain`. */

import type { ItemFace } from './api';

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The rectangle the photo actually occupies inside a `contain`-fitted frame: scaled to
 *  touch the frame on its longer relative side and centred on the other. `imageWidth` and
 *  `imageHeight` are the displayed (oriented) dimensions, since the browser applies the
 *  EXIF orientation before fitting. Degenerate inputs give an empty box rather than NaN. */
export function containedBox(imageWidth: number, imageHeight: number, frameWidth: number, frameHeight: number): Box {
  if (imageWidth <= 0 || imageHeight <= 0 || frameWidth <= 0 || frameHeight <= 0) {
    return { left: 0, top: 0, width: 0, height: 0 };
  }
  const scale = Math.min(frameWidth / imageWidth, frameHeight / imageHeight);
  const width = imageWidth * scale;
  const height = imageHeight * scale;
  return { left: (frameWidth - width) / 2, top: (frameHeight - height) / 2, width, height };
}

/** A face's rectangle in frame pixels. Picasa records faces as fractions of the displayed
 *  image, so they map into the contained box directly. */
export function faceBox(face: Pick<ItemFace, 'left' | 'top' | 'right' | 'bottom'>, image: Box): Box {
  return {
    left: image.left + face.left * image.width,
    top: image.top + face.top * image.height,
    width: (face.right - face.left) * image.width,
    height: (face.bottom - face.top) * image.height,
  };
}

/** A pointer position in the face layer's own pixels. `rect` is the layer's bounding
 *  rectangle, which already carries the viewer's zoom and pan, so the mapping is one
 *  division and repeats none of that code; `frameW`/`frameH` are the layer's unscaled size.
 *  A layer with no size gives -1, -1, which no box contains. */
export function toLayer(
  clientX: number,
  clientY: number,
  rect: Box,
  frameW: number,
  frameH: number,
): { x: number; y: number } {
  if (rect.width <= 0 || rect.height <= 0) return { x: -1, y: -1 };
  return { x: ((clientX - rect.left) * frameW) / rect.width, y: ((clientY - rect.top) * frameH) / rect.height };
}

/** The face under a point: of the boxes containing it (edges included), the smallest, so a
 *  face inside another's rectangle is reachable; ties go to the first. -1 for none. */
export function faceAt(x: number, y: number, boxes: readonly Box[]): number {
  let best = -1;
  let area = Infinity;
  boxes.forEach((b, i) => {
    if (x < b.left || x > b.left + b.width || y < b.top || y > b.top + b.height) return;
    const a = b.width * b.height;
    if (a < area) {
      best = i;
      area = a;
    }
  });
  return best;
}

/** The info panel's line for the faces that have no name, or null when there are none. */
export function unnamedFacesLabel(count: number): string | null {
  if (count <= 0) return null;
  return count === 1 ? '1 face not named' : `${count.toLocaleString()} faces not named`;
}
