//! Bringing a face to where the model expects it: the rotation, uniform scale and shift
//! that best take its five landmarks to the model's reference points, and the 112x112
//! picture sampled through it.

use image::RgbImage;

/// Where the model expects the eyes, the nose tip and the mouth corners, in its 112x112
/// input, as OpenCV's `FaceRecognizerSF` aligns them. The order is YuNet's: image-left eye
/// first.
pub(crate) const REFERENCE: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

const SIDE: usize = 112;

/// `u = a x - b y + tx`, `v = b x + a y + ty`: a rotation and uniform scale (`a`, `b`)
/// and a shift, from the picture's pixels to the model's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Similarity {
    pub a: f32,
    pub b: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Similarity {
    /// Only the tests move a point forward; the sampler goes the other way.
    #[cfg(test)]
    pub fn apply(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (
            self.a * x - self.b * y + self.tx,
            self.b * x + self.a * y + self.ty,
        )
    }

    pub fn invert(&self, (u, v): (f32, f32)) -> (f32, f32) {
        let det = self.a * self.a + self.b * self.b;
        let (u, v) = (u - self.tx, v - self.ty);
        (
            (self.a * u + self.b * v) / det,
            (-self.b * u + self.a * v) / det,
        )
    }
}

/// The least-squares similarity taking `points` to [`REFERENCE`], or `None` when the
/// points do not span anything (all in one place) or are not numbers.
pub(crate) fn fit(points: &[(f32, f32); 5]) -> Option<Similarity> {
    let mean = |p: &[(f32, f32); 5]| {
        let (sx, sy) = p
            .iter()
            .fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
        (sx / 5.0, sy / 5.0)
    };
    let (px, py) = mean(points);
    let (qx, qy) = mean(&REFERENCE);
    let (mut along, mut across, mut spread) = (0.0f32, 0.0f32, 0.0f32);
    for ((x, y), (u, v)) in points.iter().zip(REFERENCE.iter()) {
        let (x, y, u, v) = (x - px, y - py, u - qx, v - qy);
        along += x * u + y * v;
        across += x * v - y * u;
        spread += x * x + y * y;
    }
    if spread < 1e-6 {
        return None;
    }
    let (a, b) = (along / spread, across / spread);
    let fitted = Similarity {
        a,
        b,
        tx: qx - (a * px - b * py),
        ty: qy - (b * px + a * py),
    };
    // `NaN < 1e-6` is false, so a non-finite landmark gets this far; the sampler casts
    // what it reads through this to integers.
    let finite = [fitted.a, fitted.b, fitted.tx, fitted.ty]
        .iter()
        .all(|n| n.is_finite());
    finite.then_some(fitted)
}

