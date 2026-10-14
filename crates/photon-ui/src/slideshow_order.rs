//! The order a slideshow plays a view in: `slideshow-order.ts`, with its tests' cases.
//!
//! The Svelte walk awaits each photo's kind, page by page; this window holds the whole
//! `GridIndex`, so a kind is a lookup and the walk is plain.

/// The slideshow is a photo slideshow: the next offset after `from` that is a photo, in
/// the direction `dir` (1 forward, -1 back), wrapping, or none when the view holds none.
/// `from` itself comes back when it is the only photo - the caller then stays, as the
/// one-photo view always has. `is_photo` answers false for an offset it does not know.
///
/// Backward exists for the arrows and the wheel during a show: a step by hand that landed
/// on a video would leave the show sitting on something it never plays. It wraps as the
/// show's own advance does.
///
/// `stride` is how far one step moves: 1 for the view's own order, a [`shuffle_stride`]
/// for a shuffled show. It must share no factor with `len`, which is what makes `len`
/// steps visit every offset once.
pub fn next_still(
    from: usize,
    len: usize,
    is_photo: impl Fn(usize) -> bool,
    dir: i64,
    stride: usize,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let len_i = len as i128;
    for step in 1..=len_i {
        let at = (from as i128 + i128::from(dir) * stride as i128 * step).rem_euclid(len_i);
        if is_photo(at as usize) {
            return Some(at as usize);
        }
    }
    None
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// How many usable strides around the wanted one are compared.
const NEAREST: usize = 6;
/// How many slides ahead a stride is judged on.
const SPREAD_STEPS: usize = 4;

/// The step a shuffled show takes through a view of `len` photos: each photo `stride`
/// offsets after the last, wrapping.
///
/// Not a shuffled list: an offset names a photo only against one grid, and a list would
/// need its own history for the Left key. A stride needs neither: the next photo follows
/// from the one on screen, the previous one is the same step backward, and sharing no
/// factor with `len` it shows every photo once before any comes round again.
///
/// **The stride is a golden-ratio share of the view**, 37-39.5% of its length (or the
/// same from the other end): the share furthest from a half or a third, which would play
/// two or three runs through the grid interleaved. `random` (0..1) picks within the
/// window and the end, so two shows differ.
///
/// `previous` is the stride the show was using before the view changed length: kept
/// when it still shares no factor with the new length, so a scan adding a photo mid-show
/// does not start the cycle again. A view with no such stride walks in order: one of
/// fewer than five photos, and one of six.
pub fn shuffle_stride(
    len: usize,
    mut random: impl FnMut() -> f64,
    previous: Option<usize>,
) -> usize {
    // Short of 1 and of `len - 1`, which are the view's own order forward and backward.
    let usable = |s: i64| s > 1 && s < len as i64 - 1 && gcd(s as usize, len) == 1;
    if let Some(previous) = previous
        && usable(previous as i64)
    {
        return previous;
    }
    let share = 0.37 + 0.025 * random();
    let share = if random() < 0.5 { share } else { 1.0 - share };
    // JavaScript's `Math.round`: halves go up.
    let wanted = (len as f64 * share + 0.5).floor() as i64;
    // Of the usable strides nearest `wanted`, the one that spreads best: in a small view
    // the nearest alone can be two thirds of it.
    let (mut best, mut best_spread, mut found) = (1, -1_i64, 0);
    for away in 0..len as i64 {
        if found >= NEAREST {
            break;
        }
        let candidates = if away == 0 {
            vec![wanted]
        } else {
            vec![wanted - away, wanted + away]
        };
        for s in candidates {
            if !usable(s) {
                continue;
            }
            found += 1;
            let spread = spread_of(s as usize, len) as i64;
            if spread > best_spread {
                (best, best_spread) = (s as usize, spread);
            }
        }
    }
    best
}

/// The closest any of the next few slides comes, round the view, to where the walk
/// stands: what a stride near a half or a third of the view scores badly on.
fn spread_of(stride: usize, len: usize) -> usize {
    (1..=SPREAD_STEPS).fold(len, |closest, k| {
        let at = (k * stride) % len;
        closest.min(at).min(len - at)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &'static str) -> impl Fn(usize) -> bool {
        move |i| s.as_bytes().get(i).is_some_and(|kind| *kind != b'v')
    }

    #[test]
    fn it_skips_videos_and_wraps() {
        assert_eq!(next_still(0, 4, kinds("pvvp"), 1, 1), Some(3));
        assert_eq!(next_still(3, 4, kinds("pvvp"), 1, 1), Some(0));
    }

    #[test]
    fn it_comes_back_to_itself_when_it_is_the_only_photo() {
        assert_eq!(next_still(1, 3, kinds("vpv"), 1, 1), Some(1));
    }

    #[test]
    fn it_is_none_when_there_is_no_photo_at_all() {
        assert_eq!(next_still(0, 3, kinds("vvv"), 1, 1), None);
        assert_eq!(next_still(0, 0, kinds(""), 1, 1), None);
    }

    #[test]
    fn it_skips_an_offset_whose_kind_is_not_known() {
        let known = |i: usize| i != 1 && i != 2;
        assert_eq!(next_still(0, 4, known, 1, 1), Some(3));
    }

    // Each case answers differently forward: the Svelte draft's first cases all agreed
    // with the forward answer, and passed with the direction ignored.
    #[test]
    fn it_goes_backward_skipping_videos_and_wrapping_round_the_start() {
        assert_eq!(next_still(1, 4, kinds("pppp"), -1, 1), Some(0));
        assert_eq!(next_still(3, 4, kinds("ppvp"), -1, 1), Some(1));
        assert_eq!(next_still(0, 4, kinds("pvpv"), -1, 1), Some(2));
    }

    #[test]
    fn it_comes_round_the_start_and_the_end_alike() {
        assert_eq!(next_still(3, 4, kinds("pvvp"), -1, 1), Some(0));
        assert_eq!(next_still(0, 4, kinds("pvvp"), -1, 1), Some(3));
        assert_eq!(next_still(0, 5, kinds("ppvvv"), -1, 1), Some(1));
    }

    #[test]
    fn backward_comes_back_to_itself_as_the_only_photo_and_is_none_with_none() {
        assert_eq!(next_still(1, 3, kinds("vpv"), -1, 1), Some(1));
        assert_eq!(next_still(2, 3, kinds("vvv"), -1, 1), None);
    }

    #[test]
    fn it_steps_by_the_stride_wrapping_and_backward_undoes_forward() {
        let all = kinds("pppppppppp");
        assert_eq!(next_still(0, 10, &all, 1, 3), Some(3));
        assert_eq!(next_still(9, 10, &all, 1, 3), Some(2));
        assert_eq!(next_still(2, 10, &all, -1, 3), Some(9));
    }

    #[test]
    fn a_video_is_skipped_by_the_next_step_of_the_same_walk() {
        // From 0 by 3: offset 3 is a video, so the answer is 6 - not 4, its neighbour.
        assert_eq!(next_still(0, 10, kinds("pppvpppppp"), 1, 3), Some(6));
        assert_eq!(next_still(6, 10, kinds("pppvpppppp"), -1, 3), Some(0));
    }

    #[test]
    fn a_stride_visits_every_photo_once_before_any_comes_round_again() {
        let mut seen = Vec::new();
        let mut at = 0;
        for _ in 0..10 {
            at = next_still(at, 10, kinds("pppppppppp"), 1, 3).unwrap();
            seen.push(at);
        }
        seen.sort_unstable();
        assert_eq!(seen, (0..10).collect::<Vec<_>>());
        assert_eq!(at, 0);
    }

    #[test]
    fn a_stride_shares_no_factor_with_the_length_whatever_the_length_and_the_draw() {
        for len in 1..=400 {
            for r in [0.0, 0.25, 0.5, 0.75, 0.999] {
                let stride = shuffle_stride(len, || r, None);
                assert_eq!(gcd(stride, len), 1, "len {len}, r {r}");
                assert!(stride >= 1 && stride < len.max(2), "len {len}, r {r}");
            }
        }
    }

    #[test]
    fn a_stride_is_neither_the_views_own_order_nor_its_reverse_wherever_another_exists() {
        // 1, 2, 3, 4 and 6 have no step but the neighbour's that shares no factor.
        for len in 5..=400 {
            if len == 6 {
                continue;
            }
            for r in [0.0, 0.5, 0.999] {
                let stride = shuffle_stride(len, || r, None);
                assert!(stride > 1 && stride < len - 1, "len {len}, r {r}");
            }
        }
        for len in [0, 1, 2, 3, 4, 6] {
            assert_eq!(shuffle_stride(len, || 0.5, None), 1);
        }
    }

    #[test]
    fn a_stride_is_a_golden_ratio_share_of_the_view_from_one_end_or_the_other() {
        let share = |draws: [f64; 2]| {
            let mut draws = draws.into_iter();
            shuffle_stride(100_000, || draws.next().unwrap(), None) as f64 / 100_000.0
        };
        // The first draw places it within the window, the second picks the end.
        assert!((share([0.0, 0.0]) - 0.37).abs() < 5e-4);
        assert!((share([0.999, 0.0]) - 0.395).abs() < 5e-4);
        assert!((share([0.0, 0.9]) - 0.63).abs() < 5e-4);
        assert!((share([0.999, 0.9]) - 0.605).abs() < 5e-4);
    }

    // A stride of half the view passes "each slide is far from the last" and then plays
    // 0, 50, 100, 49, 99. Within four steps no slide may come within a tenth of the view
    // of where the walk started; the small lengths are where the nearest usable stride
    // fails that, and what holds the choice among the nearest to the best-spreading one.
    #[test]
    fn a_stride_keeps_the_slides_after_the_next_one_apart_too() {
        for len in [20, 22, 26, 28, 50, 101, 365, 1000, 99_991] {
            for r in [0.0, 0.5, 0.999] {
                for end in [0.0, 0.9] {
                    let mut draws = [r, end].into_iter();
                    let stride = shuffle_stride(len, || draws.next().unwrap(), None);
                    for k in 1..=4 {
                        let at = (k * stride) % len;
                        let apart = at.min(len - at);
                        assert!(apart >= len / 10, "len {len}, stride {stride}, step {k}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_stride_follows_the_draw_so_two_shows_differ() {
        assert_ne!(
            shuffle_stride(1000, || 0.1, None),
            shuffle_stride(1000, || 0.9, None)
        );
    }

    #[test]
    fn the_stride_in_use_is_kept_when_the_view_changes_length_and_it_still_fits() {
        // 381 shares no factor with 1001, so a photo arriving mid-show does not restart
        // the cycle.
        assert_eq!(shuffle_stride(1001, || 0.5, Some(381)), 381);
        // It shares 3 with 1002, and is the view's reverse at 382: a new one is drawn.
        assert_ne!(shuffle_stride(1002, || 0.5, Some(381)), 381);
        assert_ne!(shuffle_stride(382, || 0.5, Some(381)), 381);
    }
}
