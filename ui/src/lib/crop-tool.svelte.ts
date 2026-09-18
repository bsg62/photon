/** The crop tool's state, apart from the viewer so it can be tested without rendering:
 *  the rectangle being drawn, the ratio it is locked to, and the drag in progress. The
 *  geometry itself is `crop.ts`. */

import { ASPECTS, FULL, dragRect, fitAspect, fractionRatio, fromWire, toWire, type Handle, type Rect } from './crop';

export function createCropTool() {
  let active = $state(false);
  let rect = $state<Rect>(FULL);
  let aspect = $state(0);
  /** Width over height of the picture the rectangle is drawn on. */
  let imageAspect = 1;
  let drag: { handle: Handle; from: Rect; x: number; y: number } | null = null;

  const k = () => fractionRatio(ASPECTS[aspect].ratio, imageAspect);

  return {
    get active() {
      return active;
    },
    get rect() {
      return rect;
    },
    /** Index into `ASPECTS`. */
    get aspect() {
      return aspect;
    },

    /** Opens on the photo's current crop, so cropping again adjusts the rectangle rather
     *  than cropping what is left. Always unlocked: the stored crop has no memory of the
     *  preset it was drawn with, and locking would reshape it before the user touched it. */
    begin(crop: readonly number[] | null | undefined, width: number, height: number) {
      rect = fromWire(crop);
      aspect = 0;
      imageAspect = width > 0 && height > 0 ? width / height : 1;
      drag = null;
      active = true;
    },

    cancel() {
      active = false;
      drag = null;
    },

    /** Choosing a ratio reshapes the rectangle to it at once, inside what is drawn. */
    setAspect(index: number) {
      if (index < 0 || index >= ASPECTS.length) return;
      aspect = index;
      rect = fitAspect(rect, k());
    },

    /** Back to the whole picture, which saves as "no crop". */
    clear() {
      aspect = 0;
      rect = FULL;
    },

    startDrag(handle: Handle, x: number, y: number) {
      drag = { handle, from: rect, x, y };
    },

    /** `x`, `y` in the same pixels as `startDrag`; `width`, `height` the size the picture
     *  is drawn at. The rectangle is recomputed from where the drag began every time, so
     *  what the picture's edge swallows of one move is not lost to the next. */
    dragTo(x: number, y: number, width: number, height: number) {
      if (!drag || width <= 0 || height <= 0) return;
      rect = dragRect(drag.from, drag.handle, (x - drag.x) / width, (y - drag.y) / height, k());
    },

    endDrag() {
      drag = null;
    },

    /** The rectangle for `set_item_edit`; null is the whole picture. */
    wire() {
      return toWire(rect);
    },
  };
}

export type CropTool = ReturnType<typeof createCropTool>;
