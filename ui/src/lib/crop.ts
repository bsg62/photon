/** Geometry of the viewer's crop tool. Pure, like `nav.ts`: the component turns pointer
 *  events into calls here and draws what comes back.
 *
 *  A rectangle is in fractions of the picture the tool shows — the photo turned but not
 *  cropped — which is the frame `photon-core`'s `edit::Crop` is defined in. */

export interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** What is being dragged: an edge, a corner, or the whole rectangle. */
export type Handle = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw' | 'move';

export const HANDLES: Handle[] = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'];

export const FULL: Rect = { left: 0, top: 0, right: 1, bottom: 1 };

/** The backend's `CROP_UNIT`. */
const UNIT = 65535;

/** The smallest side a drag can produce, as a fraction. Comfortably above the backend's
 *  refusal threshold (~1%), so a rectangle the tool allows is never one the save rejects. */
export const MIN_SIDE = 0.03;

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);

/** The ratio presets. `ratio` is long side over short side; `fractionRatio` turns it to
 *  face the way the picture does, so "4:3" on a portrait photo offers 3:4. */
export const ASPECTS: { label: string; ratio: number | 'original' | null }[] = [
  { label: 'Free', ratio: null },
  { label: 'Original', ratio: 'original' },
  { label: '1:1', ratio: 1 },
  { label: '4:3', ratio: 4 / 3 },
  { label: '3:2', ratio: 3 / 2 },
  { label: '16:9', ratio: 16 / 9 },
];

/** The width-to-height ratio a locked rectangle must keep **in fractions**, or null for a
 *  free one. A rectangle's pixel shape is its fractional shape times the picture's own, so
 *  the picture's ratio is divided out: on any photo, "Original" is a square of fractions. */
export function fractionRatio(ratio: number | 'original' | null, imageAspect: number): number | null {
  if (ratio === null || !(imageAspect > 0)) return null;
  if (ratio === 'original') return 1;
  const pixels = imageAspect >= 1 ? ratio : 1 / ratio;
  return pixels / imageAspect;
}

/** `start` after dragging `handle` by (`dx`, `dy`), both fractions of the picture.
 *
 *  Always from the rectangle as it was at pointer-down, never from the last result:
 *  accumulating per-move deltas lets a rectangle that hit the edge drift, because the part
 *  of each move the clamp swallowed is gone for good.
 *
 *  `k` is `fractionRatio`'s answer. Locked, a corner follows whichever axis the pointer
 *  moved further along; an edge grows the other dimension about the rectangle's middle.
 *  When the picture's edge stops one dimension, it stops both. */
export function dragRect(start: Rect, handle: Handle, dx: number, dy: number, k: number | null): Rect {
  if (handle === 'move') {
    const w = start.right - start.left;
    const h = start.bottom - start.top;
    const left = clamp(start.left + dx, 0, 1 - w);
    const top = clamp(start.top + dy, 0, 1 - h);
    return { left, top, right: left + w, bottom: top + h };
  }
  const east = handle.includes('e');
  const west = handle.includes('w');
  const south = handle.includes('s');
  const north = handle.includes('n');
  if (k === null) {
    return {
      left: west ? clamp(start.left + dx, 0, start.right - MIN_SIDE) : start.left,
      right: east ? clamp(start.right + dx, start.left + MIN_SIDE, 1) : start.right,
      top: north ? clamp(start.top + dy, 0, start.bottom - MIN_SIDE) : start.top,
      bottom: south ? clamp(start.bottom + dy, start.top + MIN_SIDE, 1) : start.bottom,
    };
  }

  const minW = Math.max(MIN_SIDE, MIN_SIDE * k);
  if ((east || west) && (north || south)) {
    // A corner: the opposite corner stays put.
    const ax = east ? start.left : start.right;
    const ay = south ? start.top : start.bottom;
    const wantW = east ? start.right + dx - ax : ax - (start.left + dx);
    const wantH = south ? start.bottom + dy - ay : ay - (start.top + dy);
    const roomW = east ? 1 - ax : ax;
    const roomH = south ? 1 - ay : ay;
    const w = Math.min(Math.max(wantW, wantH * k, minW), roomW, roomH * k);
    const h = w / k;
    return {
      left: east ? ax : ax - w,
      right: east ? ax + w : ax,
      top: south ? ay : ay - h,
      bottom: south ? ay + h : ay,
    };
  }
  if (east || west) {
    const ax = east ? start.left : start.right;
    const cy = (start.top + start.bottom) / 2;
    const wantW = east ? start.right + dx - ax : ax - (start.left + dx);
    const w = Math.min(Math.max(wantW, minW), east ? 1 - ax : ax, 2 * Math.min(cy, 1 - cy) * k);
    const h = w / k;
    return { left: east ? ax : ax - w, right: east ? ax + w : ax, top: cy - h / 2, bottom: cy + h / 2 };
  }
  const ay = south ? start.top : start.bottom;
  const cx = (start.left + start.right) / 2;
  const wantH = south ? start.bottom + dy - ay : ay - (start.top + dy);
  const h = Math.min(Math.max(wantH, minW / k), south ? 1 - ay : ay, (2 * Math.min(cx, 1 - cx)) / k);
  const w = h * k;
  return { left: cx - w / 2, right: cx + w / 2, top: south ? ay : ay - h, bottom: south ? ay + h : ay };
}

/** The largest rectangle of ratio `k` inside `rect`, on the same centre: what choosing a
 *  preset does to the rectangle already drawn. */
export function fitAspect(rect: Rect, k: number | null): Rect {
  if (k === null) return rect;
  const w = rect.right - rect.left;
  const h = rect.bottom - rect.top;
  const cx = (rect.left + rect.right) / 2;
  const cy = (rect.top + rect.bottom) / 2;
  const fitW = Math.min(w, h * k);
  const fitH = fitW / k;
  return { left: cx - fitW / 2, right: cx + fitW / 2, top: cy - fitH / 2, bottom: cy + fitH / 2 };
}

/** `[left, top, right, bottom]` for `set_item_edit`, or null for the whole picture, which
 *  is how "no crop" is said. */
export function toWire(rect: Rect): [number, number, number, number] | null {
  const unit = (v: number) => Math.round(clamp(v, 0, 1) * UNIT);
  const wire: [number, number, number, number] = [unit(rect.left), unit(rect.top), unit(rect.right), unit(rect.bottom)];
  return wire[0] === 0 && wire[1] === 0 && wire[2] === UNIT && wire[3] === UNIT ? null : wire;
}

export function fromWire(crop: readonly number[] | null | undefined): Rect {
  if (!crop || crop.length !== 4) return FULL;
  return { left: crop[0] / UNIT, top: crop[1] / UNIT, right: crop[2] / UNIT, bottom: crop[3] / UNIT };
}
