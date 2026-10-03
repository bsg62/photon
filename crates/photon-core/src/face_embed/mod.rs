//! Telling faces apart. A picture and one face's landmarks in, a vector out: two faces of
//! one person give vectors pointing nearly the same way. Like `face_detect`, this knows
//! nothing about the library, and nothing outside it names `tract`.

mod align;
pub mod pass;

use crate::face_detect::Rect;
use crate::{Error, Result};
use image::DynamicImage;
use tract_onnx::prelude::*;

/// SFace, from OpenCV's model zoo (`models/README.md`).
static MODEL: &[u8] = include_bytes!("../../models/face_recognition_sface_2021dec.onnx");

/// Which embedder made a face's vector: `detected_faces.embedding_version` records it.
/// Bump it when the model file, the reference points or the sampling in `align`, or
/// [`MIN_FACE_PX`] changes; every face is then embedded again.
pub const EMBEDDER_VERSION: i64 = 1;

/// The narrowest face, in pixels of the picture it is cut from, that is embedded.
/// Measured 2026-10-03 on LFW faces shrunk into a 1600 px picture: at 35 px the same person
/// is still matched 90.8% of the time at 0.45, at 25 px 80%, at 18 px 58%, and two 18 px
/// faces start matching strangers (0.6% at 0.363). Below this a face stays detected and
/// counted, and is never grouped.
pub const MIN_FACE_PX: f32 = 35.0;

/// The length of a face's vector.
pub const DIM: usize = 128;

pub type Embedding = [f32; DIM];

/// The model's input side.
const SIDE: usize = 112;

type Run = dyn Fn(Tensor) -> TractResult<TVec<TValue>> + Send + Sync;

/// The loaded model. Parsing and optimising it takes a few tens of milliseconds; a pass
/// makes one and shares it between its workers.
pub struct Embedder {
    run: Box<Run>,
}

fn model_error(err: impl std::fmt::Display) -> Error {
    Error::FaceModel(err.to_string())
}

impl Embedder {
    pub fn new() -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(MODEL))
            .map_err(model_error)?
            .with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into())
            .map_err(model_error)?
            .into_optimized()
            .map_err(model_error)?
            .into_runnable()
            .map_err(model_error)?;
        Ok(Self {
            run: Box::new(move |input| model.run(tvec!(input.into()))),
        })
    }

    /// The face's vector, or `None` when it is narrower than [`MIN_FACE_PX`] in `image`.
    pub fn embed(&self, image: &DynamicImage, face: &FaceBox) -> Result<Option<Embedding>> {
        let (w, h) = (image.width() as f32, image.height() as f32);
        if ((face.rect.right - face.rect.left) as f32) * w < MIN_FACE_PX {
            return Ok(None);
        }
        let points = face.landmarks.map(|(x, y)| (x * w, y * h));
        let to_reference = align::fit(&points).ok_or_else(|| {
            model_error("a face's landmarks give no alignment: all in one place, or not numbers")
        })?;
        let input = align::sample(&image.to_rgb8(), &to_reference);
        let tensor: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, SIDE, SIDE), input)
            .map_err(model_error)?
            .into();
        let out = (self.run)(tensor).map_err(model_error)?;
        let raw: Vec<f32> = out
            .first()
            .ok_or_else(|| model_error("no output"))?
            .to_plain_array_view::<f32>()
            .map_err(model_error)?
            .iter()
            .copied()
            .collect();
        if raw.len() != DIM {
            return Err(model_error(format!(
                "{} numbers, expected {DIM}",
                raw.len()
            )));
        }
        let len = raw.iter().map(|x| x * x).sum::<f32>().sqrt();
        if !len.is_finite() || len == 0.0 {
            return Err(model_error("a vector that is not a number"));
        }
        let mut e = [0f32; DIM];
        for (slot, x) in e.iter_mut().zip(&raw) {
            *slot = x / len;
        }
        Ok(Some(e))
    }
}

/// One face as `detected_faces` stores it: fractions of the picture it was found in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceBox {
    pub rect: Rect,
    pub landmarks: [(f32, f32); 5],
}

/// How alike two vectors point: the cosine, 1 for the same direction. Either may be an
/// unnormalised sum (a group's centroid); zero length gives 0.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot = dot(a, b);
    let (la, lb) = (norm(a), norm(b));
    if la == 0.0 || lb == 0.0 {
        0.0
    } else {
        dot / (la * lb)
    }
}

/// The length of a vector.
pub fn norm(a: &[f32]) -> f32 {
    dot(a, a).sqrt()
}

/// The dot product of two vectors of one length, summed in eight lanes. A single running
/// sum is a chain the compiler may not reorder (float addition is not associative), so it
/// adds one product at a time; eight independent sums are a vector add per step. The
/// grouping step makes one comparison per face and group while it holds the library's
/// writer: measured 2026-10-03 (release, x86-64, 128 numbers) at about 9 ns with the
/// group's length kept, against 95 ns for the single sum with both lengths recomputed.
/// Summed in another order, the result can differ from the single sum in its last bits.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let ((a, a_tail), (b, b_tail)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    let mut lanes = [0f32; 8];
    for (x, y) in a.iter().zip(b) {
        for ((lane, x), y) in lanes.iter_mut().zip(x).zip(y) {
            *lane += x * y;
        }
    }
    let tail: f32 = a_tail.iter().zip(b_tail).map(|(x, y)| x * y).sum();
    lanes.iter().sum::<f32>() + tail
}

