//! Finding faces in a picture. Pixels in, rectangles out: this module knows nothing about
//! the library, the thumbnail cache or the engine, and nothing outside it names `tract`.

use crate::{Error, Result};
use decode::{Level, MAX_IOU, Raw, SCORE_THRESHOLD};
use image::DynamicImage;
use serde::Serialize;
use tract_onnx::prelude::*;

mod decode;
pub mod merge;
pub mod pass;

/// YuNet, from OpenCV's model zoo (`models/README.md`).
static MODEL: &[u8] = include_bytes!("../../models/face_detection_yunet_2023mar.onnx");

/// The side of the square the model is run at to find small faces. A face narrower than
/// about 15 px at this size is missed: at 640 a group photo's faces are 14-21 px wide, on
/// the edge, and at 320 the 29 people of the spike's test photograph came out as one.
pub const INPUT: usize = 1280;

/// The side of a second run, for the face [`INPUT`] is too large for: the model stops
/// finding a face once it is big enough in pixels. Measured 2026-10-02 on crops of the
/// test portrait, by the face's height at a 1280 input: 480-590 px is found at 0.90-0.93,
/// 690 px at 0.82, 720-830 px at 0.76-0.78 with the box drawn too small, and 880-910 px
/// is not found at all - a head shot, a selfie. The same crops at 320 or 640 are found at
/// 0.89-0.95. 320 rather than 640 because it costs a sixteenth of the large run where 640
/// costs a quarter, and the faces it is for are still hundreds of pixels tall at it.
pub const CLOSE_UP_INPUT: usize = 320;

/// Which detector looked at a photo: `items.face_version` records it. Bump it when the
/// model file, [`INPUT`] or [`CLOSE_UP_INPUT`], or the threshold or overlap limit in
/// `decode` changes; every
/// photo is then detected again, as `EXIF_VERSION` re-reads metadata.
pub const DETECTOR_VERSION: i64 = 1;

/// A rectangle as fractions of a picture, 0..1 from its left and top.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// One face, in fractions of the image it was found in.
#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    pub rect: Rect,
    /// The model's five points, in its order: the two eyes (the image-left one first), the
    /// nose tip, the two mouth corners (image-left first).
    pub landmarks: [(f32, f32); 5],
    pub score: f32,
}

/// The strides of the model's three output levels, in output order.
const STRIDES: [usize; 3] = [8, 16, 32];

/// `decode_level` indexes by the grid, so every tensor must be as long as the grid implies;
/// a model that answers otherwise is an error, not a panic.
fn check_lengths(level: &Level<'_>, side: usize) -> Result<()> {
    let cells = (side / level.stride).pow(2);
    for (name, got, want) in [
        ("class", level.cls.len(), cells),
        ("object", level.obj.len(), cells),
        ("box", level.bbox.len(), cells * 4),
        ("landmarks", level.kps.len(), cells * 10),
    ] {
        if got != want {
            return Err(model_error(format!(
                "{name} output at stride {} has {got} values, expected {want}",
                level.stride
            )));
        }
    }
    Ok(())
}

type Run = dyn Fn(Tensor) -> TractResult<TVec<TValue>> + Send + Sync;

/// The model optimised for one input size.
struct Plan {
    side: usize,
    run: Box<Run>,
}

/// The loaded model, once for each of the two sizes it is run at. Parsing and optimising
/// them takes about 50 ms, so a pass makes one and shares it between its workers.
pub struct Detector {
    plans: [Plan; 2],
}

fn model_error(err: impl std::fmt::Display) -> Error {
    Error::FaceModel(err.to_string())
}

impl Plan {
    fn new(side: usize) -> Result<Self> {
        // The file declares a 640 input and the shapes that follow from it; left in, tract
        // refuses any other size ("Impossible to unify 320 with 160").
        let model = tract_onnx::onnx()
            .with_ignore_output_shapes(true)
            .with_ignore_value_info(true)
            .model_for_read(&mut std::io::Cursor::new(MODEL))
            .map_err(model_error)?
            .with_input_fact(0, f32::fact([1, 3, side, side]).into())
            .map_err(model_error)?
            .into_optimized()
            .map_err(model_error)?
            .into_runnable()
            .map_err(model_error)?;
        Ok(Self {
            side,
            run: Box::new(move |input| model.run(tvec!(input.into()))),
        })
    }

