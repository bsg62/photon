//! A face's crop for the People page: a square around the face, cut from the photo's cached
//! preview. The rectangle is widened by `WIDEN` a side, so a crop shows the head and not
//! only the eyes-to-chin box YuNet draws, made square on its longer side, and kept inside
//! the picture by moving it, then by shrinking it to the picture's shorter side.

use crate::face_detect::Rect;

/// The crop's side in pixels, as served: the page draws it at 48 CSS px, twice that for a
/// high-density screen.
pub const CROP_PX: u32 = 96;
/// How much of the face's width (and height) is added on each side.
pub const WIDEN: f64 = 0.30;

/// The square to cut, as (x, y, side) in pixels of a `width` × `height` picture, or `None`
/// for a picture with no pixels or a rectangle that is not numbers.
pub fn square(rect: &Rect, width: u32, height: u32) -> Option<(u32, u32, u32)> {
    let numbers = [rect.left, rect.top, rect.right, rect.bottom];
    if width == 0 || height == 0 || !numbers.iter().all(|n| n.is_finite()) {
        return None;
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let (fw, fh) = ((rect.right - rect.left) * w, (rect.bottom - rect.top) * h);
    let (cx, cy) = (
        (rect.left + rect.right) / 2.0 * w,
        (rect.top + rect.bottom) / 2.0 * h,
    );
    let side = (fw.max(fh) * (1.0 + 2.0 * WIDEN))
        .round()
        .clamp(1.0, w.min(h));
    let x = (cx - side / 2.0).round().clamp(0.0, w - side);
    let y = (cy - side / 2.0).round().clamp(0.0, h - side);
    // Every value is inside 0..=u32::MAX by the clamps above.
    Some((x as u32, y as u32, side as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// 1000 × 500, a face 200 × 100 px in the middle: widened to 320 × 160, square on 320,
    /// centred on the face.
    #[test]
    fn a_face_in_the_middle_is_widened_and_squared_on_its_longer_side() {
        assert_eq!(
            square(&rect(0.4, 0.4, 0.6, 0.6), 1000, 500),
            Some((340, 90, 320))
        );
    }

    /// At the left edge the square is moved in, not cut: the face stays whole, off centre.
    #[test]
    fn a_face_at_the_edge_is_moved_inside_the_picture() {
        assert_eq!(
            square(&rect(0.0, 0.4, 0.1, 0.6), 1000, 500),
            Some((0, 170, 160))
        );
    }

    /// The far edges too: a face at the right or the bottom is moved in by the upper bound,
    /// which the left and top edges never reach.
    #[test]
    fn a_face_at_the_far_edges_is_moved_inside_the_picture() {
        assert_eq!(
            square(&rect(0.9, 0.4, 1.0, 0.6), 1000, 500),
            Some((840, 170, 160))
        );
        assert_eq!(
            square(&rect(0.4, 0.9, 0.6, 1.0), 1000, 500),
            Some((340, 180, 320))
        );
    }

    /// Wider than the picture is tall: the square shrinks to the shorter side.
    #[test]
    fn a_face_larger_than_the_picture_is_cut_to_its_shorter_side() {
        assert_eq!(
            square(&rect(0.0, 0.0, 1.0, 1.0), 1000, 500),
            Some((250, 0, 500))
        );
    }

    #[test]
    fn nothing_to_cut_is_none() {
        assert_eq!(square(&rect(0.4, 0.4, 0.6, 0.6), 0, 500), None);
        assert_eq!(square(&rect(f64::NAN, 0.4, 0.6, 0.6), 1000, 500), None);
    }
}
