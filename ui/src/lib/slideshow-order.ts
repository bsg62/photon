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
 *  **The stride is a golden-ratio share of the view**, 37-39.5% of its length (or the same
 *  from the other end, which is that walk backward). What matters is not only that one
 *  slide is far from the next but that the slides after it are far from both: a stride of
 *  half the view plays 0, 50, 100, 49, 99, 48 - two runs through the grid, interleaved -
 *  and a third of it plays three. The golden ratio is the share furthest from every such
 *  fraction, so the first handful of slides land in different parts of the view.
 *  `random` (0..1) picks within the window and the end, so two shows differ; a small view
 *  has few strides to pick from and may repeat one.
 *
 *  `previous` is the stride the show was using before the view changed length: kept when it
 *  still shares no factor with the new length, so a scan adding a photo mid-show does not
 *  start the cycle again. A view with no such stride walks in order: one of fewer than five
 *  photos, and one of six, where every step but the neighbour's shares a factor with the
 *  length. */
export function shuffleStride(len: number, random: () => number, previous?: number): number {
  // Short of 1 and of `len - 1`, which are the view's own order forward and backward.
  const usable = (s: number) => s > 1 && s < len - 1 && gcd(s, len) === 1;
  if (previous !== undefined && usable(previous)) return previous;
  const share = 0.37 + 0.025 * random();
  const wanted = Math.round(len * (random() < 0.5 ? share : 1 - share));
  // Of the usable strides nearest `wanted`, the one that spreads best. The nearest alone is
  // not enough in a small view, where the step to a stride sharing no factor is a large
  // share of the view: for 50 photos the nearest to 32 is 33, two thirds, and the show
  // would play three runs interleaved while 31 plays none.
  let best = 1;
  let bestSpread = -1;
  let found = 0;
  for (let away = 0; away < len && found < NEAREST; away++) {
    for (const s of away === 0 ? [wanted] : [wanted - away, wanted + away]) {
      if (!usable(s)) continue;
      found++;
      const spread = spreadOf(s, len);
      if (spread > bestSpread) [best, bestSpread] = [s, spread];
    }
  }
  return best;
}

/** How many usable strides around the wanted one are compared. */
const NEAREST = 6;
/** How many slides ahead a stride is judged on. */
const SPREAD_STEPS = 4;

/** The closest any of the next few slides comes, round the view, to where the walk stands:
 *  what a stride near a half or a third of the view scores badly on. */
function spreadOf(stride: number, len: number): number {
  let closest = len;
  for (let k = 1; k <= SPREAD_STEPS; k++) {
    const at = (k * stride) % len;
    closest = Math.min(closest, at, len - at);
  }
  return closest;
}
