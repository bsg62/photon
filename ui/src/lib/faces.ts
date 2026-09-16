/** Placing Picasa's face rectangles over a photo shown with `object-fit: contain`. */

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
