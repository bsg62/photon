/** The slideshow is a photo slideshow: the next offset after `from` that is a photo,
 *  wrapping, or null when the view holds none. `from` itself comes back when it is the only
 *  photo - the caller then stays, as the one-photo view always has. */
export async function nextStill(
  from: number,
  len: number,
  kindAt: (i: number) => Promise<'image' | 'video' | undefined>,
): Promise<number | null> {
  for (let step = 1; step <= len; step++) {
    const i = (from + step) % len;
    if ((await kindAt(i)) === 'image') return i;
  }
  return null;
}
