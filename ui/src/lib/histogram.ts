/** The info panel's histogram, as the outline to draw. Pure, so the scaling is pinned by a
 *  test rather than judged by eye.
 *
 *  The backend sends one count per brightness step, darkest first (`ViewerItem.histogram`). */

/** The height of the drawing's own coordinate space; the width is one unit per step. */
export const HISTOGRAM_HEIGHT = 100;

/** The count the tallest column stands for.
 *
 *  Not simply the largest count. The first and last steps hold everything that clipped - a
 *  blown sky, a black border - and one of them is often several times any step between.
 *  Scaled to that, the rest of the curve, which is what the histogram is looked at for,
 *  would lie flat along the bottom. So the scale is the tallest step *between* the ends, and
 *  an end taller than that is drawn full height: it still reads as "a lot clipped here". A
 *  picture with nothing between the ends (pure black and white) falls back to the ends. */
export function histogramPeak(bins: number[]): number {
  const inner = Math.max(0, ...bins.slice(1, -1));
  return inner > 0 ? inner : Math.max(0, ...bins);
}

/** An SVG path for the histogram as a filled outline, in a space `bins.length` wide and
 *  `HISTOGRAM_HEIGHT` high with the origin at the top left: one flat-topped column per
 *  step, so the drawing shows the counts and not a smoothing of them. Null when there is
 *  nothing to draw - no steps, or no pixel in any. */
export function histogramPath(bins: number[]): string | null {
  const peak = histogramPeak(bins);
  if (peak === 0) return null;
  const top = (count: number) => {
    const y = HISTOGRAM_HEIGHT * (1 - Math.min(count, peak) / peak);
    // Two decimals: a tenth of a pixel at the panel's size, and a path a test can read.
    return Math.round(y * 100) / 100;
  };
  let d = `M0 ${HISTOGRAM_HEIGHT}`;
  bins.forEach((count, i) => {
    d += `V${top(count)}H${i + 1}`;
  });
  return `${d}V${HISTOGRAM_HEIGHT}Z`;
}