/// 128 little-endian `f32`, the form `detected_faces.embedding` holds.
pub fn to_blob(e: &Embedding) -> Vec<u8> {
    e.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_blob(bytes: &[u8]) -> Option<Embedding> {
    if bytes.len() != DIM * 4 {
        return None;
    }
    let mut e = [0f32; DIM];
    let (chunks, _) = bytes.as_chunks::<4>();
    for (slot, chunk) in e.iter_mut().zip(chunks) {
        *slot = f32::from_le_bytes(*chunk);
    }
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::face_detect::{Detector, Rect};
    use image::DynamicImage;
    use std::path::Path;

    fn fixture(name: &str) -> DynamicImage {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/faces")
            .join(name);
        image::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// The one face in a picture, as the pass would hand it over.
    fn the_face(detector: &Detector, img: &DynamicImage) -> FaceBox {
        let faces = detector.detect(img).unwrap();
        assert_eq!(faces.len(), 1, "{faces:?}");
        FaceBox {
            rect: faces[0].rect,
            landmarks: faces[0].landmarks,
        }
    }

    fn vector(embedder: &Embedder, detector: &Detector, img: &DynamicImage) -> Embedding {
        embedder
            .embed(img, &the_face(detector, img))
            .unwrap()
            .expect("large enough")
    }

    /// The pipeline is wired right: one person's face, the photo shrunk to half and saved
    /// again as a JPEG, still points the same way; another person's does not. (Whether
    /// recognition is good is the spike's evidence, in the spec, not this test's.)
    #[test]
    fn the_same_face_matches_and_another_does_not() {
        let (embedder, detector) = (Embedder::new().unwrap(), Detector::new().unwrap());
        let portrait = fixture("portrait.jpg");
        let copy = {
            let small = portrait.resize(480, 480, image::imageops::FilterType::Triangle);
            let mut bytes = Vec::new();
            small
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Jpeg,
                )
                .unwrap();
            image::load_from_memory(&bytes).unwrap()
        };
        let a = vector(&embedder, &detector, &portrait);
        let b = vector(&embedder, &detector, &copy);
        let other = vector(&embedder, &detector, &fixture("other.jpg"));
        assert!(
            similarity(&a, &b) > 0.9,
            "same face: {}",
            similarity(&a, &b)
        );
        assert!(
            similarity(&a, &other) < 0.363,
            "two people: {}",
            similarity(&a, &other)
        );
    }

    #[test]
    fn a_vector_has_unit_length() {
        let (embedder, detector) = (Embedder::new().unwrap(), Detector::new().unwrap());
        let a = vector(&embedder, &detector, &fixture("portrait.jpg"));
        let len = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-4, "{len}");
    }

    /// The floor is measured in pixels of the picture the face is cut from, across its width: the
    /// picture is not square, so a floor read off the height would see half the pixels.
    #[test]
    fn a_face_under_the_floor_is_not_embedded() {
        let embedder = Embedder::new().unwrap();
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(1000, 500));
        let at = |width: f64| FaceBox {
            rect: Rect {
                left: 0.5,
                top: 0.5,
                right: 0.5 + width,
                bottom: 0.5 + width,
            },
            landmarks: [
                (0.51, 0.51),
                (0.53, 0.51),
                (0.52, 0.52),
                (0.51, 0.53),
                (0.53, 0.53),
            ],
        };
        assert_eq!(embedder.embed(&img, &at(0.034)).unwrap(), None); // 34 px
        assert!(embedder.embed(&img, &at(0.036)).unwrap().is_some()); // 36 px
    }

    #[test]
    fn similarity_is_the_cosine() {
        assert!((similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-6);
        assert!((similarity(&[2.0, 0.0], &[3.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    /// Eight lanes and the tail past them both count: 1² + 2² + ... + 11² is 506.
    #[test]
    fn the_dot_product_counts_every_number() {
        let v: Vec<f32> = (1..=11).map(|x| x as f32).collect();
        assert_eq!(dot(&v, &v), 506.0);
    }

    #[test]
    fn a_vector_round_trips_through_its_blob() {
        let mut e = [0f32; DIM];
        e[0] = 0.25;
        e[127] = -1.5;
        let blob = to_blob(&e);
        assert_eq!(blob.len(), 512);
        assert_eq!(from_blob(&blob), Some(e));
        assert_eq!(from_blob(&blob[..511]), None);
    }

    /// The bundled model parses and optimises. This is the test CI runs on Windows and
    /// macOS to show a 39 MB model embedded in the binary builds and loads there.
    #[test]
    fn the_bundled_model_loads() {
        Embedder::new().unwrap();
    }
}
