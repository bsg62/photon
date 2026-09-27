import type { Action } from 'svelte/action';

export interface Point {
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

/** The gap kept between a menu and the window's edge. */
export const MENU_MARGIN = 4;

/** Where a context menu opened at the pointer `at` goes so that all of it is on screen.
 *
 *  It opens down and to the right of the pointer, as a native menu does, and flips to the
 *  other side on an axis where that would cross the window's edge - so a menu opened near the
 *  bottom grows upward from the pointer rather than sliding up underneath it. Only when the
 *  flipped side does not fit either is it pushed against the margin. A menu larger than the
 *  window is pinned to the top-left margin; its CSS `max-height` then scrolls it. */
export function placeMenu(at: Point, size: Size, viewport: Size, margin = MENU_MARGIN): Point {
  const axis = (from: number, extent: number, room: number) => {
    const start = from + extent + margin <= room ? from : from - extent;
    return Math.max(margin, Math.min(start, room - extent - margin));
  };
  return { x: axis(at.x, size.width, viewport.width), y: axis(at.y, size.height, viewport.height) };
}

/** Positions a `position: fixed` menu opened at `at` with `placeMenu`, and again whenever
 *  its size changes (the tile menu gains its copies line after it opens) or the window's
 *  does. It sets `left` and `top` itself, so the element must not also bind them.
 *
 *  It measures with the menu at the origin, not where it last was: a fixed box's
 *  shrink-to-fit width is capped by the room to its right, so measured near the right edge
 *  it wraps its labels, reports itself narrow and tall, and is placed by that wrong size. */
export const fitMenu: Action<HTMLElement, Point> = (node, at) => {
  let point = at;
  const place = () => {
    node.style.left = '0px';
    node.style.top = '0px';
    const { width, height } = node.getBoundingClientRect();
    const { x, y } = placeMenu(point, { width, height }, { width: window.innerWidth, height: window.innerHeight });
    node.style.left = `${x}px`;
    node.style.top = `${y}px`;
  };
  place();
  const observer = new ResizeObserver(place);
  observer.observe(node);
  window.addEventListener('resize', place);
  return {
    update(next: Point) {
      point = next;
      place();
    },
    destroy() {
      observer.disconnect();
      window.removeEventListener('resize', place);
    },
  };
};