    /// The faces this size finds in `image`, over the threshold and not yet suppressed, in
    /// fractions of the image and unclamped: the two runs fit the image to different
    /// sizes, and a fraction of the image is the one unit both can be compared in.
    fn detect(&self, image: &DynamicImage, out: &mut Vec<Raw>) -> Result<()> {
        let side = self.side;
        // Scaled down to fit and never up: a face is found by its size in pixels, and
        // enlarging a small picture only invents them.
        let fitted = crate::decode::shrink_within(image, side as u32).to_rgb8();
        // Top left of a black square, as the model was trained: BGR, 0..255, planar.
        let plane = side * side;
        let mut input = vec![0f32; 3 * plane];
        for (x, y, px) in fitted.enumerate_pixels() {
            let at = y as usize * side + x as usize;
            input[at] = px[2] as f32;
            input[plane + at] = px[1] as f32;
            input[2 * plane + at] = px[0] as f32;
        }
        let tensor: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, side, side), input)
            .map_err(model_error)?
            .into();
        let found = (self.run)(tensor).map_err(model_error)?;
        // Twelve outputs: class, object, box and landmarks, each at the three strides.
        if found.len() != 12 {
            return Err(model_error(format!("{} outputs, expected 12", found.len())));
        }
        let flat = |i: usize| -> Result<Vec<f32>> {
            Ok(found[i]
                .to_plain_array_view::<f32>()
                .map_err(model_error)?
                .iter()
                .copied()
                .collect())
        };
        let mut raw: Vec<Raw> = Vec::new();
        for (i, stride) in STRIDES.into_iter().enumerate() {
            let (cls, obj, bbox, kps) = (flat(i)?, flat(3 + i)?, flat(6 + i)?, flat(9 + i)?);
            let level = Level {
                stride,
                cls: &cls,
                obj: &obj,
                bbox: &bbox,
                kps: &kps,
            };
            check_lengths(&level, side)?;
            decode::decode_level(&level, side, SCORE_THRESHOLD, &mut raw);
        }
        let (w, h) = (fitted.width() as f32, fitted.height() as f32);
        out.extend(raw.into_iter().map(|f| f.in_units_of(w, h)));
        Ok(())
    }
}

impl Detector {
    pub fn new() -> Result<Self> {
        Ok(Self {
            plans: [Plan::new(INPUT)?, Plan::new(CLOSE_UP_INPUT)?],
        })
    }

