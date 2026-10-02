//! Finding faces in a picture. Pixels in, rectangles out: this module knows nothing about
//! the library, the thumbnail cache or the engine, and nothing outside it names `tract`.

use crate::{Error, Result};
use decode::{Level, MAX_IOU, Raw, SCORE_THRESHOLD};
use image::DynamicImage;
use serde::Serialize;
use tract_onnx::prelude::*;

mod decode;
pub mod merge;

/// YuNet, from OpenCV's model zoo (`models/README.md`).
static MODEL: &[u8] = include_bytes!("../../models/face_detection_yunet_2023mar.onnx");

/// The side of the square the model is run at. A face narrower than about 15 px at this
/// size is missed: at 640 a group photo's faces are 14-21 px wide, on the edge, and at 320
/// the 29 people of the spike's test photograph came out as one.
pub const INPUT: usize = 1280;

/// Which detector looked at a photo: `items.face_version` records it. Bump it when the
/// model file, [`INPUT`], or the threshold or overlap limit in `decode` changes; every
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

/// The loaded model. Parsing and optimising it takes about 25 ms, so a pass makes one and
/// shares it between its workers.
pub struct Detector {
    run: Box<Run>,
}

fn model_error(err: impl std::fmt::Display) -> Error {
    Error::FaceModel(err.to_string())
}

impl Detector {
    pub fn new() -> Result<Self> {
        // The file declares a 640 input and the shapes that follow from it; left in, tract
        // refuses any other size ("Impossible to unify 320 with 160").
        let model = tract_onnx::onnx()
            .with_ignore_output_shapes(true)
            .with_ignore_value_info(true)
            .model_for_read(&mut std::io::Cursor::new(MODEL))
            .map_err(model_error)?
            .with_input_fact(0, f32::fact([1, 3, INPUT, INPUT]).into())
            .map_err(model_error)?
            .into_optimized()
            .map_err(model_error)?
            .into_runnable()
            .map_err(model_error)?;
        Ok(Self {
            run: Box::new(move |input| model.run(tvec!(input.into()))),
        })
    }

    /// The faces in `image`, strongest first.
    pub fn detect(&self, image: &DynamicImage) -> Result<Vec<Detection>> {
        // Scaled down to fit and never up: a face is found by its size in pixels, and
        // enlarging a small picture only invents them.
        let fitted = crate::decode::shrink_within(image, INPUT as u32).to_rgb8();
        // Top left of a black square, as the model was trained: BGR, 0..255, planar.
        let plane = INPUT * INPUT;
        let mut input = vec![0f32; 3 * plane];
        for (x, y, px) in fitted.enumerate_pixels() {
            let at = y as usize * INPUT + x as usize;
            input[at] = px[2] as f32;
            input[plane + at] = px[1] as f32;
            input[2 * plane + at] = px[0] as f32;
        }
        let tensor: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, INPUT, INPUT), input)
            .map_err(model_error)?
            .into();
        let out = (self.run)(tensor).map_err(model_error)?;
        // Twelve outputs: class, object, box and landmarks, each at the three strides.
        if out.len() != 12 {
            return Err(model_error(format!("{} outputs, expected 12", out.len())));
        }
        let flat = |i: usize| -> Result<Vec<f32>> {
            Ok(out[i]
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
            check_lengths(&level, INPUT)?;
            decode::decode_level(&level, INPUT, SCORE_THRESHOLD, &mut raw);
        }
        let (w, h) = (fitted.width() as f32, fitted.height() as f32);
        let unit = |v: f32, of: f32| (v / of).clamp(0.0, 1.0);
        Ok(decode::suppress(raw, MAX_IOU)
            .into_iter()
            .map(|f| Detection {
                rect: Rect {
                    left: unit(f.x, w) as f64,
                    top: unit(f.y, h) as f64,
                    right: unit(f.x + f.w, w) as f64,
                    bottom: unit(f.y + f.h, h) as f64,
                },
                landmarks: f.landmarks.map(|(x, y)| (unit(x, w), unit(y, h))),
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

    /// Review focus 1: a picture smaller than the input is padded, never scaled up, and
    /// its fractions are still of the picture. A third of the size, the same face.
    #[test]
    fn a_small_picture_is_not_scaled_up() {
        let small = portrait().resize(320, 320, image::imageops::FilterType::Triangle);
        assert_eq!(small.dimensions(), (320, 213));
        let faces = Detector::new().unwrap().detect(&small).unwrap();
        assert_the_portraits_face(&faces);
    }

    /// Review focus 2: a strip whose short side would scale to under a pixel.
    #[test]
    fn a_thin_strip_is_detected_without_a_zero_side() {
        let strip = DynamicImage::ImageRgb8(image::RgbImage::new(6400, 2));
        assert!(Detector::new().unwrap().detect(&strip).unwrap().is_empty());
        let dot = DynamicImage::ImageRgb8(image::RgbImage::new(1, 1));
        assert!(Detector::new().unwrap().detect(&dot).unwrap().is_empty());
    }

    /// Review focus 3: the cache hands back RGBA for a photo with transparency, and a
    /// greyscale picture is one channel. Both are the same face.
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

    /// The bundled model parses and optimises at the input size the pass uses. This is the
    /// test CI runs on Windows and macOS to show `tract` builds and runs there at all.
    #[test]
    fn the_bundled_model_loads() {
        Detector::new().unwrap();
    }
}