/// The model's input: each of its 112x112 pixels looked up in `image` through the inverse
/// of `to_reference`, bilinearly, black outside the picture. Planar RGB, 0..255, as
/// OpenCV feeds the model (it swaps its BGR to RGB for this one).
pub(crate) fn sample(image: &RgbImage, to_reference: &Similarity) -> Vec<f32> {
    let (w, h) = (image.width() as i64, image.height() as i64);
    let plane = SIDE * SIDE;
    let mut out = vec![0f32; 3 * plane];
    let pixel = |x: i64, y: i64, c: usize| -> f32 {
        if x < 0 || y < 0 || x >= w || y >= h {
            0.0
        } else {
            image.get_pixel(x as u32, y as u32)[c] as f32
        }
    };
    for v in 0..SIDE {
        for u in 0..SIDE {
            let (x, y) = to_reference.invert((u as f32, v as f32));
            let (x0, y0) = (x.floor() as i64, y.floor() as i64);
            let (fx, fy) = (x - x0 as f32, y - y0 as f32);
            for c in 0..3 {
                out[c * plane + v * SIDE + u] = pixel(x0, y0, c) * (1.0 - fx) * (1.0 - fy)
                    + pixel(x0 + 1, y0, c) * fx * (1.0 - fy)
                    + pixel(x0, y0 + 1, c) * (1.0 - fx) * fy
                    + pixel(x0 + 1, y0 + 1, c) * fx * fy;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn landmarks_at_the_reference_need_no_transform() {
        let s = fit(&REFERENCE).unwrap();
        assert!((s.a - 1.0).abs() < 1e-5 && s.b.abs() < 1e-5, "{s:?}");
        assert!(s.tx.abs() < 1e-3 && s.ty.abs() < 1e-3, "{s:?}");
    }

    /// A face turned 30 degrees, made 2.5 times larger and moved is brought back: each of
    /// its landmarks lands on its reference point.
    #[test]
    fn a_turned_scaled_shifted_face_is_brought_back() {
        let (angle, k, dx, dy) = (30f32.to_radians(), 2.5f32, 400.0f32, 250.0f32);
        let moved = REFERENCE.map(|(x, y)| {
            (
                k * (x * angle.cos() - y * angle.sin()) + dx,
                k * (x * angle.sin() + y * angle.cos()) + dy,
            )
        });
        let s = fit(&moved).unwrap();
        for (p, q) in moved.iter().zip(REFERENCE.iter()) {
            assert!(
                close(s.apply(*p), *q),
                "{:?} -> {:?}, want {q:?}",
                p,
                s.apply(*p)
            );
        }
    }

    #[test]
    fn invert_undoes_apply() {
        let s = Similarity {
            a: 0.8,
            b: -0.3,
            tx: 12.0,
            ty: -7.0,
        };
        for p in [(0.0, 0.0), (55.5, 70.25), (-3.0, 400.0)] {
            assert!(close(s.invert(s.apply(p)), p));
        }
    }

    /// Five landmarks on one spot (a broken detection) have no transform.
    #[test]
    fn landmarks_on_one_spot_have_no_transform() {
        assert!(fit(&[(10.0, 10.0); 5]).is_none());
    }

    /// With no transform, sampling copies the picture: red then green then blue planes,
    /// each pixel where it was.
    #[test]
    fn the_identity_samples_the_picture_as_it_is() {
        let img = RgbImage::from_fn(112, 112, |x, y| Rgb([x as u8, y as u8, 7]));
        let planes = sample(
            &img,
            &Similarity {
                a: 1.0,
                b: 0.0,
                tx: 0.0,
                ty: 0.0,
            },
        );
        let plane = 112 * 112;
        let at = |c: usize, x: usize, y: usize| planes[c * plane + y * 112 + x];
        assert_eq!((at(0, 30, 5), at(1, 30, 5), at(2, 30, 5)), (30.0, 5.0, 7.0));
        assert_eq!((at(0, 111, 100), at(1, 111, 100)), (111.0, 100.0));
    }

    /// What falls outside the picture is black.
    #[test]
    fn outside_the_picture_is_black() {
        let img = RgbImage::from_pixel(50, 50, Rgb([200, 200, 200]));
        let planes = sample(
            &img,
            &Similarity {
                a: 1.0,
                b: 0.0,
                tx: 0.0,
                ty: 0.0,
            },
        );
        assert_eq!(planes[10 * 112 + 10], 200.0);
        assert_eq!(planes[100 * 112 + 100], 0.0);
    }

    /// A shift moves the picture the way the transform says: model pixel u reads picture
    /// pixel u - tx. Through `apply` instead of `invert` it would read u + tx.
    #[test]
    fn a_shift_reads_the_picture_where_the_transform_came_from() {
        let img = RgbImage::from_fn(200, 200, |x, y| Rgb([x as u8, y as u8, 0]));
        let planes = sample(
            &img,
            &Similarity {
                a: 1.0,
                b: 0.0,
                tx: 5.0,
                ty: 0.0,
            },
        );
        let at = |c: usize, x: usize, y: usize| planes[c * 112 * 112 + y * 112 + x];
        assert_eq!((at(0, 20, 9), at(1, 20, 9)), (15.0, 9.0));
    }

    /// Half a pixel between two neighbours is their mean, and a quarter is weighted
    /// towards the nearer one.
    #[test]
    fn a_fractional_shift_mixes_two_neighbours() {
        let img = RgbImage::from_fn(200, 200, |x, _| Rgb([(x * 10) as u8, 0, 0]));
        let at = |tx: f32| {
            let planes = sample(
                &img,
                &Similarity {
                    a: 1.0,
                    b: 0.0,
                    tx,
                    ty: 0.0,
                },
            );
            planes[20]
        };
        // u = 20 reads x = 19.5 (columns 19 and 20: 190 and 200).
        assert!((at(0.5) - 195.0).abs() < 1e-3, "{}", at(0.5));
        // u = 20 reads x = 19.75, three quarters of the way from 190 to 200.
        assert!((at(0.25) - 197.5).abs() < 1e-3, "{}", at(0.25));
    }

    /// Landmarks that are not numbers have no transform: NaN or infinity would reach the
    /// sampler's integer casts.
    #[test]
    fn landmarks_that_are_not_numbers_have_no_transform() {
        let mut p = REFERENCE;
        p[2] = (f32::NAN, 10.0);
        assert!(fit(&p).is_none());
        p[2] = (f32::INFINITY, 10.0);
        assert!(fit(&p).is_none());
    }
}