    /// The faces in `image`, strongest first.
    pub fn detect(&self, image: &DynamicImage) -> Result<Vec<Detection>> {
        let mut raw: Vec<Raw> = Vec::new();
        for plan in &self.plans {
            plan.detect(image, &mut raw)?;
        }
        // Before anything compares or stores them: one such face would cost its neighbours
        // in the suppression and its whole batch at the write.
        raw.retain(Raw::is_finite);
        // One suppression over both runs' faces, not one each: a face both sizes find is
        // drawn slightly differently by each, and must come out once.
        let unit = |v: f32| v.clamp(0.0, 1.0);
        Ok(decode::suppress(raw, MAX_IOU)
            .into_iter()
            .map(|f| Detection {
                rect: Rect {
                    left: unit(f.x) as f64,
                    top: unit(f.y) as f64,
                    right: unit(f.x + f.w) as f64,
                    bottom: unit(f.y + f.h) as f64,
                },
                landmarks: f.landmarks.map(|(x, y)| (unit(x), unit(y))),
                score: f.score,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use image::{DynamicImage, GenericImageView};
    use std::path::Path;

    fn fixture(rel: &str) -> DynamicImage {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        image::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn portrait() -> DynamicImage {
        fixture("testdata/faces/portrait.jpg")
    }

    /// The fixture's one face, where `testdata/faces/README.md` records it.
    fn assert_the_portraits_face(faces: &[Detection]) {
        assert_eq!(faces.len(), 1, "{faces:?}");
        let r = faces[0].rect;
        for (got, want) in [
            (r.left, 0.317),
            (r.top, 0.209),
            (r.right, 0.638),
            (r.bottom, 0.896),
        ] {
            assert!((got - want).abs() < 0.05, "{r:?}");
        }
        assert!(faces[0].score >= 0.85, "{}", faces[0].score);
    }

    #[test]
    fn a_portrait_has_one_face_where_it_is() {
        let faces = Detector::new().unwrap().detect(&portrait()).unwrap();
        assert_the_portraits_face(&faces);
        // The first two landmarks are the eyes: inside the box, in its upper half, the
        // image-left one first.
        let r = faces[0].rect;
        let (a, b) = (faces[0].landmarks[0], faces[0].landmarks[1]);
        assert!(a.0 < b.0);
        for (x, y) in [a, b] {
            assert!((r.left..r.right).contains(&(x as f64)), "{x}");
            assert!(
                (r.top..(r.top + r.bottom) / 2.0).contains(&(y as f64)),
                "{y}"
            );
        }
    }

    #[test]
    fn a_landscape_has_no_face() {
        let img = fixture("../xtask/screenshots/photos/14.jpg");
        assert!(Detector::new().unwrap().detect(&img).unwrap().is_empty());
    }

    /// The portrait on a taller canvas: the padding moves from the bottom of the input to
    /// its right, and the face must come back at the same place on the canvas. Fractions
    /// are of the image, never of the padded square. (Turning the portrait on its side was
    /// the first form of this test; a sideways face is not one the model finds reliably -
    /// 0.71 and a box 0.1 off one way, none at all the other - so it tested the model.)
    #[test]
    fn fractions_are_of_the_image_whatever_its_shape() {
        let detector = Detector::new().unwrap();
        let upright = detector.detect(&portrait()).unwrap();
        let mut canvas = image::RgbImage::new(960, 1280);
        image::imageops::overlay(&mut canvas, &portrait().to_rgb8(), 0, 0);
        let tall = detector.detect(&DynamicImage::ImageRgb8(canvas)).unwrap();
        assert_eq!((upright.len(), tall.len()), (1, 1));
        // The 640 rows of picture are the canvas's top half.
        let (u, t) = (upright[0].rect, tall[0].rect);
        for (got, want) in [
            (t.left, u.left),
            (t.top, u.top / 2.0),
            (t.right, u.right),
            (t.bottom, u.bottom / 2.0),
        ] {
            assert!((got - want).abs() < 0.05, "upright {u:?} tall {t:?}");
        }
    }

    /// A picture no larger than the input is padded, never scaled up, and its fractions are
    /// still of the picture. A third of the size - which fits even the small input, so
    /// neither run scales it - the same face.
    #[test]
    fn a_small_picture_is_not_scaled_up() {
        let small = portrait().resize(320, 320, image::imageops::FilterType::Triangle);
        assert_eq!(small.dimensions(), (320, 213));
        let faces = Detector::new().unwrap().detect(&small).unwrap();
        assert_the_portraits_face(&faces);
    }

    /// A face that fills the frame, as a head shot or a selfie does: the portrait cropped to
    /// its face and a fifth of the face's size around it (the bottom runs out first, so the
    /// face is three quarters of the frame's height), at a preview's 1600 px. At the large
    /// input alone this face, about 940 px tall there, is not found at all; the small run is
    /// what finds it.
    #[test]
    fn a_face_that_fills_the_frame_is_found() {
        let (x, y, w, h) = (242, 45, 432, 595);
        let close_up = portrait().crop_imm(x, y, w, h).resize(
            1600,
            1600,
            image::imageops::FilterType::CatmullRom,
        );
        assert_eq!(close_up.dimensions(), (1162, 1600));
        let faces = Detector::new().unwrap().detect(&close_up).unwrap();
        assert_eq!(faces.len(), 1, "{faces:?}");
        // The portrait's face, in fractions of the crop.
        let r = faces[0].rect;
        for (got, want) in [
            (r.left, (0.317 * 960.0 - x as f64) / w as f64),
            (r.top, (0.209 * 640.0 - y as f64) / h as f64),
            (r.right, (0.638 * 960.0 - x as f64) / w as f64),
            (r.bottom, (0.896 * 640.0 - y as f64) / h as f64),
        ] {
            assert!((got - want).abs() < 0.05, "{r:?}");
        }
    }

    /// A preview is 1600 px, so the pass always hands over a picture that is scaled down to
    /// the input: the same face, in fractions of the picture given and not of the input.
    #[test]
    fn a_picture_larger_than_the_input_is_scaled_down() {
        let large = portrait().resize(1600, 1600, image::imageops::FilterType::CatmullRom);
        assert_eq!(large.dimensions(), (1600, 1067));
        assert_the_portraits_face(&Detector::new().unwrap().detect(&large).unwrap());
    }

    /// A small face in a large picture, which only the large input finds (it is 10 px wide
    /// at the small one): the portrait at a sixth of its size, low and to the right of a
    /// black 1600 px picture. Away from the top left so that a fraction taken of anything
    /// but the picture as it was scaled to the input lands visibly elsewhere, and held to
    /// a tolerance that suits a face 0.03 of the picture wide.
    #[test]
    fn a_small_face_in_a_large_picture_is_found_where_it_is() {
        let small = portrait().resize(160, 160, image::imageops::FilterType::CatmullRom);
        assert_eq!(small.dimensions(), (160, 107));
        let (at_x, at_y) = (1200, 700);
        let mut canvas = image::RgbImage::new(1600, 1067);
        image::imageops::overlay(&mut canvas, &small.to_rgb8(), at_x, at_y);
        let faces = Detector::new()
            .unwrap()
            .detect(&DynamicImage::ImageRgb8(canvas))
            .unwrap();
        assert_eq!(faces.len(), 1, "{faces:?}");
        let r = faces[0].rect;
        for (got, want) in [
            (r.left, (at_x as f64 + 0.317 * 160.0) / 1600.0),
            (r.top, (at_y as f64 + 0.209 * 107.0) / 1067.0),
            (r.right, (at_x as f64 + 0.638 * 160.0) / 1600.0),
            (r.bottom, (at_y as f64 + 0.896 * 107.0) / 1067.0),
        ] {
            assert!((got - want).abs() < 0.01, "{r:?}");
        }
    }

    /// A strip whose short side would scale to under a pixel, and a single pixel: neither
    /// may reach the model as an image with a side of zero.
    #[test]
    fn a_thin_strip_is_detected_without_a_zero_side() {
        let strip = DynamicImage::ImageRgb8(image::RgbImage::new(6400, 2));
        assert!(Detector::new().unwrap().detect(&strip).unwrap().is_empty());
        let dot = DynamicImage::ImageRgb8(image::RgbImage::new(1, 1));
        assert!(Detector::new().unwrap().detect(&dot).unwrap().is_empty());
    }

    /// The cache hands back RGBA for a photo with transparency, and a greyscale picture is
    /// one channel. Both are the same face.
    #[test]
    fn alpha_and_greyscale_pictures_are_read() {
        let detector = Detector::new().unwrap();
        let rgba = DynamicImage::ImageRgba8(portrait().to_rgba8());
        assert_the_portraits_face(&detector.detect(&rgba).unwrap());
        let grey = DynamicImage::ImageLuma8(portrait().to_luma8());
        assert_eq!(detector.detect(&grey).unwrap().len(), 1);
    }

    /// `decode_level` indexes by the grid, so a tensor shorter than the grid implies would
    /// panic there; it is refused here instead, naming the output.
    #[test]
    fn a_short_output_tensor_is_an_error_not_a_panic() {
        let cols = INPUT / 32;
        let cells = cols * cols;
        let (cls, obj) = (vec![0.0; cells], vec![0.0; cells]);
        let (bbox, kps) = (vec![0.0; cells * 4], vec![0.0; cells * 10]);
        let level = |cls: &[f32], obj: &[f32], bbox: &[f32], kps: &[f32]| {
            check_lengths(
                &Level {
                    stride: 32,
                    cls,
                    obj,
                    bbox,
                    kps,
                },
                INPUT,
            )
        };
        assert!(level(&cls, &obj, &bbox, &kps).is_ok());
        let err = level(&cls, &obj, &bbox[1..], &kps).unwrap_err().to_string();
        assert!(err.contains("box"), "{err}");
        let err = level(&cls, &obj, &bbox, &kps[1..]).unwrap_err().to_string();
        assert!(err.contains("landmarks"), "{err}");
        assert!(level(&cls[1..], &obj, &bbox, &kps).is_err());
        assert!(level(&cls, &obj[1..], &bbox, &kps).is_err());
    }

    /// The bundled model parses and optimises at both input sizes the pass uses. This is the
    /// test CI runs on Windows and macOS to show `tract` builds and runs there at all.
    #[test]
    fn the_bundled_model_loads() {
        Detector::new().unwrap();
    }
}
