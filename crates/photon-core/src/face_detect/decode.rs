//! The arithmetic between the model's twelve output tensors and faces in input pixels.
//! Pure, so each rule has a test that fails without it.

/// A detection must score at least this. Measured 2026-10-01: at 0.7 all 29 faces of a
/// group photograph (0.89-0.94) and all 401 sampled LFW subjects are kept, and the three
/// false detections in 23 photos without a face (an ibex at 0.48, a flower and a woman seen
/// from behind at 0.61) are not. At 0.9 four of the 29 are lost.
pub(crate) const SCORE_THRESHOLD: f32 = 0.7;

/// Two boxes sharing more than this much of their union are one face. The model authors'
/// value.
pub(crate) const MAX_IOU: f32 = 0.3;

/// One face: in the model's input pixels as [`decode_level`] produces it, in fractions of
/// the image once [`Raw::in_units_of`] has divided it by the image's size at that input.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Raw {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub landmarks: [(f32, f32); 5],
    pub score: f32,
}

impl Raw {
    /// Whether every number in it is one. A face with a box, a landmark or a score that is
    /// not cannot be stored - SQLite binds NaN as NULL, which the table's `NOT NULL`
    /// refuses, and the refusal takes the whole batch with it, on every pass after - and
    /// in [`suppress`] its overlap with anything is NaN, which drops the face compared
    /// with it.
    pub(crate) fn is_finite(&self) -> bool {
        [self.x, self.y, self.w, self.h, self.score]
            .into_iter()
            .chain(self.landmarks.into_iter().flat_map(|(x, y)| [x, y]))
            .all(f32::is_finite)
    }

    /// The same face with every horizontal measure divided by `width` and every vertical
    /// one by `height`. The overlap of two boxes is a ratio of areas, so it is the same in
    /// these units as in pixels, which is what lets faces found at two input sizes be
    /// suppressed together.
    pub(crate) fn in_units_of(self, width: f32, height: f32) -> Self {
        Self {
            x: self.x / width,
            y: self.y / height,
            w: self.w / width,
            h: self.h / height,
            landmarks: self.landmarks.map(|(x, y)| (x / width, y / height)),
            score: self.score,
        }
    }
}

/// The four outputs for one stride, flattened: one value per cell for `cls` and `obj`,
/// four for `bbox`, ten for `kps`, cells in row order.
pub(crate) struct Level<'a> {
    pub stride: usize,
    pub cls: &'a [f32],
    pub obj: &'a [f32],
    pub bbox: &'a [f32],
    pub kps: &'a [f32],
}

/// Appends the faces of one level, for a square input `side` pixels wide.
pub(crate) fn decode_level(level: &Level<'_>, side: usize, threshold: f32, out: &mut Vec<Raw>) {
    let cols = side / level.stride;
    let stride = level.stride as f32;
    for n in 0..cols * cols {
        // Clamped first: a sigmoid that rounds past 1 must not carry a weak partner over
        // the threshold.
        let score = (level.cls[n].clamp(0.0, 1.0) * level.obj[n].clamp(0.0, 1.0)).sqrt();
        // Asked as "is it over", not "is it under": a score that is not a number is neither,
        // and asked the other way round it would be kept.
        let over = score >= threshold;
        if !over {
            continue;
        }
        let (row, col) = ((n / cols) as f32, (n % cols) as f32);
        let b = &level.bbox[n * 4..n * 4 + 4];
        let (cx, cy) = ((col + b[0]) * stride, (row + b[1]) * stride);
        let (w, h) = (b[2].exp() * stride, b[3].exp() * stride);
        let k = &level.kps[n * 10..n * 10 + 10];
        let mut landmarks = [(0.0, 0.0); 5];
        for (i, point) in landmarks.iter_mut().enumerate() {
            *point = ((col + k[2 * i]) * stride, (row + k[2 * i + 1]) * stride);
        }
        out.push(Raw {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
            landmarks,
            score,
        });
    }
}

fn iou(a: &Raw, b: &Raw) -> f32 {
    let w = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.0);
    let h = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.0);
    let both = w * h;
    both / (a.w * a.h + b.w * b.h - both)
}

