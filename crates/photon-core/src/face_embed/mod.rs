//! Telling faces apart. A picture and one face's landmarks in, a vector out: two faces of
//! one person give vectors pointing nearly the same way. Like `face_detect`, this knows
//! nothing about the library, and nothing outside it names `tract`.

use crate::{Error, Result};
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
    // Read from Task 3 of the people plan on; until then nothing calls it.
    #[allow(dead_code)]
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled model parses and optimises. This is the test CI runs on Windows and
    /// macOS to show a 39 MB model embedded in the binary builds and loads there.
    #[test]
    fn the_bundled_model_loads() {
        Embedder::new().unwrap();
    }
}
