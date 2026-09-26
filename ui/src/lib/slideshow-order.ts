/** The slideshow is a photo slideshow: the next offset after `from` that is a photo, in
 *  the direction `dir` (1 forward, -1 back), wrapping, or null when the view holds none.
 *  `from` itself comes back when it is the only photo - the caller then stays, as the
 *  one-photo view always has.
 *
 *  Backward exists for the arrows and the wheel during a show: a step by hand that landed
 *  on a video would leave the show sitting on something it never plays. It wraps like the
 *  show's own advance does, so Left on the first photo goes round to the last one, as the
 *  timer would have come round to the first. */
export async function nextStill(
  from: number,
  len: number,
  kindAt: (i: number) => Promise<'image' | 'video' | undefined>,
  dir: 1 | -1 = 1,
): Promise<number | null> {
  for (let step = 1; step <= len; step++) {
    const i = (((from + dir * step) % len) + len) % len;
    if ((await kindAt(i)) === 'image') return i;
  }
  return null;
}
