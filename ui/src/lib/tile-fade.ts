/** How soon after its `src` is set a thumbnail counts as already there: by the next frame
 *  or the one after at 60Hz. A load that quick is a cached thumbnail - the protocol answers
 *  one from disk in a few milliseconds - and it would have been on screen by the time a
 *  fade could be seen starting, so fading it in only makes it arrive later. */
export const INSTANT_LOAD_MS = 32;

/** Whether a tile's thumbnail fades in as it loads. `complete` is whether the image was
 *  already available when its `src` was set (the browser's own memory cache), `elapsed`
 *  how long after that the load arrived, and `moving` whether the grid is scrolling.
 *
 *  A thumbnail that truly arrives late - one the backend had to render - fades onto a still
 *  grid, so it does not snap onto a tile the user is looking at. Not onto a moving one: the
 *  grid's lead mounts tiles a few hundred milliseconds ahead of a scroll, a late load there
 *  lands before the tile is on screen or while it is sliding past, and a fade still running
 *  as it comes into view is exactly the blank tile the lead is there to prevent. */
export function fadesIn(complete: boolean, elapsed: number, moving: boolean): boolean {
  return !complete && !moving && elapsed > INSTANT_LOAD_MS;
}
