//! The info panel's histogram, as the heights to draw. `histogram.ts`, with its tests'
//! cases: the scaling is pinned here and not judged by eye.
//!
//! The engine counts the pixels at each brightness step, darkest first
//! (`ViewerItem::histogram`).

/// The count the tallest column stands for.
///
/// Not simply the largest count. The first and last steps hold everything that clipped -
/// a blown sky, a black border - and one of them is often several times any step between.
/// Scaled to that, the rest of the curve, which is what a histogram is looked at for,
/// would lie flat along the bottom. So the scale is the tallest step *between* the ends,
/// and an end taller than that is drawn full height: it still reads as "a lot clipped
/// here". A picture with nothing between the ends - pure black and white - falls back to
/// the ends.
pub fn histogram_peak(bins: &[u32]) -> u32 {
    let between = (bins.len() > 2)
        .then(|| bins[1..bins.len() - 1].iter().copied().max())
        .flatten()
        .unwrap_or(0);
    if between > 0 {
        between
    } else {
        bins.iter().copied().max().unwrap_or(0)
    }
}

/// How tall each step's column is, as a share of the drawing's height: one flat-topped
/// column a step, so that the drawing shows the counts and not a smoothing of them. None
/// when there is nothing to draw - no steps, or no pixel in any.
pub fn histogram_columns(bins: &[u32]) -> Option<Vec<f32>> {
    let peak = histogram_peak(bins);
    if peak == 0 {
        return None;
    }
    Some(
        (bins.iter())
            .map(|count| (*count).min(peak) as f32 / peak as f32)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peak_is_the_tallest_step_between_the_ends() {
        // So that clipping does not flatten the curve.
        assert_eq!(histogram_peak(&[900, 10, 40, 20, 5]), 40);
        assert_eq!(histogram_peak(&[5, 10, 40, 20, 900]), 40);
        // With nothing between them, the ends.
        assert_eq!(histogram_peak(&[30, 0, 0, 0, 70]), 70);
        assert_eq!(histogram_peak(&[0, 0, 0]), 0);
        assert_eq!(histogram_peak(&[]), 0);
        // Two steps are two ends, and one is one.
        assert_eq!(histogram_peak(&[3, 9]), 9);
        assert_eq!(histogram_peak(&[4]), 4);
    }

    #[test]
    fn each_step_is_a_column_scaled_to_the_peak() {
        // Peak 40: 10 is a quarter of the height, 20 half, 40 all of it.
        assert_eq!(
            histogram_columns(&[0, 10, 40, 20, 0]),
            Some(vec![0.0, 0.25, 1.0, 0.5, 0.0])
        );
        // A clipped end is full height, not off the top.
        assert_eq!(
            histogram_columns(&[900, 10, 40, 20, 5]),
            Some(vec![1.0, 0.25, 1.0, 0.5, 0.125])
        );
    }

    #[test]
    fn an_empty_histogram_has_nothing_to_draw() {
        assert_eq!(histogram_columns(&[0, 0, 0, 0]), None);
        assert_eq!(histogram_columns(&[]), None);
    }
}