/// Keeps the strongest of each set of boxes that overlap by more than `max_iou`.
pub(crate) fn suppress(mut faces: Vec<Raw>, max_iou: f32) -> Vec<Raw> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Raw> = Vec::new();
    for face in faces {
        if kept.iter().all(|k| iou(k, &face) <= max_iou) {
            kept.push(face);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One level of `cols` x `cols` cells, silent but for cell `n`.
    struct Cell {
        cls: Vec<f32>,
        obj: Vec<f32>,
        bbox: Vec<f32>,
        kps: Vec<f32>,
    }

    fn cell(cols: usize, n: usize, cls: f32, obj: f32, bbox: [f32; 4], kps: [f32; 10]) -> Cell {
        let cells = cols * cols;
        let mut c = Cell {
            cls: vec![0.0; cells],
            obj: vec![0.0; cells],
            bbox: vec![0.0; cells * 4],
            kps: vec![0.0; cells * 10],
        };
        c.cls[n] = cls;
        c.obj[n] = obj;
        c.bbox[n * 4..n * 4 + 4].copy_from_slice(&bbox);
        c.kps[n * 10..n * 10 + 10].copy_from_slice(&kps);
        c
    }

    fn decode(c: &Cell, stride: usize, side: usize) -> Vec<Raw> {
        let mut out = Vec::new();
        let level = Level {
            stride,
            cls: &c.cls,
            obj: &c.obj,
            bbox: &c.bbox,
            kps: &c.kps,
        };
        decode_level(&level, side, SCORE_THRESHOLD, &mut out);
        out
    }

    /// Side 64 at stride 16 is a 4x4 grid; cell 6 is row 1, column 2. Its centre is
    /// (2 + 0.5, 1 + 0.25) cells = (40, 20) px, its size exp(ln 2) = 2 cells = 32 px wide
    /// and exp(0) = 1 cell = 16 px high, so the box starts at (24, 12).
    #[test]
    fn a_cell_becomes_a_box_in_input_pixels() {
        let c = cell(4, 6, 0.81, 1.0, [0.5, 0.25, 2f32.ln(), 0.0], [0.0; 10]);
        let faces = decode(&c, 16, 64);
        assert_eq!(faces.len(), 1);
        let f = &faces[0];
        assert!((f.x - 24.0).abs() < 1e-3, "x {}", f.x);
        assert!((f.y - 12.0).abs() < 1e-3, "y {}", f.y);
        assert!((f.w - 32.0).abs() < 1e-3, "w {}", f.w);
        assert!((f.h - 16.0).abs() < 1e-3, "h {}", f.h);
    }

    /// The same outputs at stride 8 (an 8x8 grid, cell 6 = row 0, column 6) land elsewhere
    /// and half the size: the stride and the column count both come from the level.
    #[test]
    fn the_stride_scales_and_places_the_box() {
        let c = cell(8, 6, 0.81, 1.0, [0.5, 0.25, 2f32.ln(), 0.0], [0.0; 10]);
        let f = &decode(&c, 8, 64)[0];
        // centre ((6 + 0.5) * 8, (0 + 0.25) * 8) = (52, 2); 16 x 8.
        assert!((f.x - 44.0).abs() < 1e-3, "x {}", f.x);
        assert!((f.y - -2.0).abs() < 1e-3, "y {}", f.y);
        assert!((f.w - 16.0).abs() < 1e-3, "w {}", f.w);
        assert!((f.h - 8.0).abs() < 1e-3, "h {}", f.h);
    }

    /// A landmark is the cell plus its offset, times the stride: no `exp`, unlike the size.
    #[test]
    fn landmarks_are_offsets_from_the_cell() {
        let mut kps = [0.0; 10];
        kps[0] = 0.5; // first point: x
        kps[1] = -0.5; // first point: y
        kps[8] = 1.0; // fifth point: x
        kps[9] = 2.0; // fifth point: y
        let c = cell(4, 6, 0.81, 1.0, [0.0; 4], kps);
        let f = &decode(&c, 16, 64)[0];
        assert_eq!(f.landmarks[0], ((2.0 + 0.5) * 16.0, (1.0 - 0.5) * 16.0));
        assert_eq!(f.landmarks[4], ((2.0 + 1.0) * 16.0, (1.0 + 2.0) * 16.0));
    }

    /// The score is the square root of class times object: 0.81 x 0.64 gives 0.72, which
    /// passes 0.7, where the plain product (0.52) or either mean would not agree.
    #[test]
    fn the_score_is_the_geometric_mean() {
        let c = cell(4, 6, 0.81, 0.64, [0.0; 4], [0.0; 10]);
        let faces = decode(&c, 16, 64);
        assert_eq!(faces.len(), 1);
        assert!((faces[0].score - 0.72).abs() < 1e-3, "{}", faces[0].score);
    }

    #[test]
    fn a_score_under_the_threshold_is_dropped() {
        // sqrt(0.6 * 0.8) = 0.693
        let c = cell(4, 6, 0.6, 0.8, [0.0; 4], [0.0; 10]);
        assert!(decode(&c, 16, 64).is_empty());
    }

    /// The model's sigmoid outputs can leave 0..1 by a rounding error; a class of 1.3 must
    /// not lift a weak object score over the threshold.
    #[test]
    fn scores_are_clamped_before_they_are_multiplied() {
        // Unclamped: sqrt(1.3 * 0.4) = 0.721. Clamped: sqrt(1.0 * 0.4) = 0.632.
        let c = cell(4, 6, 1.3, 0.4, [0.0; 4], [0.0; 10]);
        assert!(decode(&c, 16, 64).is_empty());
    }

    /// A class score that is not a number makes a score that is not one, which is not
    /// "under the threshold" to a comparison. It is not a face.
    #[test]
    fn a_score_that_is_not_a_number_is_dropped() {
        let c = cell(4, 6, f32::NAN, 1.0, [0.0; 4], [0.0; 10]);
        assert!(decode(&c, 16, 64).is_empty());
    }

    /// Each of a face's numbers on its own: a box whose size overflowed `exp`, a landmark
    /// and a coordinate that are not numbers, a score that is not.
    #[test]
    fn a_face_with_a_number_that_is_not_finite_is_told_apart() {
        let good = raw(1.0, 2.0, 3.0, 4.0, 0.9);
        assert!(good.is_finite());
        for bad in [
            Raw {
                x: f32::NAN,
                ..good.clone()
            },
            Raw {
                y: f32::NEG_INFINITY,
                ..good.clone()
            },
            Raw {
                w: f32::INFINITY,
                ..good.clone()
            },
            Raw {
                h: f32::NAN,
                ..good.clone()
            },
            Raw {
                score: f32::NAN,
                ..good.clone()
            },
        ] {
            assert!(!bad.is_finite(), "{bad:?}");
        }
        let mut bad = good.clone();
        bad.landmarks[4].1 = f32::NAN;
        assert!(!bad.is_finite());
        bad = good;
        bad.landmarks[0].0 = f32::INFINITY;
        assert!(!bad.is_finite());
    }

    fn raw(x: f32, y: f32, w: f32, h: f32, score: f32) -> Raw {
        Raw {
            x,
            y,
            w,
            h,
            landmarks: [(0.0, 0.0); 5],
            score,
        }
    }

    /// Two boxes sharing more than 0.3 of their union are one face: the stronger stays.
    #[test]
    fn overlapping_boxes_keep_the_higher_score() {
        // 10x10 boxes offset by 2: intersection 80, union 120, 0.67.
        let kept = suppress(
            vec![
                raw(0.0, 0.0, 10.0, 10.0, 0.8),
                raw(2.0, 0.0, 10.0, 10.0, 0.9),
            ],
            MAX_IOU,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].score, 0.9);
    }

    /// Two faces side by side, their boxes overlapping only at the edges, are two faces.
    #[test]
    fn boxes_under_the_limit_are_both_kept() {
        // Offset by 8: intersection 20, union 180, 0.11.
        let kept = suppress(
            vec![
                raw(0.0, 0.0, 10.0, 10.0, 0.8),
                raw(8.0, 0.0, 10.0, 10.0, 0.9),
            ],
            MAX_IOU,
        );
        assert_eq!(kept.len(), 2);
    }
}
