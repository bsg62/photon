/** The slideshow is a photo slideshow: the next offset after `from` that is a photo, in
 *  the direction `dir` (1 forward, -1 back), wrapping, or null when the view holds none.
 *  `from` itself comes back when it is the only photo - the caller then stays, as the
 *  one-photo view always has.
 *
 *  Backward exists for the arrows and the wheel during a show: a step by hand that landed
 *  on a video would leave the show sitting on something it never plays. It wraps like the
 *  show's own advance does, so Left on the first photo goes round to the last one, as the
 *  timer would have come round to the first.
 *
 *  `stride` is how far one step moves: 1 for the view's own order, a [`shuffleStride`] for
 *  a shuffled show. It must share no factor with `len`, which is what makes `len` steps
 *  visit every offset once - with a factor in common the walk would circle a fraction of
 *  the view and report no photo where the rest holds some. */
export async function nextStill(
  from: number,
  len: number,
  kindAt: (i: number) => Promise<'image' | 'video' | undefined>,
  dir: 1 | -1 = 1,
  stride = 1,
): Promise<number | null> {
  for (let step = 1; step <= len; step++) {
    const i = (((from + dir * stride * step) % len) + len) % len;
    if ((await kindAt(i)) === 'image') return i;
  }
  return null;
}

function gcd(a: number, b: number): number {
  while (b !== 0) [a, b] = [b, a % b];
  return a;
}

/** The step a shuffled show takes through a view of `len` photos: each photo `stride`
 *  offsets after the last, wrapping.
 *
 *  Not a shuffled list. A list of offsets would be wrong after the first rebuild - an offset
 *  names a photo only against one grid version - and would need its own history for the
 *  Left key. A stride needs neither: the next photo follows from the one on screen, the
 *  previous one is the same step backward, and sharing no factor with `len` it shows every
 *  photo once before any comes round again.
 *
 *  It is taken from the middle of the view, between 30% and 70% of its length, so
 *  neighbours in the show are far apart in the grid - a shuffle that played three frames of
 *  one burst in a row would not feel like one - and `random` (0..1) moves it, so two shows
 *  differ. A view with no such stride walks in order: one of fewer than five photos, and
 *  one of six, where every step but the neighbour's shares a factor with the length. */
export function shuffleStride(len: number, random: () => number): number {
  const wanted = Math.floor(len * (0.3 + 0.4 * random()));
  // The nearest stride to `wanted` that shares no factor with `len`, short of 1 and of
  // `len - 1`, which are the view's own order forward and backward.
  for (let away = 0; away < len; away++) {
    for (const s of [wanted + away, wanted - away]) {
      if (s > 1 && s < len - 1 && gcd(s, len) === 1) return s;
    }
  }
  return 1;
}
