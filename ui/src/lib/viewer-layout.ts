import type { Pan } from './nav';

/** The info panel's width, and how far it stands from the viewer's right edge: clear of the
 *  close button in the corner. The viewer sets both on the panel from here, so the room the
 *  photo is given and the panel it is given up to cannot come to disagree. */
export const INFO_WIDTH = 280;
export const INFO_RIGHT = 56;
/** The black left between the photo's area and the panel. */
export const INFO_GAP = 12;

/** How much of the viewer's right side the info panel takes from the photo while it is open.
 *  The photo is fitted beside the panel, not under it: under it, the panel covered a third
 *  of a landscape photo, and the faces it lists were among what it covered. */
export function infoRoom(open: boolean): number {
  return open ? INFO_WIDTH + INFO_RIGHT + INFO_GAP : 0;
}

/** The area the photo is fitted into, in a viewer of this size: all of it, less the info
 *  panel's room. What the zoom and the pan are measured in. */
export function photoArea(viewerWidth: number, viewerHeight: number, infoOpen: boolean): { width: number; height: number } {
  return { width: Math.max(0, viewerWidth - infoRoom(infoOpen)), height: Math.max(0, viewerHeight) };
}

/** A pointer's place measured from the middle of the photo's area, which is what the pan is
 *  measured from too (`zoomAt`). */
export function fromPhotoCentre(
  clientX: number,
  clientY: number,
  viewer: { left: number; top: number; width: number; height: number },
  infoOpen: boolean,
): Pan {
  const area = photoArea(viewer.width, viewer.height, infoOpen);
  return { x: clientX - (viewer.left + area.width / 2), y: clientY - (viewer.top + area.height / 2) };
}

/** Whether the viewer has a photo to step to that way from offset `current` of `len`: what
 *  enables its previous and next buttons. A slideshow (`wraps`) goes round at both ends. A
 *  photo that has left the view (`orphaned`) holds no offset of its own - the photo at
 *  `current` is its right-hand neighbour - so its next is that one. */
export function canStep(dir: 1 | -1, current: number, len: number, wraps: boolean, orphaned = false): boolean {
  if (wraps) return len > 1;
  if (dir < 0) return current > 0 && len > 0;
  return orphaned ? current < len : current < len - 1;
}
